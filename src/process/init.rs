use anyhow::{bail, Context, Result};
use nix::mount::mount as nix_mount;
use nix::mount::MsFlags;
use nix::{
    fcntl, sched,
    sys::statfs,
    unistd::{Gid, Uid},
};
use oci_spec::Spec;
use std::collections::HashMap;
use std::{
    env,
    os::unix::{io::AsRawFd, prelude::RawFd},
};
use std::{fs, io::Write, path::Path, path::PathBuf};

use crate::{
    capabilities,
    container::Container,
    hooks,
    namespaces::Namespaces,
    notify_socket::NotifyListener,
    process::child,
    rootfs,
    syscall::{linux::LinuxSyscall, Syscall},
    tty, utils,
};

// Make sure a given path is on procfs. This is to avoid the security risk that
// /proc path is mounted over. Ref: CVE-2019-16884
fn ensure_procfs(path: &Path) -> Result<()> {
    let procfs_fd = fs::File::open(path)?;
    let fstat_info = statfs::fstatfs(&procfs_fd.as_raw_fd())?;

    if fstat_info.filesystem_type() != statfs::PROC_SUPER_MAGIC {
        bail!(format!("{:?} is not on the procfs", path));
    }

    Ok(())
}

// Get a list of open fds for the calling process.
fn get_open_fds() -> Result<Vec<i32>> {
    const PROCFS_FD_PATH: &str = "/proc/self/fd";
    ensure_procfs(Path::new(PROCFS_FD_PATH))
        .with_context(|| format!("{} is not the actual procfs", PROCFS_FD_PATH))?;

    let fds: Vec<i32> = fs::read_dir(PROCFS_FD_PATH)?
        .filter_map(|entry| match entry {
            Ok(entry) => Some(entry.path()),
            Err(_) => None,
        })
        .filter_map(|path| path.file_name().map(|file_name| file_name.to_owned()))
        .filter_map(|file_name| file_name.to_str().map(String::from))
        .filter_map(|file_name| -> Option<i32> {
            // Convert the file name from string into i32. Since we are looking
            // at /proc/<pid>/fd, anything that's not a number (i32) can be
            // ignored. We are only interested in opened fds.
            match file_name.parse() {
                Ok(fd) => Some(fd),
                Err(_) => None,
            }
        })
        .collect();

    Ok(fds)
}

// Cleanup any extra file descriptors, so the new container process will not
// leak a file descriptor from before execve gets executed. The first 3 fd will
// stay open: stdio, stdout, and stderr. We would further preserve the next
// "preserve_fds" number of fds. Set the rest of fd with CLOEXEC flag, so they
// will be closed after execve into the container payload. We can't close the
// fds immediatly since we at least still need it for the pipe used to wait on
// starting the container.
fn cleanup_file_descriptors(preserve_fds: i32) -> Result<()> {
    let open_fds = get_open_fds().with_context(|| "Failed to obtain opened fds")?;
    // Include stdin, stdout, and stderr for fd 0, 1, and 2 respectively.
    let min_fd = preserve_fds + 3;
    let to_be_cleaned_up_fds: Vec<i32> = open_fds
        .iter()
        .filter_map(|&fd| if fd >= min_fd { Some(fd) } else { None })
        .collect();

    to_be_cleaned_up_fds.iter().for_each(|&fd| {
        // Intentionally ignore errors here -- the cases where this might fail
        // are basically file descriptors that have already been closed.
        let _ = fcntl::fcntl(fd, fcntl::F_SETFD(fcntl::FdFlag::FD_CLOEXEC));
    });

    Ok(())
}

pub struct ContainerInitArgs {
    /// Flag indicating if an init or a tenant container should be created
    pub init: bool,
    /// Interface to operating system primitives
    pub syscall: LinuxSyscall,
    /// OCI complient runtime spec
    pub spec: Spec,
    /// Root filesystem of the container
    pub rootfs: PathBuf,
    /// Socket to communicate the file descriptor of the ptty
    pub console_socket: Option<RawFd>,
    /// Options for rootless containers
    pub is_rootless: bool,
    /// Path to the Unix Domain Socket to communicate container start
    pub notify_path: PathBuf,
    /// File descriptos preserved/passed to the container init process.
    pub preserve_fds: i32,
    /// Container state
    pub container: Option<Container>,
    /// Pipe used to communicate with the child process
    pub child: child::ChildProcess,
}

