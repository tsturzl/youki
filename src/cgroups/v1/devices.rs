use std::{fs::create_dir_all, path::Path};

use anyhow::Result;
use nix::unistd::Pid;

use crate::cgroups::common::{self, CGROUP_PROCS};
use crate::{cgroups::v1::Controller, rootfs::default_devices};
use oci_spec::{LinuxDeviceCgroup, LinuxDeviceType, LinuxResources};

pub struct Devices {}

impl Controller for Devices {
    fn apply(linux_resources: &LinuxResources, cgroup_root: &Path, pid: Pid) -> Result<()> {
        log::debug!("Apply Devices cgroup config");
        create_dir_all(&cgroup_root)?;

        for d in &linux_resources.devices {
            Self::apply_device(d, cgroup_root)?;
        }

        for d in [
            default_devices().iter().map(|d| d.into()).collect(),
            Self::default_allow_devices(),
        ]
        .concat()
        {
            Self::apply_device(&d, &cgroup_root)?;
        }

        common::write_cgroup_file(cgroup_root.join(CGROUP_PROCS), pid)?;
        Ok(())
    }
}

impl Devices {
    fn apply_device(device: &LinuxDeviceCgroup, cgroup_root: &Path) -> Result<()> {
        let path = if device.allow {
            cgroup_root.join("devices.allow")
        } else {
            cgroup_root.join("devices.deny")
        };

        common::write_cgroup_file_str(path, &device.to_string())?;
        Ok(())
    }

    fn default_allow_devices() -> Vec<LinuxDeviceCgroup> {
        vec![
            LinuxDeviceCgroup {
                allow: true,
                typ: LinuxDeviceType::C,
                major: None,
                minor: None,
                access: "m".to_string(),
            },
            LinuxDeviceCgroup {
                allow: true,
                typ: LinuxDeviceType::B,
                major: None,
                minor: None,
                access: "m".to_string(),
            },
            // /dev/console
            LinuxDeviceCgroup {
                allow: true,
                typ: LinuxDeviceType::C,
                major: Some(5),
                minor: Some(1),
                access: "rwm".to_string(),
            },
            // /dev/pts
            LinuxDeviceCgroup {
                allow: true,
                typ: LinuxDeviceType::C,
                major: Some(136),
                minor: None,
                access: "rwm".to_string(),
            },
            LinuxDeviceCgroup {
                allow: true,
                typ: LinuxDeviceType::C,
                major: Some(5),
                minor: Some(2),
                access: "rwm".to_string(),
            },
            // tun/tap
            LinuxDeviceCgroup {
                allow: true,
                typ: LinuxDeviceType::C,
                major: Some(10),
                minor: Some(200),
                access: "rwm".to_string(),
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cgroups::test::{create_temp_dir, set_fixture};
    use oci_spec::{LinuxDeviceCgroup, LinuxDeviceType};
    use std::fs::read_to_string;

    #[test]
    fn test_set_default_devices() {
        let tmp =
            create_temp_dir("test_set_default_devices").expect("create temp directory for test");

        Devices::default_allow_devices().iter().for_each(|d| {
            // NOTE: We reset the fixtures every iteration because files aren't appended
            // so what happens in the tests is you get strange overwrites which can contain
            // remaining bytes from the last iteration. Resetting the files more appropriately
            // mocks the behavior of cgroup files.
            set_fixture(&tmp, "devices.allow", "").expect("create allowed devices list");
            set_fixture(&tmp, "devices.deny", "").expect("create denied devices list");

            Devices::apply_device(&d, &tmp).expect("Apply default device");
            println!("Device: {}", d.to_string());
            if d.allow {
                let allowed_content =
                    read_to_string(tmp.join("devices.allow")).expect("read to string");
                assert_eq!(allowed_content, d.to_string());
            } else {
                let denied_content =
                    read_to_string(tmp.join("devices.deny")).expect("read to string");
                assert_eq!(denied_content, d.to_string());
            }
        });
    }

    #[test]
    fn test_set_mock_devices() {
        let tmp = create_temp_dir("test_set_mock_devices").expect("create temp directory for test");
        [
            LinuxDeviceCgroup {
                allow: true,
                typ: LinuxDeviceType::C,
                major: Some(10),
                minor: None,
                access: "rwm".to_string(),
            },
            LinuxDeviceCgroup {
                allow: true,
                typ: LinuxDeviceType::A,
                major: None,
                minor: Some(200),
                access: "rwm".to_string(),
            },
            LinuxDeviceCgroup {
                allow: false,
                typ: LinuxDeviceType::P,
                major: Some(10),
                minor: Some(200),
                access: "m".to_string(),
            },
            LinuxDeviceCgroup {
                allow: false,
                typ: LinuxDeviceType::U,
                major: None,
                minor: None,
                access: "rw".to_string(),
            },
        ]
        .iter()
        .for_each(|d| {
            set_fixture(&tmp, "devices.allow", "").expect("create allowed devices list");
            set_fixture(&tmp, "devices.deny", "").expect("create denied devices list");

            Devices::apply_device(&d, &tmp).expect("Apply default device");
            println!("Device: {}", d.to_string());
            if d.allow {
                let allowed_content =
                    read_to_string(tmp.join("devices.allow")).expect("read to string");
                assert_eq!(allowed_content, d.to_string());
            } else {
                let denied_content =
                    read_to_string(tmp.join("devices.deny")).expect("read to string");
                assert_eq!(denied_content, d.to_string());
            }
        });
    }

    quickcheck! {
        fn property_test_apply_device(device: LinuxDeviceCgroup) -> bool {
            let tmp = create_temp_dir("property_test_apply_device").expect("create temp directory for test");
            set_fixture(&tmp, "devices.allow", "").expect("create allowed devices list");
            set_fixture(&tmp, "devices.deny", "").expect("create denied devices list");
            Devices::apply_device(&device, &tmp).expect("Apply default device");
            if device.allow {
                let allowed_content =
                    read_to_string(tmp.join("devices.allow")).expect("read to string");
                allowed_content == device.to_string()
            } else {
                let denied_content =
                    read_to_string(tmp.join("devices.deny")).expect("read to string");
                denied_content == device.to_string()
            }
        }

        fn property_test_apply_multiple_devices(devices: Vec<LinuxDeviceCgroup>) -> bool {
            let tmp = create_temp_dir("property_test_apply_multiple_devices").expect("create temp directory for test");
            devices.iter()
                .map(|device| {
                    set_fixture(&tmp, "devices.allow", "").expect("create allowed devices list");
                    set_fixture(&tmp, "devices.deny", "").expect("create denied devices list");
                    Devices::apply_device(&device, &tmp).expect("Apply default device");
                    if device.allow {
                        let allowed_content =
                            read_to_string(tmp.join("devices.allow")).expect("read to string");
                        allowed_content == device.to_string()
                    } else {
                        let denied_content =
                            read_to_string(tmp.join("devices.deny")).expect("read to string");
                        denied_content == device.to_string()
                    }
                })
                .all(|is_ok| is_ok)
        }
    }
}
