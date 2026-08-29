//! Independent, unofficial Mosh client implementation.
//!
//! The crate exposes validated bootstrap values and a small asynchronous
//! Session API. SSH authentication, host trust, and `mosh-server` startup
//! remain the caller's responsibility.

#![forbid(unsafe_code)]
#![deny(rustdoc::broken_intra_doc_links)]

mod bootstrap;
#[allow(dead_code)]
mod crypto;
mod error;
#[allow(dead_code)]
mod fragment;
#[allow(dead_code)]
mod instruction;
mod limits;
#[allow(dead_code)]
mod packet;
#[allow(dead_code)]
mod prediction;
mod session;
#[allow(dead_code)]
mod synchronization;
#[allow(dead_code)]
mod terminal;
#[cfg(all(test, target_os = "linux"))]
mod test_support;
#[allow(dead_code)]
mod timing;

pub use bootstrap::Bootstrap;
pub use error::BootstrapError;
pub use session::{
    Session, SessionCommandError, SessionError, SessionExit, SessionState, SessionTask,
};
