use std::io::Write;
use std::{
    fs::{create_dir_all, OpenOptions},
    path::Path,
};

use anyhow::Result;
use nix::unistd::Pid;

use crate::{
    cgroups::Controller,
    spec::{LinuxNetwork, LinuxResources},
};

pub struct NetworkClassifier {}

impl Controller for NetworkClassifier {
    fn apply(linux_resources: &LinuxResources, cgroup_root: &Path, pid: Pid) -> Result<()> {
        create_dir_all(&cgroup_root)?;

        Self::apply(cgroup_root, linux_resources.network.as_ref().unwrap())?;

        OpenOptions::new()
            .create(false)
            .write(true)
            .truncate(false)
            .open(cgroup_root.join("cgroup.procs"))?
            .write_all(pid.to_string().as_bytes())?;

        Ok(())
    }
}

impl NetworkClassifier {
    fn apply(root_path: &Path, network: &LinuxNetwork) -> Result<()> {
        if let Some(class_id) = network.class_id {
            Self::write_file(&root_path.join("net_cls.classid"), &class_id.to_string())?;
        }

        Ok(())
    }

    fn write_file(file_path: &Path, data: &str) -> Result<()> {
        OpenOptions::new()
            .create(false)
            .write(true)
            .truncate(true)
            .open(file_path)?
            .write_all(data.as_bytes())?;

        Ok(())
    }
}