pub fn container_init(args: ContainerInitArgs) -> Result<()> {
    let command = &args.syscall;
    let spec = &args.spec;
    let linux = spec.linux.as_ref().context("no linux in spec")?;
    // need to create the notify socket before we pivot root, since the unix
    // domain socket used here is outside of the rootfs of container
    let mut notify_socket: NotifyListener = NotifyListener::new(&args.notify_path)?;
    let proc = spec.process.as_ref().context("no process in spec")?;
    let mut envs: Vec<String> = proc.env.as_ref().unwrap_or(&vec![]).clone();
    let rootfs = &args.rootfs;
    let hooks = spec.hooks.as_ref();
    let container = args.container.as_ref();
    let mut child = args.child;

    // if Out-of-memory score adjustment is set in specification.  set the score
    // value for the current process check
    // https://dev.to/rrampage/surviving-the-linux-oom-killer-2ki9 for some more
    // information
    if let Some(ref resource) = linux.resources {
        if let Some(oom_score_adj) = resource.oom_score_adj {
            let mut f = fs::File::create("/proc/self/oom_score_adj")?;
            f.write_all(oom_score_adj.to_string().as_bytes())?;
        }
    }

    // if new user is specified in specification, this will be true and new
    // namespace will be created, check
    // https://man7.org/linux/man-pages/man7/user_namespaces.7.html for more
    // information
    if args.is_rootless {
        // child needs to be dumpable, otherwise the non root parent is not
        // allowed to write the uid/gid maps
        prctl::set_dumpable(true).unwrap();
        child.request_identifier_mapping()?;
        child.wait_for_mapping_ack()?;
        prctl::set_dumpable(false).unwrap();
    }

    // set limits and namespaces to the process
    if let Some(rlimits) = proc.rlimits.as_ref() {
        for rlimit in rlimits.iter() {
            command.set_rlimit(rlimit).context("failed to set rlimit")?;
        }
    }

    command
        .set_id(Uid::from_raw(0), Gid::from_raw(0))
        .context("failed to become root")?;

    // set up tty if specified
    if let Some(csocketfd) = args.console_socket {
        tty::setup_console(&csocketfd)?;
    }

    // join existing namespaces
    let bind_service = if let Some(ns) = linux.namespaces.as_ref() {
        let namespaces = Namespaces::from(ns);
        namespaces.apply_setns()?;
        namespaces
            .clone_flags
            .contains(sched::CloneFlags::CLONE_NEWUSER)
    } else {
        false
    };

    if let Some(hostname) = spec.hostname.as_ref() {
        command.set_hostname(hostname)?;
    }

    if let Some(true) = proc.no_new_privileges {
        let _ = prctl::set_no_new_privileges(true);
    }

    if args.init {
        // create_container hook needs to be called after the namespace setup, but
        // before pivot_root is called. This runs in the container namespaces.
        if let Some(hooks) = hooks {
            hooks::run_hooks(hooks.create_container.as_ref(), container)?
        }
        rootfs::prepare_rootfs(spec, rootfs, bind_service)
            .with_context(|| "Failed to prepare rootfs")?;

        // change the root of filesystem of the process to the rootfs
        command
            .pivot_rootfs(rootfs)
            .with_context(|| format!("Failed to pivot root to {:?}", rootfs))?;

        if let Some(kernel_params) = &linux.sysctl {
            sysctl(kernel_params)?;
        }
    }

    if let Some(paths) = &linux.readonly_paths {
        // mount readonly path
        for path in paths {
            readonly_path(path)?;
        }
    }

    command.set_id(Uid::from_raw(proc.user.uid), Gid::from_raw(proc.user.gid))?;
    capabilities::reset_effective(command)?;
    if let Some(caps) = &proc.capabilities {
        capabilities::drop_privileges(caps, command)?;
    }

    // Take care of LISTEN_FDS used for systemd-active-socket. If the value is
    // not 0, then we have to preserve those fds as well, and set up the correct
    // environment variables.
    let preserve_fds: i32 = match env::var("LISTEN_FDS") {
        Ok(listen_fds_str) => {
            let listen_fds = match listen_fds_str.parse::<i32>() {
                Ok(v) => v,
                Err(error) => {
                    log::warn!(
                        "LISTEN_FDS entered is not a fd. Ignore the value. {:?}",
                        error
                    );

                    0
                }
            };

            // The LISTEN_FDS will have to be passed to container init process.
            // The LISTEN_PID will be set to PID 1. Based on the spec, if
            // LISTEN_FDS is 0, the variable should be unset, so we just ignore
            // it here, if it is 0.
            if listen_fds > 0 {
                envs.append(&mut vec![
                    format!("LISTEN_FDS={}", listen_fds),
                    "LISTEN_PID=1".to_string(),
                ]);
            }

            args.preserve_fds + listen_fds
        }
        Err(env::VarError::NotPresent) => args.preserve_fds,
        Err(env::VarError::NotUnicode(value)) => {
            log::warn!(
                "LISTEN_FDS entered is malformed: {:?}. Ignore the value.",
                &value
            );
            args.preserve_fds
        }
    };

    // clean up and handle perserved fds.
    if args.init {
        cleanup_file_descriptors(preserve_fds).with_context(|| "Failed to clean up extra fds")?;
    }

    // Reset the process env based on oci spec.
    env::vars().for_each(|(key, _value)| std::env::remove_var(key));
    utils::parse_env(&envs)
        .iter()
        .for_each(|(key, value)| env::set_var(key, value));

    // notify parents that the init process is ready to execute the payload.
    child.notify_parent()?;

    // listing on the notify socket for container start command
    notify_socket.wait_for_container_start()?;

    // create_container hook needs to be called after the namespace setup, but
    // before pivot_root is called. This runs in the container namespaces.
    if args.init {
        if let Some(hooks) = hooks {
            hooks::run_hooks(hooks.start_container.as_ref(), container)?
        }
    }

    if let Some(args) = proc.args.as_ref() {
        utils::do_exec(&args[0], args)?;
    } else {
        bail!("On non-Windows, at least one process arg entry is required.")
    }

    // After do_exec is called, the process is replaced with the container
    // payload through execvp, so it should never reach here.
    unreachable!();
}

