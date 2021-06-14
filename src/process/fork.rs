use std::fs;
use std::io::prelude::*;
use std::path::Path;
use std::process::exit;

use anyhow::Result;

use anyhow::bail;
use child::ChildProcess;
use init::InitProcess;
use nix::sched;
use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd;
use nix::unistd::Pid;

use crate::cgroups::common::CgroupManager;
use crate::container::ContainerStatus;
use crate::process::{child, init, parent, Process};
use crate::{cond::Cond, container::Container};

/// Function to perform the first fork for in order to run the container process
pub fn fork_first<P: AsRef<Path>>(
    pid_file: Option<P>,
    is_userns: bool,
    linux: &oci_spec::Linux,
    container: &Container,
    cmanager: Box<dyn CgroupManager>,
) -> Result<Process> {
    // create a new pipe
    let ccond = Cond::new()?;

    // create new parent process structure
    let (mut parent, sender_for_parent) = parent::ParentProcess::new()?;
    // create a new child process structure with sending end of parent process
    let child = child::ChildProcess::new(sender_for_parent)?;

    // fork the process
    match unsafe { unistd::fork()? } {
        // in the child process
        unistd::ForkResult::Child => {
            // if Out-of-memory score adjustment is set in specification.
            // set the score value for the current process
            // check https://dev.to/rrampage/surviving-the-linux-oom-killer-2ki9 for some more information
            if let Some(ref r) = linux.resources {
                if let Some(adj) = r.oom_score_adj {
                    let mut f = fs::File::create("/proc/self/oom_score_adj")?;
                    f.write_all(adj.to_string().as_bytes())?;
                }
            }

            // if new user is specified in specification, this will be true
            // and new namespace will be created, check https://man7.org/linux/man-pages/man7/user_namespaces.7.html
            // for more information
            if is_userns {
                sched::unshare(sched::CloneFlags::CLONE_NEWUSER)?;
            }

            ccond.notify()?;
            Ok(Process::Child(child))
        }
        // in the parent process
        unistd::ForkResult::Parent { child } => {
            ccond.wait()?;

            // wait for child to fork init process and report back its pid
            let init_pid = parent.wait_for_child_ready()?;
            log::debug!("init pid is {:?}", init_pid);
            cmanager.apply(&linux.resources.as_ref().unwrap(), Pid::from_raw(init_pid))?;

            // update status and pid of the container process
            container
                .update_status(ContainerStatus::Created)?
                .set_pid(init_pid)
                .save()?;
            // if file to write the pid to is specified, write pid of the child
            if let Some(pid_file) = pid_file {
                fs::write(&pid_file, format!("{}", child))?;
            }
            Ok(Process::Parent(parent))
        }
    }
}

/// Function to perform the second fork, which will spawn the actual container process
pub fn fork_init(mut child_process: ChildProcess) -> Result<Process> {
    // setup sockets for init process
    let sender_for_child = child_process.setup_pipe()?;
    // for the process into current process (C1) (which is child of first_fork) and init process
    match unsafe { unistd::fork()? } {
        // if it is child process, create new InitProcess structure and return
        unistd::ForkResult::Child => Ok(Process::Init(InitProcess::new(sender_for_child))),
        // in the forking process C1
        unistd::ForkResult::Parent { child } => {
            // wait for init process to be ready
            child_process.wait_for_init_ready()?;
            // notify the parent process (original youki process) that init process is forked and ready
            child_process.notify_parent(child)?;

            // wait for the init process, which is container process, to change state
            // check https://man7.org/linux/man-pages/man3/wait.3p.html for more information
            match waitpid(child, None)? {
                // if normally exited
                WaitStatus::Exited(pid, status) => {
                    log::debug!("exited pid: {:?}, status: {:?}", pid, status);
                    exit(status);
                }
                // if terminated by a signal
                WaitStatus::Signaled(pid, status, _) => {
                    log::debug!("signaled pid: {:?}, status: {:?}", pid, status);
                    exit(0);
                }
                _ => bail!("abnormal exited!"),
            }
        }
    }
}
