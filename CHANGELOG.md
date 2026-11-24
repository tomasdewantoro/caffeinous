# Changelog

## 0.1.0 (unreleased)

### New

- **Menu-bar app.** Turn On / Turn Off, Turn On For 15 minutes to 8 hours or indefinitely, and
  the time left in the menu. The cup in the menu bar fills and steams while on.
- **Keep Display On.** Optional, on top of preventing idle system sleep.
- **Keep Running With Lid Closed** (direct build only). Runs `pmset -a disablesleep 1` while on
  and restores it when Caffeinous turns off or quits. A setting left behind by a crash or
  `kill -9` is repaired at the next launch. **Don't Ask for Password** installs a sudoers rule
  that allows only `pmset -a disablesleep 0|1`.
- **Turn On at Launch** and **Launch at Login**.
- **`caffeinous` command line**, flag-compatible with `caffeinate` (`-d -i -m -s -u -t -w`,
  `-- command`), plus `--lid`, `status`, `restore` and `allow-passwordless`.
