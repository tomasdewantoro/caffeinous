//! Thin AppKit / Foundation / ServiceManagement wrappers. All must run on the main thread.

use objc2::MainThreadMarker;
use objc2_app_kit::{NSAlert, NSApplication, NSWorkspace};
use objc2_foundation::{NSString, NSURL, NSUserDefaults};
use objc2_service_management::{SMAppService, SMAppServiceStatus};

fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().expect("AppKit call off the main thread")
}

/// Preferences in the standard user defaults (`defaults read io.github.tomasdewantoro.caffeinous`).
#[derive(Clone, Debug)]
pub struct Prefs {
    pub keep_display_on: bool,
    pub on_at_launch: bool,
    pub lid_closed: bool,
    pub lid_warning_shown: bool,
}

const KEY_DISPLAY: &str = "KeepDisplayOn";
const KEY_ON_AT_LAUNCH: &str = "OnAtLaunch";
const KEY_LID: &str = "LidClosed";
const KEY_LID_WARNED: &str = "LidWarningShown";

impl Prefs {
    pub fn load() -> Self {
        let d = NSUserDefaults::standardUserDefaults();
        let get = |k: &str| d.boolForKey(&NSString::from_str(k));
        Self {
            keep_display_on: get(KEY_DISPLAY),
            on_at_launch: get(KEY_ON_AT_LAUNCH),
            lid_closed: get(KEY_LID),
            lid_warning_shown: get(KEY_LID_WARNED),
        }
    }

    pub fn save(&self) {
        let d = NSUserDefaults::standardUserDefaults();
        let set = |k: &str, v: bool| d.setBool_forKey(v, &NSString::from_str(k));
        set(KEY_DISPLAY, self.keep_display_on);
        set(KEY_ON_AT_LAUNCH, self.on_at_launch);
        set(KEY_LID, self.lid_closed);
        set(KEY_LID_WARNED, self.lid_warning_shown);
    }
}

/// Brings the app forward so a modal or panel is not hidden behind other windows;
/// an accessory (menu-bar) app is never frontmost on its own.
fn activate() {
    #[allow(deprecated)]
    NSApplication::sharedApplication(mtm()).activateIgnoringOtherApps(true);
}

fn build_alert(title: &str, message: &str) -> objc2::rc::Retained<NSAlert> {
    let alert = NSAlert::new(mtm());
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(message));
    alert
}

pub fn alert(title: &str, message: &str) {
    activate();
    build_alert(title, message).runModal();
}

/// Returns true when the user chose `confirm`.
pub fn confirm(title: &str, message: &str, confirm: &str) -> bool {
    activate();
    let alert = build_alert(title, message);
    alert.addButtonWithTitle(&NSString::from_str(confirm));
    alert.addButtonWithTitle(&NSString::from_str("Cancel"));
    alert.runModal() == objc2_app_kit::NSAlertFirstButtonReturn
}

/// Tells the user lid-closed mode stopped at a safety limit. Modal, so it waits on screen for
/// when the lid is opened again; nothing else needs the event loop while it shows.
pub fn notify_stop(reason: &str) {
    alert(
        "Lid-closed mode stopped",
        &format!(
            "Caffeinous stopped keeping the Mac awake with the lid closed because {reason}. \
             The Mac sleeps when the lid closes again. Caffeinous is still on otherwise."
        ),
    );
}

pub fn show_about() {
    activate();
    NSApplication::sharedApplication(mtm()).orderFrontStandardAboutPanel(None);
}

pub fn open_url(url: &str) {
    if let Some(url) = NSURL::URLWithString(&NSString::from_str(url)) {
        NSWorkspace::sharedWorkspace().openURL(&url);
    }
}

pub fn launch_at_login() -> bool {
    // SAFETY: plain property reads on the main app service.
    unsafe { SMAppService::mainAppService().status() == SMAppServiceStatus::Enabled }
}

/// Registers the app as a login item. Works only from an installed `.app` bundle.
pub fn set_launch_at_login(on: bool) -> Result<(), String> {
    // SAFETY: documented ServiceManagement calls on the main app service.
    let result = unsafe {
        let service = SMAppService::mainAppService();
        if on {
            service.registerAndReturnError()
        } else {
            service.unregisterAndReturnError()
        }
    };
    result.map_err(|e| {
        let detail = e.localizedDescription().to_string();
        format!("{detail}\n\nLaunch at Login works only when Caffeinous runs from Applications.")
    })
}
