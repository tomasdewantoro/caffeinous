//! Sleep prevention for macOS.
//!
//! - [`Assertion`]: IOKit power assertions, the mechanism behind `caffeinate`. Released on drop,
//!   and by the kernel if the process dies.
//! - [`lid`]: keeps the Mac running with the lid closed, without root, with crash recovery and
//!   battery/thermal cut-offs.
//! - [`health`]: thermal pressure and battery state.

#![cfg(target_os = "macos")]

mod assertion;
mod error;
pub mod health;
pub mod lid;

pub use assertion::{Assertion, Kind};
pub use error::Error;

/// Bundle identifier, shared by the app, its defaults domain and the lid holder directory.
pub const BUNDLE_ID: &str = "io.github.tomasdewantoro.caffeinous";
