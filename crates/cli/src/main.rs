//! `caffeinous`: keep a Mac awake from the command line. Flags match `caffeinate`, plus `--lid`.

use std::io::{self, Write};
use std::process::{Command, ExitCode};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::sleep;
use std::time::{Duration, Instant};

use caffeinous_core::lid::{self, Recovery};
use caffeinous_core::{Assertion, Error, Kind, health};
use clap::{Parser, Subcommand};
use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};

const POLL: Duration = Duration::from_millis(250);

#[derive(Parser)]
#[command(
    name = "caffeinous",
    version,
    about = "Keep your Mac awake, like caffeinate, optionally with the lid closed",
    after_help = "With no mode flag, -i is assumed. With no COMMAND, -w or -t, runs until Ctrl-C."
)]
struct Cli {
    /// Keep the display on
    #[arg(short = 'd', long)]
    display: bool,
    /// Prevent idle system sleep
    #[arg(short = 'i', long)]
    idle: bool,
    /// Prevent idle disk sleep
    #[arg(short = 'm', long)]
    disk: bool,
    /// Prevent system sleep (macOS honours it only on AC power)
    #[arg(short = 's', long)]
    system: bool,
    /// Declare the user active: wakes the display and resets the idle timer
    #[arg(short = 'u', long)]
    user: bool,
    /// Keep running with the lid closed (no password; stops at low battery or high heat)
    #[arg(short = 'l', long)]
    lid: bool,
    /// Stop after this many seconds
    #[arg(short = 't', long, value_name = "SECONDS")]
    timeout: Option<u64>,
    /// Stop when this process exits
    #[arg(short = 'w', long, value_name = "PID")]
    wait: Option<u32>,
    #[command(subcommand)]
    action: Option<Action>,
    /// Command to run; stays awake until it exits and returns its exit code
    #[arg(last = true, value_name = "COMMAND")]
    command: Vec<String>,
}

#[derive(Subcommand)]
enum Action {
    /// Show lid-closed wake, battery, thermal state and active power assertions
    Status,
    /// Re-enable lid-close sleep (after a crash or `kill -9`)
    Restore,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.action {
        Some(Action::Status) => status(),
        Some(Action::Restore) => {
            lid::force_restore().map(|()| eprintln!("caffeinous: lid-close sleep is enabled"))
        }
        None => return run(cli),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(&e),
    }
}

