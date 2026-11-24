//! `caffeinous`: keep a Mac awake from the command line. Flags match `caffeinate`, plus `--lid`.

use std::process::{Command, ExitCode};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::sleep;
use std::time::{Duration, Instant};

use caffeinous_core::lid::{self, Elevation, Recovery};
use caffeinous_core::{Assertion, Error, Kind};
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
    /// Keep running with the lid closed (asks for sudo; restored on exit)
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
    /// Show the lid-close setting and active power assertions
    Status,
    /// Re-enable lid-close sleep (after a crash or `kill -9`)
    Restore,
    /// Let `--lid` run without a password (installs /etc/sudoers.d/caffeinous)
    AllowPasswordless,
    /// Remove the passwordless rule
    DisallowPasswordless,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.action {
        Some(Action::Status) => status(),
        Some(Action::Restore) => lid::force_restore(Elevation::Terminal)
            .map(|()| eprintln!("caffeinous: lid-close sleep is enabled")),
        Some(Action::AllowPasswordless) => lid::install_passwordless()
            .map(|()| eprintln!("caffeinous: `--lid` no longer asks for a password")),
        Some(Action::DisallowPasswordless) => lid::uninstall_passwordless()
            .map(|()| eprintln!("caffeinous: passwordless rule removed")),
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
    // A dead earlier run may have left sleep disabled; repair it first so the new guard
    // does not mistake that for a setting the user chose and skip restoring it.
    if lid::recover(Elevation::Terminal)? == Recovery::Restored {
        eprintln!("caffeinous: re-enabled lid-close sleep left disabled by an earlier run");
    }
    lid::Guard::acquire(Elevation::Terminal)
}

fn status() -> Result<(), Error> {
    let disabled = lid::is_sleep_disabled()?;
    println!(
        "lid-close sleep: {}",
        if disabled {
            "disabled (stays on with the lid closed)"
        } else {
            "enabled"
        }
    );
    println!(
        "passwordless --lid: {}",
        if lid::passwordless_installed() {
            "allowed"
        } else {
            "not set up"
        }
    );
    let out = Command::new("/usr/bin/pmset")
        .args(["-g", "assertions"])
        .output()?;
    print!("{}", String::from_utf8_lossy(&out.stdout));
    Ok(())
}

fn fail(e: &Error) -> ExitCode {
    eprintln!("caffeinous: {e}");
    ExitCode::FAILURE
}
