//! Utility functionality

use std::env;
use std::ffi::CString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Result};
use nix::unistd;

pub trait PathBufExt {
    fn as_in_container(&self) -> Result<PathBuf>;
    fn join_absolute_path(&self, p: &Path) -> Result<PathBuf>;
}

impl PathBufExt for PathBuf {
    fn as_in_container(&self) -> Result<PathBuf> {
        if self.is_relative() {
            bail!("Relative path cannot be converted to the path in the container.")
        } else {
            let path_string = self.to_string_lossy().into_owned();
            Ok(PathBuf::from(path_string[1..].to_string()))
        }
    }

    fn join_absolute_path(&self, p: &Path) -> Result<PathBuf> {
        if !p.is_absolute() && !p.as_os_str().is_empty() {
            bail!(
                "cannot join {:?} because it is not the absolute path.",
                p.display()
            )
        }
        Ok(PathBuf::from(format!("{}{}", self.display(), p.display())))
    }
}

pub fn do_exec(path: impl AsRef<Path>, args: &[String], envs: &[String]) -> Result<()> {
    let p = CString::new(path.as_ref().to_string_lossy().to_string())?;
    let a: Vec<CString> = args
        .iter()
        .map(|s| CString::new(s.to_string()).unwrap_or_default())
        .collect();

    // clear env vars
    env::vars().for_each(|(key, _value)| std::env::remove_var(key));
    // set env vars
    envs.iter().for_each(|e| {
        let mut split = e.split("=");
        match split.next() {
            Some(key) => {
                let value: String = split.collect::<Vec<&str>>().join("=");
                env::set_var(key, value)
            }
            None => {}
        };
    });

    unistd::execvp(&p, &a)?;
    Ok(())
}

// TODO implement
pub fn set_name(_name: &str) -> Result<()> {
    Ok(())
}

/// If None, it will generate a default path for cgroups.
pub fn get_cgroup_path(cgroups_path: &Option<PathBuf>, container_id: &str) -> PathBuf {
    match cgroups_path {
        Some(cpath) => cpath.clone(),
        None => PathBuf::from(format!("/youki/{}", container_id)),
    }
}

pub fn delete_with_retry<P: AsRef<Path>>(path: P) -> Result<()> {
    let mut attempts = 0;
    let mut delay = Duration::from_millis(10);
    let path = path.as_ref();

    while attempts < 5 {
        if fs::remove_dir(path).is_ok() {
            return Ok(());
        }

        std::thread::sleep(delay);
        attempts += attempts;
        delay *= attempts;
    }

    bail!("could not delete {:?}", path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_join_absolute_path() {
        assert_eq!(
            PathBuf::from("sample/a/")
                .join_absolute_path(&PathBuf::from("/b"))
                .unwrap(),
            PathBuf::from("sample/a/b")
        );
    }

    #[test]
    fn test_join_absolute_path_error() {
        assert_eq!(
            PathBuf::from("sample/a/")
                .join_absolute_path(&PathBuf::from("b/c"))
                .is_err(),
            true
        );
    }

    #[test]
    fn test_get_cgroup_path() {
        let cid = "sample_container_id";
        assert_eq!(
            get_cgroup_path(&None, cid),
            PathBuf::from("/youki/sample_container_id")
        );
        assert_eq!(
            get_cgroup_path(&Some(PathBuf::from("/youki")), cid),
            PathBuf::from("/youki")
        );
    }
}
