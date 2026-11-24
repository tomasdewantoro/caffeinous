//! Lid-closed (clamshell) wake.
//!
//! No IOKit assertion prevents clamshell sleep on a laptop without an external display. The
//! supported knob is `pmset -a disablesleep 1`, which needs root and persists across reboots.
//! [`Guard`] turns it on and restores it on drop. A marker file records that Caffeinous changed
//! the setting, so a crashed or force-killed process can be repaired by [`recover`].

use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::{Command, Output};

use crate::{BUNDLE_ID, Error};

const PMSET: &str = "/usr/bin/pmset";
const SUDO: &str = "/usr/bin/sudo";
const OSASCRIPT: &str = "/usr/bin/osascript";
const SUDOERS_FILE: &str = "/etc/sudoers.d/caffeinous";

/// How to obtain root for `pmset`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Elevation {
    /// `sudo`, which prompts on the terminal when no rule allows it without a password.
    /// For command-line use.
    Terminal,
    /// `sudo -n` when the passwordless rule is installed, otherwise the standard macOS
    /// administrator dialog. For the menu-bar app, which has no terminal.
    Dialog,
}

/// True when lid-close sleep is currently disabled system-wide.
pub fn is_sleep_disabled() -> Result<bool, Error> {
    let out = run(Command::new(PMSET).arg("-g"))?;
    Ok(parse_sleep_disabled(&String::from_utf8_lossy(&out.stdout)))
}

fn parse_sleep_disabled(pmset_g: &str) -> bool {
    pmset_g
        .lines()
        .filter_map(|l| l.trim().strip_prefix("SleepDisabled"))
        .any(|v| v.trim() == "1")
}

/// Sets `pmset -a disablesleep`, elevating as requested.
pub fn set_sleep_disabled(disabled: bool, elevation: Elevation) -> Result<(), Error> {
    let value = if disabled { "1" } else { "0" };
    if is_root() {
        return run(Command::new(PMSET).args(["-a", "disablesleep", value])).map(drop);
    }
    match elevation {
        Elevation::Terminal => {
            run(Command::new(SUDO).args([PMSET, "-a", "disablesleep", value])).map(drop)
        }
        Elevation::Dialog => {
            let quiet = Command::new(SUDO)
                .args(["-n", PMSET, "-a", "disablesleep", value])
                .output()?;
            if quiet.status.success() {
                return Ok(());
            }
            admin_shell(&format!("{PMSET} -a disablesleep {value}"))
        }
    }
}

/// Holds lid-close sleep disabled. Restores the previous state on drop.
#[derive(Debug)]
pub struct Guard {
    elevation: Elevation,
    changed: bool,
}

impl Guard {
    pub fn acquire(elevation: Elevation) -> Result<Self, Error> {
        if is_sleep_disabled()? {
            // Someone else disabled it; leave it exactly as we found it.
            return Ok(Self {
                elevation,
                changed: false,
            });
        }
        set_sleep_disabled(true, elevation)?;
        // Best effort: without the marker, `recover` cannot tell our change from the user's.
        let _ = write_marker();
        Ok(Self {
            elevation,
            changed: true,
        })
    }

    /// Restores sleep now and reports failure, instead of the silent best effort of `drop`.
    pub fn release(mut self) -> Result<(), Error> {
        self.restore()
    }

