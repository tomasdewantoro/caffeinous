//! IOKit power-management assertions.

use core_foundation::base::TCFType;
use core_foundation::string::{CFString, CFStringRef};

use crate::Error;

type IOPMAssertionID = u32;
type IOReturn = i32;
const IOPM_ASSERTION_LEVEL_ON: u32 = 255;
const IO_RETURN_SUCCESS: IOReturn = 0;

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOPMAssertionCreateWithName(
        assertion_type: CFStringRef,
        level: u32,
        name: CFStringRef,
        id: *mut IOPMAssertionID,
    ) -> IOReturn;
    fn IOPMAssertionRelease(id: IOPMAssertionID) -> IOReturn;
}

/// What an assertion keeps awake. Mirrors the `caffeinate` flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `-d`: the display stays on.
    Display,
    /// `-i`: no idle system sleep. The display may still sleep.
    Idle,
    /// `-m`: the disk does not idle-sleep.
    Disk,
    /// `-s`: no system sleep at all. macOS honours it only on AC power.
    System,
    /// `-u`: declares the user active, waking the display and resetting the idle timer.
    User,
}

impl Kind {
    fn iokit_type(self) -> &'static str {
        match self {
            Self::Display => "PreventUserIdleDisplaySleep",
            Self::Idle => "PreventUserIdleSystemSleep",
            Self::Disk => "PreventDiskIdle",
            Self::System => "PreventSystemSleep",
            Self::User => "UserIsActive",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Display => "display",
            Self::Idle => "idle",
            Self::Disk => "disk",
            Self::System => "system",
            Self::User => "user",
        }
    }
}

/// A held power assertion. Released on drop; the kernel also releases it if the process exits.
#[derive(Debug)]
pub struct Assertion {
    id: IOPMAssertionID,
    kind: Kind,
}

impl Assertion {
    /// `reason` is shown by `pmset -g assertions` and in Activity Monitor's energy tab.
    pub fn new(kind: Kind, reason: &str) -> Result<Self, Error> {
        let ty = CFString::from_static_string(kind.iokit_type());
        let name = CFString::new(reason);
        let mut id: IOPMAssertionID = 0;
        // SAFETY: both CFStrings outlive the call and `id` is a valid out-pointer.
        let code = unsafe {
            IOPMAssertionCreateWithName(
                ty.as_concrete_TypeRef(),
                IOPM_ASSERTION_LEVEL_ON,
                name.as_concrete_TypeRef(),
                &mut id,
            )
        };
        if code == IO_RETURN_SUCCESS {
            Ok(Self { id, kind })
        } else {
            Err(Error::IoKit {
                call: "IOPMAssertionCreateWithName",
                code,
            })
        }
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }
}

impl Drop for Assertion {
    fn drop(&mut self) {
        // SAFETY: `id` came from a successful IOPMAssertionCreateWithName and is released once.
        unsafe { IOPMAssertionRelease(self.id) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn assertions_named(reason: &str) -> usize {
        let out = Command::new("/usr/bin/pmset")
            .args(["-g", "assertions"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).matches(reason).count()
    }

    #[test]
    fn assertion_is_visible_while_held_and_gone_after_drop() {
        let reason = format!("caffeinous-test-{}", std::process::id());
        let held = Assertion::new(Kind::Idle, &reason).unwrap();
        assert_eq!(assertions_named(&reason), 1);
        drop(held);
        assert_eq!(assertions_named(&reason), 0);
    }
}
