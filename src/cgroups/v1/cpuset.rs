use std::{fs, path::Path};

use anyhow::{bail, Result};
use nix::unistd::Pid;
use oci_spec::{LinuxCpu, LinuxResources};

use crate::cgroups::common::{self, CGROUP_PROCS};

use super::{util, Controller, ControllerType};

const CGROUP_CPUSET_CPUS: &str = "cpuset.cpus";
const CGROUP_CPUSET_MEMS: &str = "cpuset.mems";

pub struct CpuSet {}

impl Controller for CpuSet {
    fn apply(linux_resources: &LinuxResources, cgroup_path: &Path, pid: Pid) -> Result<()> {
        log::debug!("Apply CpuSet cgroup config");
        fs::create_dir_all(cgroup_path)?;

        Self::ensure_not_empty(cgroup_path, CGROUP_CPUSET_CPUS)?;
        Self::ensure_not_empty(cgroup_path, CGROUP_CPUSET_MEMS)?;

        if let Some(cpuset) = &linux_resources.cpu {
            Self::apply(cgroup_path, cpuset)?;
        }

        common::write_cgroup_file(cgroup_path.join(CGROUP_PROCS), pid)?;
        Ok(())
    }
}

impl CpuSet {
    fn apply(cgroup_path: &Path, cpuset: &LinuxCpu) -> Result<()> {
        if let Some(cpus) = &cpuset.cpus {
            common::write_cgroup_file_str(cgroup_path.join(CGROUP_CPUSET_CPUS), cpus)?;
        }

        if let Some(mems) = &cpuset.mems {
            common::write_cgroup_file_str(cgroup_path.join(CGROUP_CPUSET_MEMS), mems)?;
        }

        Ok(())
    }

    // if a task is moved into the cgroup and a value has not been set for cpus and mems
    // Errno 28 (no space left on device) will be returned. Therefore we set the value from the parent if required.
    fn ensure_not_empty(cgroup_path: &Path, interface_file: &str) -> Result<()> {
        let mut current = util::get_subsystem_mount_points(&ControllerType::CpuSet.to_string())?;
        let relative_cgroup_path = cgroup_path.strip_prefix(&current)?;

        for component in relative_cgroup_path.components() {
            let parent_value = fs::read_to_string(current.join(interface_file))?;
            if parent_value.trim().is_empty() {
                bail!("cpuset parent value is empty")
            }

            current.push(component);
            let child_path = current.join(interface_file);
            let child_value = fs::read_to_string(&child_path)?;
            // the file can contain a newline character. Need to trim it away,
            // otherwise it is not considered empty and value will not be written
            if child_value.trim().is_empty() {
                common::write_cgroup_file_str(&child_path, &parent_value)?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::cgroups::test::{setup, LinuxCpuBuilder};

    #[test]
    fn test_set_cpus() {
        // arrange
        let (tmp, cpus) = setup("test_set_cpus", CGROUP_CPUSET_CPUS);
        let cpuset = LinuxCpuBuilder::new().with_cpus("1-3".to_owned()).build();

        // act
        CpuSet::apply(&tmp, &cpuset).expect("apply cpuset");

        // assert
        let content = fs::read_to_string(&cpus)
            .unwrap_or_else(|_| panic!("read {} file content", CGROUP_CPUSET_CPUS));
        assert_eq!(content, "1-3");
    }

    #[test]
    fn test_set_mems() {
        // arrange
        let (tmp, mems) = setup("test_set_mems", CGROUP_CPUSET_MEMS);
        let cpuset = LinuxCpuBuilder::new().with_mems("1-3".to_owned()).build();

        // act
        CpuSet::apply(&tmp, &cpuset).expect("apply cpuset");

        // assert
        let content = fs::read_to_string(&mems)
            .unwrap_or_else(|_| panic!("read {} file content", CGROUP_CPUSET_MEMS));
        assert_eq!(content, "1-3");
    }
}