    fn restore(&mut self) -> Result<(), Error> {
        if !self.changed {
            return Ok(());
        }
        set_sleep_disabled(false, self.elevation)?;
        self.changed = false;
        remove_marker();
        Ok(())
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
    /// No marker: nothing to repair.
    Clean,
    /// The process that set the marker is still running.
    InUse,
    /// A dead process left sleep disabled; it has been re-enabled.
    Restored,
    /// A dead process left a marker but sleep was already enabled; the marker was removed.
    StaleMarker,
}

/// Re-enables lid-close sleep if a previous Caffeinous process disabled it and died without
/// restoring it (crash, `kill -9`, power loss).
pub fn recover(elevation: Elevation) -> Result<Recovery, Error> {
    let Some(pid) = read_marker() else {
        return Ok(Recovery::Clean);
    };
    if pid != std::process::id() && process_alive(pid) {
        return Ok(Recovery::InUse);
    }
    if is_sleep_disabled()? {
        set_sleep_disabled(false, elevation)?;
        remove_marker();
        Ok(Recovery::Restored)
    } else {
        remove_marker();
        Ok(Recovery::StaleMarker)
    }
}

/// Unconditionally re-enables lid-close sleep and clears the marker.
pub fn force_restore(elevation: Elevation) -> Result<(), Error> {
    if is_sleep_disabled()? {
        set_sleep_disabled(false, elevation)?;
    }
    remove_marker();
    Ok(())
}

/// True when `/etc/sudoers.d/caffeinous` exists, so toggling needs no password.
pub fn passwordless_installed() -> bool {
    fs::metadata(SUDOERS_FILE).is_ok()
}

/// Installs a sudoers rule that lets the current user run exactly
/// `pmset -a disablesleep 0|1` without a password. Asks for an administrator password once.
pub fn install_passwordless() -> Result<(), Error> {
    let user = current_user()?;
    let rule = sudoers_rule(&user);
    let tmp = std::env::temp_dir().join(format!("caffeinous-sudoers-{}", std::process::id()));
    fs::write(&tmp, rule)?;
    let tmp_path = tmp.to_string_lossy();
    // visudo validates before install, so a bad file can never lock the user out of sudo.
    let script = format!(
        "/usr/sbin/visudo -cf '{tmp_path}' && \
         /usr/bin/install -m 0440 -o root -g wheel '{tmp_path}' {SUDOERS_FILE}"
    );
    let result = admin_shell(&script);
    let _ = fs::remove_file(&tmp);
    result
}

/// Removes the sudoers rule. Asks for an administrator password.
pub fn uninstall_passwordless() -> Result<(), Error> {
    admin_shell(&format!("/bin/rm -f {SUDOERS_FILE}"))
}

fn sudoers_rule(user: &str) -> String {
    format!(
        "# Installed by Caffeinous: toggle lid-close sleep without a password.\n\
         {user} ALL=(root) NOPASSWD: {PMSET} -a disablesleep 0, {PMSET} -a disablesleep 1\n"
    )
}

/// Login name, restricted to characters that are safe inside sudoers and a shell string.
fn current_user() -> Result<String, Error> {
    // SAFETY: getpwuid returns a pointer into static storage or null; we copy out immediately.
    let name = unsafe {
        let pw = libc::getpwuid(libc::getuid());
        if pw.is_null() {
            return Err(io::Error::other("cannot resolve the current user").into());
        }
        std::ffi::CStr::from_ptr((*pw).pw_name)
            .to_string_lossy()
            .into_owned()
    };
    let safe = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if safe {
        Ok(name)
    } else {
        Err(io::Error::other(format!("unsupported user name {name:?}")).into())
    }
}

/// Runs `script` as root through the standard macOS administrator dialog.
/// `script` must contain no double quotes or backslashes; callers only pass fixed strings.
fn admin_shell(script: &str) -> Result<(), Error> {
    debug_assert!(!script.contains(['"', '\\']));
    let apple_script = format!(
        "do shell script \"{script}\" with prompt \"Caffeinous needs to change the lid-close \
         sleep setting.\" with administrator privileges"
    );
    let out = Command::new(OSASCRIPT)
        .args(["-e", &apple_script])
        .output()?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    // -128 is AppleScript's "User canceled".
    if stderr.contains("(-128)") {
        return Err(Error::Cancelled);
    }
    Err(Error::Command {
        command: "osascript (administrator)".into(),
        status: out.status,
        stderr,
    })
}

fn run(cmd: &mut Command) -> Result<Output, Error> {
    let out = cmd.output()?;
    if out.status.success() {
        Ok(out)
    } else {
        Err(Error::Command {
            command: format!("{cmd:?}"),
            status: out.status,
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }
}

fn is_root() -> bool {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() == 0 }
}

/// `kill(pid, 0)` probes existence; EPERM means it exists but belongs to another user.
pub fn process_alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    // SAFETY: signal 0 performs only the permission/existence check.
    unsafe { libc::kill(pid, 0) == 0 || *libc::__error() == libc::EPERM }
}

fn marker_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join("Library/Application Support")
            .join(BUNDLE_ID)
            .join("lid-disabled.pid"),
    )
}

fn write_marker() -> io::Result<()> {
    let path = marker_path().ok_or_else(|| io::Error::other("HOME is not set"))?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, std::process::id().to_string())
}

fn read_marker() -> Option<u32> {
    fs::read_to_string(marker_path()?).ok()?.trim().parse().ok()
}

fn remove_marker() {
    if let Some(path) = marker_path() {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sleep_disabled_from_pmset() {
        let on = "System-wide power settings:\n SleepDisabled\t\t1\nCurrently in use:\n sleep 1\n";
        let off = "System-wide power settings:\n SleepDisabled\t\t0\n";
        assert!(parse_sleep_disabled(on));
        assert!(!parse_sleep_disabled(off));
        assert!(!parse_sleep_disabled("Currently in use:\n sleep 1\n"));
    }

    #[test]
    fn sudoers_rule_allows_only_the_two_pmset_commands() {
        let rule = sudoers_rule("tomas");
        let grant = rule.lines().nth(1).unwrap();
        assert_eq!(
            grant,
            "tomas ALL=(root) NOPASSWD: /usr/bin/pmset -a disablesleep 0, \
             /usr/bin/pmset -a disablesleep 1"
        );
    }
}
