//! What the app is doing: held assertions, the optional lid guard, the timer, and preferences.

use std::time::{Duration, SystemTime};

use caffeinous_core::{Assertion, Kind};

use crate::mac::{self, Prefs};
use crate::ui::Command;

const REASON: &str = "Caffeinous";

pub struct Awake {
    prefs: Prefs,
    /// Non-empty while on.
    assertions: Vec<Assertion>,
    until: Option<SystemTime>,
    #[cfg(feature = "lid")]
    lid: Option<caffeinous_core::lid::Guard>,
}

impl Awake {
    pub fn new() -> Self {
        Self {
            prefs: Prefs::load(),
            assertions: Vec::new(),
            until: None,
            #[cfg(feature = "lid")]
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

    #[cfg(feature = "lid")]
    pub fn lid_active(&self) -> bool {
        self.lid.is_some()
    }

    /// Repairs a lid setting left behind by a crash, then applies "turn on at launch".
    pub fn startup(&mut self) {
        #[cfg(feature = "lid")]
        {
            use caffeinous_core::lid::{self, Elevation};
            if let Err(e) = lid::recover(Elevation::Dialog) {
                mac::alert(
                    "Lid-close sleep may still be disabled",
                    &format!(
                        "A previous session turned it off and could not be undone: {e}\n\n\
                         Run `sudo pmset -a disablesleep 0` in Terminal to restore it."
                    ),
                );
            }
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
            #[cfg(feature = "lid")]
            Command::Lid(on) => self.set_lid_pref(on),
            #[cfg(feature = "lid")]
            Command::Passwordless(on) => set_passwordless(on),
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

    fn turn_on(&mut self, duration: Option<Duration>) {
        self.until = duration.map(|d| SystemTime::now() + d);
        self.apply_assertions();
        #[cfg(feature = "lid")]
        if self.is_on() && self.prefs.lid_closed && self.lid.is_none() {
            self.acquire_lid();
        }
    }

    fn turn_off(&mut self) {
        #[cfg(feature = "lid")]
        if let Some(guard) = self.lid.take()
            && let Err(e) = guard.release()
        {
            mac::alert(
                "Lid-close sleep is still disabled",
                &format!(
                    "{e}\n\nYour Mac will keep running with the lid closed until this is \
                     undone. Turn Caffeinous on and off again, or run \
                     `sudo pmset -a disablesleep 0` in Terminal."
                ),
            );
        }
        self.assertions.clear();
        self.until = None;
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

    #[cfg(feature = "lid")]
    fn set_lid_pref(&mut self, on: bool) {
        if on && !self.prefs.lid_warning_shown {
            let ok = mac::confirm(
                "Keep running with the lid closed?",
                "Your Mac will stay on when you close the lid, until Caffeinous turns off.\n\n\
                 Don't put it in a bag or sleeve while it's on: with the lid closed it cools \
                 poorly and can get hot. On battery it keeps draining.\n\n\
                 Changing this setting needs an administrator password.",
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
        } else if let Some(guard) = self.lid.take()
            && let Err(e) = guard.release()
        {
            mac::alert("Could not re-enable lid-close sleep", &e.to_string());
        }
    }

    #[cfg(feature = "lid")]
    fn acquire_lid(&mut self) {
        use caffeinous_core::{Error, lid};
        match lid::Guard::acquire(lid::Elevation::Dialog) {
            Ok(guard) => self.lid = Some(guard),
            Err(Error::Cancelled) => {
                self.prefs.lid_closed = false;
                self.prefs.save();
            }
            Err(e) => {
                self.prefs.lid_closed = false;
                self.prefs.save();
                mac::alert("Could not keep running with the lid closed", &e.to_string());
            }
        }
    }
}

#[cfg(feature = "lid")]
fn set_passwordless(on: bool) {
    use caffeinous_core::{Error, lid};
    let result = if on {
        lid::install_passwordless()
    } else {
        lid::uninstall_passwordless()
    };
    match result {
        Ok(()) | Err(Error::Cancelled) => {}
        Err(e) => mac::alert("Could not change the password setting", &e.to_string()),
    }
}
