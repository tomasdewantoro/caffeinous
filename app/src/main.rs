//! Caffeinous menu-bar app.

#![cfg(target_os = "macos")]

mod awake;
mod mac;
mod ui;

use std::time::{Duration, Instant, SystemTime};

use caffeinous_core::lid;
use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
use signal_hook::iterator::Signals;
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
use tray_icon::menu::MenuEvent;

use awake::Awake;
use ui::{Command, Ui};

/// Wakes the event loop from other threads.
#[derive(Debug)]
enum UserEvent {
    Menu(MenuEvent),
    Quit,
}

fn main() {
    let mut event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    // Menu-bar only: no Dock icon, no app menu.
    event_loop.set_activation_policy(ActivationPolicy::Accessory);

    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |e| {
        let _ = proxy.send_event(UserEvent::Menu(e));
    }));

    // SIGTERM/SIGINT/SIGHUP quit cleanly so lid-close sleep is turned back on.
    let proxy = event_loop.create_proxy();
    match Signals::new([SIGTERM, SIGINT, SIGHUP]) {
        Ok(mut signals) => {
            std::thread::spawn(move || {
                if signals.forever().next().is_some() {
                    let _ = proxy.send_event(UserEvent::Quit);
                }
            });
        }
        Err(e) => eprintln!("caffeinous: cannot install signal handler: {e}"),
    }

    let mut app: Option<(Ui, Awake)> = None;
    let mut next_lid_check = Instant::now();

    event_loop.run(move |event, _, control_flow| {
        match event {
            // The status item must be created once the loop is running.
            Event::NewEvents(StartCause::Init) => {
                let mut awake = Awake::new();
                awake.startup();
                let ui = Ui::new(&awake);
                app = Some((ui, awake));
            }
            Event::UserEvent(UserEvent::Menu(e)) => {
                if let Some((ui, awake)) = app.as_mut() {
                    match ui.command(&e.id) {
                        Some(Command::Quit) => *control_flow = ControlFlow::Exit,
                        Some(cmd) => awake.handle(cmd),
                        None => {}
                    }
                }
            }
            Event::UserEvent(UserEvent::Quit) => *control_flow = ControlFlow::Exit,
            Event::LoopDestroyed => {
                if let Some((_, awake)) = app.as_mut() {
                    awake.shutdown();
                }
                return;
            }
            _ => {}
        }

        let Some((ui, awake)) = app.as_mut() else {
            return;
        };
        awake.expire_if_due();
        if Instant::now() >= next_lid_check {
            next_lid_check = Instant::now() + lid::REASSERT_INTERVAL;
            awake.check_lid();
        }
        ui.refresh(awake);
        if *control_flow != ControlFlow::Exit {
            *control_flow = ControlFlow::WaitUntil(next_wakeup(awake, next_lid_check));
        }
    });
}

/// Wakes for the timer deadline, the next minute while a timer runs (so the time left stays
/// current), and the next lid check while lid-closed mode is held.
fn next_wakeup(awake: &Awake, next_lid_check: Instant) -> Instant {
    let mut at = if awake.lid_active() {
        next_lid_check
    } else {
        Instant::now() + Duration::from_secs(3600)
    };
    if let Some(until) = awake.until() {
        let left = until
            .duration_since(SystemTime::now())
            .unwrap_or(Duration::ZERO);
        let to_minute = Duration::from_secs(
            60 - SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs() % 60),
        );
        at = at.min(Instant::now() + left.min(to_minute));
    }
    at
}
