# Changelog

## 0.1.0 (unreleased)

### New

- **Menu-bar app.** Turn On / Turn Off, Turn On For 15 minutes to 8 hours or indefinitely, and
  the time left in the menu. The cup in the menu bar fills and steams while on.
- **Keep Display On.** Optional, on top of preventing idle system sleep.
- **Keep Running With Lid Closed.** No administrator password, no changed system settings.
  Stops on its own when the Mac runs hot or the battery reaches 10% while unplugged; a
  setting left behind by a crash or `kill -9` is undone at the next launch.
- **Turn On at Launch** and **Launch at Login**.
- **`caffeinous` command line**, flag-compatible with `caffeinate` (`-d -i -m -s -u -t -w`,
  `-- command`), plus `--lid`, `status` and `restore`.
