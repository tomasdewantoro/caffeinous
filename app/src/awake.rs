//! What the app is doing: held assertions, the lid guard, the timer, and preferences.

use std::time::{Duration, SystemTime};

use caffeinous_core::lid::{self, Recovery};
use caffeinous_core::{Assertion, Kind};

use crate::mac::{self, Prefs};
use crate::ui::Command;

const REASON: &str = "Caffeinous";

pub struct Awake {
    prefs: Prefs,
    /// Non-empty while on.
    assertions: Vec<Assertion>,
    until: Option<SystemTime>,
    lid: Option<lid::Guard>,
}

impl Awake {
    pub fn new() -> Self {
        Self {
            prefs: Prefs::load(),
            assertions: Vec::new(),
            until: None,
            lid: None,
        }
    }

    pub fn prefs(&self) -> &Prefs {
        &self.prefs
    }

    pub fn is_on(&self) -> bool {
        !self.assertions.is_empty()
    }

    pub fn until(&self) -> Option<SystemTime> {
        self.until
    }

    pub fn lid_active(&self) -> bool {
        self.lid.is_some()
    }

    /// Repairs a lid setting left behind by a crash, then applies "turn on at launch".
    pub fn startup(&mut self) {
        match lid::recover() {
            Ok(Recovery::Clean | Recovery::InUse | Recovery::Restored) => {}
            Err(e) => mac::alert(
                "Lid-close sleep may still be off",
                &format!(
                    "A previous session kept the Mac awake with the lid closed and could not \
                     be undone: {e}\n\nRestarting the Mac turns lid-close sleep back on."
                ),
            ),
        }
        if self.prefs.on_at_launch {
            self.turn_on(None);
        }
    }

    pub fn shutdown(&mut self) {
        self.turn_off();
    }

    pub fn handle(&mut self, cmd: Command) {
        match cmd {
            Command::Toggle => {
                if self.is_on() {
                    self.turn_off();
                } else {
                    self.turn_on(None);
                }
            }
            Command::For(minutes) => self.turn_on(Some(Duration::from_secs(minutes * 60))),
            Command::Indefinitely => self.turn_on(None),
            Command::Display(on) => {
                self.prefs.keep_display_on = on;
                self.prefs.save();
                if self.is_on() {
                    self.apply_assertions();
                }
            }
            Command::Lid(on) => self.set_lid_pref(on),
            Command::OnAtLaunch(on) => {
                self.prefs.on_at_launch = on;
                self.prefs.save();
            }
            Command::LaunchAtLogin(on) => {
                if let Err(e) = mac::set_launch_at_login(on) {
                    mac::alert("Could not change Launch at Login", &e);
                }
            }
            Command::About => mac::show_about(),
            Command::Website => mac::open_url(env!("CARGO_PKG_REPOSITORY")),
            Command::Quit => {}
        }
    }

    /// Turns off when the timer has run out.
    pub fn expire_if_due(&mut self) {
        if self.until.is_some_and(|t| SystemTime::now() >= t) {
            self.turn_off();
        }
    }

    /// Periodic lid upkeep: stops at the battery or thermal limit, otherwise re-applies the
    /// kernel bit that powerd may have cleared.
    pub fn check_lid(&mut self) {
        let Some(guard) = self.lid.as_ref() else {
            return;
        };
        if let Some(reason) = lid::should_stop() {
            self.release_lid();
            mac::notify_stop(&reason.to_string());
            return;
        }
        if let Err(e) = guard.reassert() {
            self.release_lid();
            mac::alert(
                "Stopped keeping the Mac awake with the lid closed",
                &e.to_string(),
            );
        }
    }

    fn turn_on(&mut self, duration: Option<Duration>) {
        self.until = duration.map(|d| SystemTime::now() + d);
        self.apply_assertions();
        if self.is_on() && self.prefs.lid_closed && self.lid.is_none() {
            self.acquire_lid();
        }
    }

    fn turn_off(&mut self) {
        self.release_lid();
        self.assertions.clear();
        self.until = None;
    }

    fn release_lid(&mut self) {
        if let Some(guard) = self.lid.take()
            && let Err(e) = guard.release()
        {
            mac::alert(
                "Lid-close sleep is still off",
                &format!(
                    "{e}\n\nThe Mac keeps running with the lid closed until Caffeinous turns \
                     on and off again, or until the Mac restarts."
                ),
            );
        }
    }

    fn apply_assertions(&mut self) {
        let mut kinds = vec![Kind::Idle];
        if self.prefs.keep_display_on {
            kinds.push(Kind::Display);
        }
        // Take the new set before releasing the old one so there is no gap.
        match kinds
            .into_iter()
            .map(|k| Assertion::new(k, REASON))
            .collect()
        {
            Ok(set) => self.assertions = set,
            Err(e) => {
                mac::alert("Could not keep your Mac awake", &e.to_string());
            }
        }
    }

    fn set_lid_pref(&mut self, on: bool) {
        if on && !self.prefs.lid_warning_shown {
            let ok = mac::confirm(
                "Keep running with the lid closed?",
                "While Caffeinous is on, your Mac keeps running when you close the lid.\n\n\
                 Don't put it in a bag or sleeve: with the lid closed it cools poorly. \
                 Caffeinous stops if the Mac gets hot or the battery drops to 10%.",
                "Keep Running",
            );
            if !ok {
                return;
            }
            self.prefs.lid_warning_shown = true;
        }
        self.prefs.lid_closed = on;
        self.prefs.save();
        if !self.is_on() {
            return;
        }
        if on {
            self.acquire_lid();
        } else {
            self.release_lid();
        }
    }

    fn acquire_lid(&mut self) {
        if let Some(reason) = lid::should_stop() {
            mac::alert(
                "Not keeping the Mac awake with the lid closed",
                &format!("Caffeinous won't start lid-closed mode because {reason}."),
            );
            return;
        }
        match lid::Guard::acquire() {
            Ok(guard) => self.lid = Some(guard),
            Err(e) => {
                self.prefs.lid_closed = false;
                self.prefs.save();
                mac::alert("Could not keep running with the lid closed", &e.to_string());
            }
        }
    }
}