fn sysctl(kernel_params: &HashMap<String, String>) -> Result<()> {
    let sys = PathBuf::from("/proc/sys");
    for (kernel_param, value) in kernel_params {
        let path = sys.join(kernel_param.replace(".", "/"));
        log::debug!(
            "apply value {} to kernel parameter {}.",
            value,
            kernel_param
        );
        fs::write(path, value.as_bytes())
            .with_context(|| format!("failed to set sysctl {}={}", kernel_param, value))?;
    }

    Ok(())
}

fn readonly_path(path: &str) -> Result<()> {
    match nix_mount::<str, str, str, str>(
        Some(path),
        path,
        None::<&str>,
        MsFlags::MS_BIND
            | MsFlags::MS_REC
            | MsFlags::MS_NOSUID
            | MsFlags::MS_NODEV
            | MsFlags::MS_NOEXEC
            | MsFlags::MS_BIND
            | MsFlags::MS_RDONLY,
        None::<&str>,
    ) {
        // ignore error if path is not exist.
        Err(nix::errno::Errno::ENOENT) => {
            log::warn!("readonly path {:?} not exist", path);
            return Ok(());
        }
        Err(err) => bail!(err),
        Ok(_) => {}
    }
    log::debug!("readonly path {:?} mounted", path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{bail, Result};
    use nix::{fcntl, sys, unistd};
    use std::fs;

    #[test]
    fn test_get_open_fds() -> Result<()> {
        let file = fs::File::open("/dev/null")?;
        let fd = file.as_raw_fd();
        let open_fds = super::get_open_fds()?;

        if !open_fds.iter().any(|&v| v == fd) {
            bail!("Failed to find the opened dev null fds: {:?}", open_fds);
        }

        // explicitly close the file before the test case returns.
        drop(file);

        // The stdio fds should also be contained in the list of opened fds.
        if !vec![0, 1, 2]
            .iter()
            .all(|&stdio_fd| open_fds.iter().any(|&open_fd| open_fd == stdio_fd))
        {
            bail!("Failed to find the stdio fds: {:?}", open_fds);
        }

        Ok(())
    }

    #[test]
    fn test_cleanup_file_descriptors() -> Result<()> {
        // Open a fd without the CLOEXEC flag. Rust automatically adds the flag,
        // so we use fcntl::open here for more control.
        let fd = fcntl::open("/dev/null", fcntl::OFlag::O_RDWR, sys::stat::Mode::empty())?;
        cleanup_file_descriptors(fd - 1).with_context(|| "Failed to clean up the fds")?;
        let fd_flag = fcntl::fcntl(fd, fcntl::F_GETFD)?;
        if (fd_flag & fcntl::FdFlag::FD_CLOEXEC.bits()) != 0 {
            bail!("CLOEXEC flag is not set correctly");
        }

        unistd::close(fd)?;
        Ok(())
    }
}