fn run(cli: Cli) -> ExitCode {
    let mut kinds: Vec<Kind> = [
        (cli.display, Kind::Display),
        (cli.idle, Kind::Idle),
        (cli.disk, Kind::Disk),
        (cli.system, Kind::System),
        (cli.user, Kind::User),
    ]
    .into_iter()
    .filter_map(|(on, kind)| on.then_some(kind))
    .collect();
    if kinds.is_empty() {
        kinds.push(Kind::Idle);
    }

    let stop = Arc::new(AtomicBool::new(false));
    for sig in [SIGINT, SIGTERM, SIGHUP] {
        if let Err(e) = signal_hook::flag::register(sig, Arc::clone(&stop)) {
            eprintln!("caffeinous: cannot install signal handler: {e}");
            return ExitCode::FAILURE;
        }
    }

    // Held for the whole run; dropped (released) on return.
    let reason = format!("caffeinous (pid {})", std::process::id());
    let assertions: Vec<Assertion> =
        match kinds.iter().map(|&k| Assertion::new(k, &reason)).collect() {
            Ok(a) => a,
            Err(e) => return fail(&e),
        };
    let lid_guard = if cli.lid {
        match acquire_lid() {
            Ok(g) => Some(g),
            Err(e) => return fail(&e),
        }
    } else {
        None
    };

    let mut child = if cli.command.is_empty() {
        None
    } else {
        match Command::new(&cli.command[0])
            .args(&cli.command[1..])
            .spawn()
        {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("caffeinous: cannot run {}: {e}", cli.command[0]);
                return ExitCode::FAILURE;
            }
        }
    };

    let labels: Vec<_> = assertions.iter().map(|a| a.kind().label()).collect();
    eprintln!(
        "caffeinous: awake [{}{}]; Ctrl-C to stop",
        labels.join(", "),
        if lid_guard.is_some() {
            ", lid closed"
        } else {
            ""
        }
    );

    let deadline = cli.timeout.map(|s| Instant::now() + Duration::from_secs(s));
    let mut code = ExitCode::SUCCESS;
    let mut next_lid_check = Instant::now() + lid::REASSERT_INTERVAL;
    while !stop.load(Ordering::Relaxed) {
        if deadline.is_some_and(|d| Instant::now() >= d) {
            break;
        }
        if cli.wait.is_some_and(|pid| !lid::process_alive(pid)) {
            break;
        }
        if let Some(c) = child.as_mut() {
            match c.try_wait() {
                Ok(Some(status)) => {
                    code = ExitCode::from(status.code().unwrap_or(1).clamp(0, 255) as u8);
                    break;
                }
                Ok(None) => {}
                Err(e) => {
                    eprintln!("caffeinous: waiting for the command failed: {e}");
                    code = ExitCode::FAILURE;
                    break;
                }
            }
        }
        if let Some(guard) = lid_guard.as_ref()
            && Instant::now() >= next_lid_check
        {
            next_lid_check = Instant::now() + lid::REASSERT_INTERVAL;
            if let Some(reason) = lid::should_stop() {
                eprintln!("caffeinous: stopping, {reason}");
                break;
            }
            if let Err(e) = guard.reassert() {
                eprintln!("caffeinous: could not keep lid-closed wake: {e}");
                code = ExitCode::FAILURE;
                break;
            }
        }
        sleep(POLL);
    }

    if let Some(guard) = lid_guard
        && let Err(e) = guard.release()
    {
        eprintln!("caffeinous: could not re-enable lid-close sleep: {e}");
        eprintln!("caffeinous: run `caffeinous restore`");
        code = ExitCode::FAILURE;
    }
    drop(assertions);
    eprintln!("caffeinous: released");
    code
}

fn acquire_lid() -> Result<lid::Guard, Error> {
    if lid::recover()? == Recovery::Restored {
        eprintln!("caffeinous: re-enabled lid-close sleep left on by a crashed run");
    }
    if let Some(reason) = lid::should_stop() {
        return Err(std::io::Error::other(format!("not keeping the lid awake: {reason}")).into());
    }
    lid::Guard::acquire()
}

fn status() -> Result<(), Error> {
    let holders = lid::holders();
    let lid_line = if holders.is_empty() {
        "off".to_owned()
    } else {
        let pids: Vec<_> = holders.iter().map(u32::to_string).collect();
        format!("on (pid {})", pids.join(", "))
    };
    let battery = match health::battery() {
        Some(b) => format!(
            "{}%, {}",
            b.percent,
            if b.discharging { "on battery" } else { "on AC" }
        ),
        None => "none".to_owned(),
    };
    let assertions = Command::new("/usr/bin/pmset")
        .args(["-g", "assertions"])
        .output()?;
    let mut out = io::stdout().lock();
    let written = writeln!(out, "lid-closed wake: {lid_line}")
        .and_then(|()| writeln!(out, "battery: {battery}"))
        .and_then(|()| writeln!(out, "thermal state: {:?}", health::thermal_state()))
        .and_then(|()| out.write_all(&assertions.stdout));
    match written {
        // `caffeinous status | head` closes the pipe early; that is not an error.
        Err(e) if e.kind() != io::ErrorKind::BrokenPipe => Err(e.into()),
        _ => Ok(()),
    }
}

fn fail(e: &Error) -> ExitCode {
    eprintln!("caffeinous: {e}");
    ExitCode::FAILURE
}
