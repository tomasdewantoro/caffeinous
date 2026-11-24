//! Lid-closed (clamshell) wake, without root.
//!
//! `IOPMrootDomain`'s user client takes selector `kPMSetClamshellSleepState` (12, declared in the
//! SDK's `IOPMLibDefs.h`), which sets the powerd bit of the kernel's clamshell-sleep-disable mask.
//! XNU checks no privilege or entitlement for it, so a normal user can call it, also inside the
//! App Sandbox. It is not documented API, so callers must treat failure as a real possibility.
//!
//! The bit is kernel state, not tied to the caller:
//! - it outlives the process, so a crash leaves the Mac awake with the lid closed until someone
//!   clears it or the Mac restarts. Each holder records its pid in a holder directory, and
//!   [`recover`] clears the bit for holders that died;
//! - powerd writes the same bit (around sleep and wake), so holders re-apply it every
//!   [`REASSERT_INTERVAL`] with [`Guard::reassert`];
//! - it cannot be read back (the registry's `AppleClamshellCausesSleep` only updates on lid
//!   events), so the holder directory is the only record of who wants it set.

use std::ffi::{c_char, c_void};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::{BUNDLE_ID, Error, health};

/// How often a holder re-applies the bit while it holds it.
pub const REASSERT_INTERVAL: Duration = Duration::from_secs(15);

/// Lid-closed wake stops at or below this charge while on battery.
pub const BATTERY_FLOOR_PERCENT: u8 = 10;

const K_PM_SET_CLAMSHELL_SLEEP_STATE: u32 = 12;

type MachPort = u32;
type KernReturn = i32;

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOServiceMatching(name: *const c_char) -> *mut c_void;
    fn IOServiceGetMatchingService(main_port: MachPort, matching: *mut c_void) -> MachPort;
    fn IOServiceOpen(
        service: MachPort,
        task: MachPort,
        kind: u32,
        connect: *mut MachPort,
    ) -> KernReturn;
    fn IOServiceClose(connect: MachPort) -> KernReturn;
    fn IOObjectRelease(object: MachPort) -> KernReturn;
    fn IOConnectCallScalarMethod(
        connection: MachPort,
        selector: u32,
        input: *const u64,
        input_count: u32,
        output: *mut u64,
        output_count: *mut u32,
    ) -> KernReturn;
}

unsafe extern "C" {
    /// The value behind the `mach_task_self()` macro.
    static mach_task_self_: MachPort;
}

/// Sets or clears lid-close sleep prevention in the kernel.
fn set_clamshell_sleep_disabled(disabled: bool) -> Result<(), Error> {
    // SAFETY: plain IOKit calls; `matching` is consumed by IOServiceGetMatchingService, and the
    // service and connection are released on every path.
    unsafe {
        let matching = IOServiceMatching(c"IOPMrootDomain".as_ptr());
        if matching.is_null() {
            return Err(Error::IoKit {
                call: "IOServiceMatching",
                code: 0,
            });
        }
        let service = IOServiceGetMatchingService(0, matching);
        if service == 0 {
            return Err(Error::IoKit {
                call: "IOServiceGetMatchingService",
                code: 0,
            });
        }
        let mut connect: MachPort = 0;
        let code = IOServiceOpen(service, mach_task_self_, 0, &mut connect);
        IOObjectRelease(service);
        if code != 0 {
            return Err(Error::IoKit {
                call: "IOServiceOpen(IOPMrootDomain)",
                code,
            });
        }
        let input = [u64::from(disabled)];
        let code = IOConnectCallScalarMethod(
            connect,
            K_PM_SET_CLAMSHELL_SLEEP_STATE,
            input.as_ptr(),
            1,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        IOServiceClose(connect);
        if code != 0 {
            return Err(Error::IoKit {
                call: "kPMSetClamshellSleepState",
                code,
            });
        }
    }
    Ok(())
}

/// Keeps the Mac running with the lid closed while held. Released on drop.
#[derive(Debug)]
pub struct Guard {
    dir: PathBuf,
    held: bool,
}

impl Guard {
    pub fn acquire() -> Result<Self, Error> {
        let dir = holder_dir().ok_or_else(|| io::Error::other("HOME is not set"))?;
        fs::create_dir_all(&dir)?;
        // Record first, so a crash right after the kernel call is still recoverable.
        fs::write(dir.join(std::process::id().to_string()), b"")?;
        if let Err(e) = set_clamshell_sleep_disabled(true) {
            let _ = fs::remove_file(dir.join(std::process::id().to_string()));
            return Err(e);
        }
        Ok(Self { dir, held: true })
    }

    /// Re-applies the bit; powerd may have cleared it. Call every [`REASSERT_INTERVAL`].
    pub fn reassert(&self) -> Result<(), Error> {
        set_clamshell_sleep_disabled(true)
    }

    /// Releases now and reports failure, instead of the silent best effort of `drop`.
    pub fn release(mut self) -> Result<(), Error> {
        self.restore()
    }

    fn restore(&mut self) -> Result<(), Error> {
        if !self.held {
            return Ok(());
        }
        self.held = false;
        let _ = fs::remove_file(self.dir.join(std::process::id().to_string()));
        // Another live Caffeinous (the app and the CLI can both hold it) still wants it set.
        if !live_holders(&self.dir).is_empty() {
            return Ok(());
        }
        set_clamshell_sleep_disabled(false)
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        if let Err(e) = self.restore() {
            eprintln!("caffeinous: could not re-enable lid-close sleep: {e}");
        }
    }
}

/// What [`recover`] found.
#[derive(Debug, PartialEq, Eq)]
pub enum Recovery {
    /// No dead holders.
    Clean,
    /// Dead holders were removed; another live holder still keeps the lid awake.
    InUse,
    /// Dead holders were removed and lid-close sleep was re-enabled.
    Restored,
}

/// Re-enables lid-close sleep if a Caffeinous process that held it died without releasing it
/// (crash, `kill -9`). A restart also clears it.
pub fn recover() -> Result<Recovery, Error> {
    let Some(dir) = holder_dir() else {
        return Ok(Recovery::Clean);
    };
    if reap_dead_holders(&dir) == 0 {
        return Ok(Recovery::Clean);
    }
    if !live_holders(&dir).is_empty() {
        return Ok(Recovery::InUse);
    }
    set_clamshell_sleep_disabled(false)?;
    Ok(Recovery::Restored)
}

/// Unconditionally re-enables lid-close sleep and forgets all holders.
pub fn force_restore() -> Result<(), Error> {
    if let Some(dir) = holder_dir() {
        let _ = fs::remove_dir_all(dir);
    }
    set_clamshell_sleep_disabled(false)
}

/// Pids of running Caffeinous processes that hold lid-closed wake.
pub fn holders() -> Vec<u32> {
    holder_dir().map_or_else(Vec::new, |dir| live_holders(&dir))
}

/// Why lid-closed wake should stop now, if it should.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    LowBattery(u8),
    Hot,
}

