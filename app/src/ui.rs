//! The status item and its menu. Pure presentation: state lives in [`Awake`].

use std::time::SystemTime;

use tray_icon::menu::{CheckMenuItem, Menu, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::awake::Awake;
use crate::mac;

/// Menu actions, decoded from item ids.
#[derive(Clone, Copy, Debug)]
pub enum Command {
    Toggle,
    For(u64),
    Indefinitely,
    Display(bool),
    #[cfg(feature = "lid")]
    Lid(bool),
    #[cfg(feature = "lid")]
    Passwordless(bool),
    OnAtLaunch(bool),
    LaunchAtLogin(bool),
    About,
    Website,
    Quit,
}

/// Durations offered under "Turn On For", in minutes.
const DURATIONS: [(u64, &str); 6] = [
    (15, "15 Minutes"),
    (30, "30 Minutes"),
    (60, "1 Hour"),
    (120, "2 Hours"),
    (300, "5 Hours"),
    (480, "8 Hours"),
];

const ICON_ON: &[u8] = include_bytes!("../../assets/tray-on.png");
const ICON_OFF: &[u8] = include_bytes!("../../assets/tray-off.png");

pub struct Ui {
    tray: TrayIcon,
    icon_on: Icon,
    icon_off: Icon,
    status: MenuItem,
    toggle: MenuItem,
    durations: Vec<(MenuId, u64)>,
    indefinitely: MenuItem,
    display: CheckMenuItem,
    #[cfg(feature = "lid")]
    lid: CheckMenuItem,
    #[cfg(feature = "lid")]
    passwordless: CheckMenuItem,
    on_at_launch: CheckMenuItem,
    launch_at_login: CheckMenuItem,
    about: MenuItem,
    website: MenuItem,
    quit: MenuItem,
    /// Last rendered state, to skip redundant AppKit updates.
    shown: Option<Shown>,
}

#[derive(PartialEq)]
struct Shown {
    on: bool,
    status: String,
}

impl Ui {
    pub fn new(awake: &Awake) -> Self {
        let status = MenuItem::new("Caffeinous: Off", false, None);
        let toggle = MenuItem::new("Turn On", true, None);

        let for_menu = Submenu::new("Turn On For", true);
        let mut durations = Vec::with_capacity(DURATIONS.len());
        for (minutes, label) in DURATIONS {
            let item = MenuItem::new(label, true, None);
            durations.push((item.id().clone(), minutes));
            for_menu.append(&item).expect("append menu item");
        }
        let indefinitely = MenuItem::new("Indefinitely", true, None);
        for_menu
            .append_items(&[&PredefinedMenuItem::separator(), &indefinitely])
            .expect("append menu items");

        let prefs = awake.prefs();
        let display = CheckMenuItem::new("Keep Display On", true, prefs.keep_display_on, None);
        #[cfg(feature = "lid")]
        let lid = CheckMenuItem::new("Keep Running With Lid Closed", true, prefs.lid_closed, None);
        #[cfg(feature = "lid")]
        let passwordless = CheckMenuItem::new(
            "Don't Ask for Password",
            true,
            caffeinous_core::lid::passwordless_installed(),
            None,
        );
        let on_at_launch = CheckMenuItem::new("Turn On at Launch", true, prefs.on_at_launch, None);
        let launch_at_login =
            CheckMenuItem::new("Launch at Login", true, mac::launch_at_login(), None);

        let settings = Submenu::new("Settings", true);
        settings
            .append_items(&[&on_at_launch, &launch_at_login])
            .expect("append menu items");
        #[cfg(feature = "lid")]
        settings
            .append_items(&[&PredefinedMenuItem::separator(), &passwordless])
            .expect("append menu items");

        let about = MenuItem::new("About Caffeinous", true, None);
        let website = MenuItem::new("Caffeinous on GitHub", true, None);
        let quit = MenuItem::new("Quit Caffeinous", true, None);

        let menu = Menu::new();
        menu.append_items(&[
            &status,
            &PredefinedMenuItem::separator(),
            &toggle,
            &for_menu,
            &PredefinedMenuItem::separator(),
            &display,
        ])
        .expect("append menu items");
        #[cfg(feature = "lid")]
        menu.append(&lid).expect("append menu item");
        menu.append_items(&[
            &PredefinedMenuItem::separator(),
            &settings,
            &about,
            &website,
            &PredefinedMenuItem::separator(),
            &quit,
        ])
        .expect("append menu items");

        let icon_on = load_icon(ICON_ON);
        let icon_off = load_icon(ICON_OFF);
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_icon_templated(icon_off.clone())
            .with_menu_on_left_click(true)
            .with_tooltip("Caffeinous")
            .build()
            .expect("create status item");

        Self {
            tray,
            icon_on,
            icon_off,
            status,
            toggle,
            durations,
            indefinitely,
            display,
            #[cfg(feature = "lid")]
            lid,
            #[cfg(feature = "lid")]
            passwordless,
            on_at_launch,
            launch_at_login,
            about,
            website,
            quit,
            shown: None,
        }
    }

    /// Maps a clicked item to a command, reading the new checkbox state where relevant.
    pub fn command(&self, id: &MenuId) -> Option<Command> {
        if id == self.toggle.id() {
            return Some(Command::Toggle);
        }
        if let Some(&(_, minutes)) = self.durations.iter().find(|(d, _)| d == id) {
            return Some(Command::For(minutes));
        }
        if id == self.indefinitely.id() {
            return Some(Command::Indefinitely);
        }
        if id == self.display.id() {
            return Some(Command::Display(self.display.is_checked()));
        }
        #[cfg(feature = "lid")]
        if id == self.lid.id() {
            return Some(Command::Lid(self.lid.is_checked()));
        }
        #[cfg(feature = "lid")]
        if id == self.passwordless.id() {
            return Some(Command::Passwordless(self.passwordless.is_checked()));
        }
        if id == self.on_at_launch.id() {
            return Some(Command::OnAtLaunch(self.on_at_launch.is_checked()));
        }
        if id == self.launch_at_login.id() {
            return Some(Command::LaunchAtLogin(self.launch_at_login.is_checked()));
        }
        if id == self.about.id() {
            return Some(Command::About);
        }
        if id == self.website.id() {
            return Some(Command::Website);
        }
        if id == self.quit.id() {
            return Some(Command::Quit);
        }
        None
    }

    /// Syncs the menu with the real state. Checkboxes are reset from the source of truth so a
    /// cancelled password prompt or a failed login-item change never leaves a stale tick.
    pub fn refresh(&mut self, awake: &Awake) {
        let prefs = awake.prefs();
        self.display.set_checked(prefs.keep_display_on);
        self.on_at_launch.set_checked(prefs.on_at_launch);
        self.launch_at_login.set_checked(mac::launch_at_login());
        #[cfg(feature = "lid")]
        {
            self.lid.set_checked(prefs.lid_closed);
            self.passwordless
                .set_checked(caffeinous_core::lid::passwordless_installed());
        }

        let shown = Shown {
            on: awake.is_on(),
            status: status_text(awake),
        };
        if self.shown.as_ref() == Some(&shown) {
            return;
        }
        self.status.set_text(&shown.status);
        self.toggle
            .set_text(if shown.on { "Turn Off" } else { "Turn On" });
        let icon = if shown.on {
            &self.icon_on
        } else {
            &self.icon_off
        };
        let _ = self.tray.set_icon_templated(Some(icon.clone()));
        let _ = self.tray.set_tooltip(Some(&shown.status));
        self.shown = Some(shown);
    }
}

fn status_text(awake: &Awake) -> String {
    if !awake.is_on() {
        return "Caffeinous: Off".into();
    }
    #[cfg(feature = "lid")]
    let lid = if awake.lid_active() {
        ", lid closed OK"
    } else {
        ""
    };
    #[cfg(not(feature = "lid"))]
    let lid = "";
    match awake.until() {
        None => format!("Caffeinous: On{lid}"),
        Some(until) => {
            let secs = until
                .duration_since(SystemTime::now())
                .map_or(0, |d| d.as_secs());
            format!("Caffeinous: On, {} left{lid}", format_remaining(secs))
        }
    }
}

/// Rounds up to the minute so "0 min left" never shows while still on.
fn format_remaining(secs: u64) -> String {
    let minutes = secs.div_ceil(60).max(1);
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

fn load_icon(png_bytes: &[u8]) -> Icon {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().expect("embedded PNG header");
    let mut buf = vec![0; reader.output_buffer_size().expect("embedded PNG size")];
    let info = reader.next_frame(&mut buf).expect("embedded PNG data");
    buf.truncate(info.buffer_size());
    Icon::from_rgba(buf, info.width, info.height).expect("embedded PNG is RGBA")
}

#[cfg(test)]
mod tests {
    use super::format_remaining;

    #[test]
    fn remaining_time_rounds_up_to_the_minute() {
        assert_eq!(format_remaining(1), "1 min");
        assert_eq!(format_remaining(60), "1 min");
        assert_eq!(format_remaining(61), "2 min");
        assert_eq!(format_remaining(3600), "1 h");
        assert_eq!(format_remaining(3601), "1 h 1 min");
        assert_eq!(format_remaining(5 * 3600 - 30), "5 h");
    }
}
