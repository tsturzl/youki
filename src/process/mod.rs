//! Provides a thin wrapper around fork syscall,
//! with enums and functions specific to youki implemented

use std::time::Duration;

pub mod child;
pub mod fork;
pub mod init;
pub mod message;
pub mod parent;

/// Used to describe type of process after fork.
/// Parent and child processes mean the same thing as in a normal fork call
/// InitProcess is specifically used to indicate the process which will run the command of container
pub enum Process<'a> {
    Parent(parent::ParentProcess<'a>),
    Child(child::ChildProcess),
}
/// Maximum event capacity of polling
const MAX_EVENTS: usize = 128;
/// Time to wait when polling for message from child process
const WAIT_FOR_CHILD: Duration = Duration::from_secs(5);
/// Time to wait when polling for mapping ack from parent
const WAIT_FOR_MAPPING: Duration = Duration::from_secs(3);
