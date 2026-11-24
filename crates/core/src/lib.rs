//! Sleep prevention for macOS.
//!
//! - [`Assertion`]: IOKit power assertions, the mechanism behind `caffeinate`. Released on drop,
//!   and by the kernel if the process dies.
//! - [`lid`]: keeps the Mac running with the lid closed by toggling `pmset disablesleep`, which
//!   needs root and is a persistent system setting, so it is guarded and crash-recovered.

#![cfg(target_os = "macos")]

mod assertion;
mod error;
pub mod lid;

pub use assertion::{Assertion, Kind};
pub use error::Error;

/// Bundle identifier, shared by the app, its defaults domain and the recovery marker directory.
pub const BUNDLE_ID: &str = "io.github.tomasdewantoro.caffeinous";