impl std::fmt::Display for StopReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LowBattery(p) => write!(f, "battery at {p}%"),
            Self::Hot => f.write_str("the Mac is running hot"),
        }
    }
}

/// Checks the safety limits for running with the lid closed: a serious or critical thermal
/// state, or a battery at or below [`BATTERY_FLOOR_PERCENT`] while unplugged.
pub fn should_stop() -> Option<StopReason> {
    if health::thermal_state() >= health::Thermal::Serious {
        return Some(StopReason::Hot);
    }
    match health::battery() {
        Some(b) if b.discharging && b.percent <= BATTERY_FLOOR_PERCENT => {
            Some(StopReason::LowBattery(b.percent))
        }
        _ => None,
    }
}

fn holder_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join("Library/Application Support")
            .join(BUNDLE_ID)
            .join("lid-holders"),
    )
}

fn holder_pids(dir: &Path) -> Vec<u32> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok()?.file_name().to_str()?.parse().ok())
        .collect()
}

fn live_holders(dir: &Path) -> Vec<u32> {
    holder_pids(dir)
        .into_iter()
        .filter(|&pid| process_alive(pid))
        .collect()
}

/// Removes holder files of processes that no longer run. Returns how many were removed.
fn reap_dead_holders(dir: &Path) -> usize {
    holder_pids(dir)
        .into_iter()
        .filter(|&pid| !process_alive(pid))
        .filter(|pid| fs::remove_file(dir.join(pid.to_string())).is_ok())
        .count()
}

/// `kill(pid, 0)` probes existence; EPERM means it exists but belongs to another user.
pub fn process_alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    if pid <= 0 {
        return false;
    }
    // SAFETY: signal 0 performs only the permission/existence check.
    unsafe { libc::kill(pid, 0) == 0 || *libc::__error() == libc::EPERM }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dead_pid() -> u32 {
        let mut child = std::process::Command::new("/usr/bin/true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        pid
    }

    #[test]
    fn dead_holders_are_reaped_and_live_ones_kept() {
        let dir = std::env::temp_dir().join(format!("caffeinous-holders-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let me = std::process::id();
        let dead = dead_pid();
        fs::write(dir.join(me.to_string()), b"").unwrap();
        fs::write(dir.join(dead.to_string()), b"").unwrap();
        fs::write(dir.join("not-a-pid"), b"").unwrap();

        assert_eq!(live_holders(&dir), vec![me]);
        assert_eq!(reap_dead_holders(&dir), 1);
        assert!(!dir.join(dead.to_string()).exists());
        assert!(dir.join(me.to_string()).exists());
        assert_eq!(reap_dead_holders(&dir), 0);

        fs::remove_dir_all(&dir).unwrap();
    }
}
