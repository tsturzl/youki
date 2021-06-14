//! Contains a wrapper of syscalls for unit tests
//! This provides a uniform interface for rest of Youki
//! to call syscalls required for container management

#[allow(clippy::module_inception)]
mod command;
pub mod linux;
pub mod test;

pub use command::Command;
