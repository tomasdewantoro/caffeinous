<p align="center"><img src="assets/icon-256.png" width="128" alt="Caffeinous icon: a coffee cup under a moon"></p>

# Caffeinous

**Keeps your Mac awake. Even with the lid closed.**

A small menu-bar app for macOS, written in Rust. It does what `caffeinate` does, from a menu
instead of a terminal, and adds the one thing `caffeinate` can't do: keep the Mac running after
you close the lid, with no external display attached.

- **Native and small.** Rust with AppKit menus: no web view, no Electron. About 1 MB.
- **Timers.** On indefinitely, or for 15 minutes to 8 hours; the menu shows the time left.
- **Lid closed.** Keep downloads, builds and SSH sessions alive with the lid shut.
- **Always undone.** Everything is released when Caffeinous turns off or quits, and a lid setting
  left behind by a crash is repaired at the next launch.
- **No network.** Caffeinous makes no network requests. No analytics, no accounts.
- **Free.** MIT OR Apache-2.0.

## Install

Download the dmg from the [latest release](https://github.com/tomasdewantoro/caffeinous/releases),
open it and drag Caffeinous to Applications. It runs on macOS 13 or later, Apple Silicon and
Intel.

Caffeinous lives in the menu bar (there is no Dock icon). Click the cup to open the menu.

## Using it

| Menu item | What it does |
|---|---|
| **Turn On / Turn Off** | Prevents idle sleep until you turn it off |
| **Turn On For ▸** | Same, ending automatically after 15 min to 8 h |
| **Keep Display On** | Also keeps the screen from dimming and sleeping |
| **Keep Running With Lid Closed** | Keeps the Mac running when the lid is shut, while Caffeinous is on |
| **Settings ▸ Turn On at Launch** | Starts in the on state |
| **Settings ▸ Launch at Login** | Opens Caffeinous when you log in |
| **Settings ▸ Don't Ask for Password** | Lets the lid option work without an administrator password each time |

### Lid closed: how it works and what to watch

macOS has no app-level switch for clamshell sleep; the power assertions `caffeinate` uses are
ignored when the lid closes. Caffeinous runs `pmset -a disablesleep 1` while it's on and
`pmset -a disablesleep 0` when it turns off. That system setting needs administrator rights, so
macOS asks for your password, unless you turn on **Don't Ask for Password**, which installs
`/etc/sudoers.d/caffeinous` allowing exactly those two commands and nothing else.

> [!WARNING]
> With the lid closed a MacBook cools worse. Don't put it in a bag or sleeve while Caffeinous
> is on. On battery it keeps draining until you turn Caffeinous off.

If Caffeinous is force-quit while the lid option is active, the setting stays until its next
launch repairs it. To repair it by hand: `sudo pmset -a disablesleep 0`.

The Mac App Store version can't include this option: the App Store sandbox doesn't allow
administrator commands. Everything else is the same.

## Command line

The release also ships `caffeinous`, a drop-in for `caffeinate` with the lid option:

```bash
caffeinous                    # prevent idle sleep until Ctrl-C
caffeinous -d -t 3600         # also keep the display on, stop after an hour
caffeinous -w 1234            # stay awake until process 1234 exits
caffeinous -- cargo build     # stay awake while a command runs; returns its exit code
caffeinous --lid -- ./backup.sh
caffeinous status             # lid setting and current power assertions
caffeinous restore            # re-enable lid-close sleep
caffeinous allow-passwordless # --lid without a password prompt
```

Flags: `-d` display, `-i` idle (default), `-m` disk, `-s` system (AC only), `-u` user active,
`-l`/`--lid`, `-t` seconds, `-w` pid.

## Build from source

Needs Rust 1.88+ and the Xcode command line tools.

```bash
git clone https://github.com/tomasdewantoro/caffeinous
cd caffeinous
scripts/bundle.sh              # dist/Caffeinous.app, dmg and CLI tarball for this Mac
open dist/Caffeinous.app
```

`scripts/bundle.sh --universal` builds for Apple Silicon and Intel;
`scripts/bundle.sh --appstore` builds the sandboxed App Store variant. See
[docs/RELEASING.md](docs/RELEASING.md).

| Path | Contents |
|---|---|
| `crates/core` | Power assertions (IOKit) and the lid guard (`pmset`, crash recovery) |
| `crates/cli` | The `caffeinous` command |
| `app` | The menu-bar app (`tao`, `tray-icon`, `objc2`) |
| `assets` | Icons; `scripts/icons.sh` renders them from `assets/src/*.svg` |

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
