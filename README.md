<p align="center">
  <img src="assets/icon.png" alt="osu-twitchbot icon" width="128">
</p>

# osu! twitchbot

A lightweight desktop app that reads beatmap information from osu! and can respond to Twitch chat commands. Supports both Stable and Lazer.

## Requirements

- Windows or Linux (x86_64)
- osu!stable or osu!lazer
- A Twitch account

## Usage

1. Launch the app; it will automatically detect your osu! instance.
2. Visit the [companion website](https://osu-twitchbot.dyl.blue/) to get your Twitch OAuth token.
3. Copy and enter your token, then press "Connect".
4. Optionally, configure custom command settings.

When a viewer types one of your configured commands in chat, the bot responds with the respective information.

Closing the window minimizes the app to the system tray so the bot keeps running; reopen it or quit from the tray icon.

## Command-Line Arguments

| Argument        | Description                            |
| --------------- | -------------------------------------- |
| `--theme`, `-t` | `light`, `dark`, or `system` (default) |
| `--no-update`   | Disable auto-updater on start          |

## Command Placeholders

### Now Playing

| Placeholder | Description     |
| ----------- | --------------- |
| `{artist}`  | Song artist     |
| `{title}`   | Song title      |
| `{diff}`    | Difficulty name |
| `{creator}` | Mapper name     |
| `{id}`      | Beatmap ID      |
| `{mods}`    | Mods            |
| `{link}`    | Beatmap link    |
| `{status}`  | Beatmap status  |

Default:

```
{artist} - {title} [{diff}] ({creator}) {mods} | {status} {link}
```

### Performance Points

| Placeholder | Description        |
| ----------- | ------------------ |
| `{mods}`    | Mods               |
| `{pp_95}`   | PP at 95% accuracy |
| `{pp_97}`   | PP at 97% accuracy |
| `{pp_98}`   | PP at 98% accuracy |
| `{pp_99}`   | PP at 99% accuracy |
| `{pp_100}`  | PP at 100% (SS)    |

Default:

```
95%: {pp_95}pp | 97%: {pp_97}pp | 98%: {pp_98}pp | 99%: {pp_99}pp | 100%: {pp_100}pp {mods}
```

## File Locations

Settings are stored as `settings.toml`; logs are written to a `latest.log` (truncated each run) alongside timestamped archives. The Console tab has **Open Logs Folder** and **Open Config Folder** buttons.

|          | Windows                                          | Linux                                  |
| -------- | ------------------------------------------------ | -------------------------------------- |
| Settings | `%APPDATA%\osu-twitchbot\config\settings.toml`   | `~/.config/osu-twitchbot/settings.toml` |
| Logs     | `%LOCALAPPDATA%\osu-twitchbot\logs\`             | `~/.local/share/osu-twitchbot/logs/`   |

## Linux Notes

**Memory reading requires ptrace permission.** Most distros ship with `ptrace_scope=1`, which only allows reading a *child* process's memory — so reading an independently-launched osu! fails with a permission error. Pick one of:

```bash
# option 1: allow ptrace between your own processes (until reboot)
sudo sysctl kernel.yama.ptrace_scope=0
# make it permanent:
echo "kernel.yama.ptrace_scope = 0" | sudo tee /etc/sysctl.d/10-ptrace.conf

# option 2: grant the capability to the binary only
sudo setcap cap_sys_ptrace+ep ./osu-twitchbot-linux-x86_64
```

**Token persistence requires a Secret Service daemon** (gnome-keyring or KWallet — present on any normal desktop). Without one, connecting still works but the token isn't remembered across restarts.

**The tray icon requires AppIndicator support.** On GNOME, install the [AppIndicator extension](https://extensions.gnome.org/extension/615/appindicator-support/); KDE and most other desktops work out of the box. If no tray is available, closing the window quits the app instead.

## Building from Source

### Prerequisites

- [Rust](https://rustup.rs/)
- [just](https://github.com/casey/just) (optional, command runner)
- [jq](https://jqlang.github.io/jq/) (optional, for release builds)
- A [Twitch application](https://dev.twitch.tv/console/apps) with OAuth credentials
- A Twitch access token with scopes:
  - `channel:bot`
  - `user:read:chat`
  - `user:write:chat`

**Linux only:**

```bash
sudo apt install libdbus-1-dev pkg-config libgtk-3-dev libayatana-appindicator3-dev
# to also cross-compile the Windows binary from Linux:
sudo apt install gcc-mingw-w64-x86-64
rustup target add x86_64-pc-windows-gnu
```

The GTK/appindicator packages are only needed for the system tray; build with `--no-default-features` to skip it.

### Environment Setup

Create a `.env` file in the project root:

```env
TWITCH_CLIENT_ID=your_client_id_here
GITHUB_LATEST_RELEASE_URL=https://api.github.com/repos/medylme/osu-twitchbot/releases/latest # or set your own
TARGET_DIR=/path/to/target  # optional, for cross-compilation
DIST_DIR=/path/to/dist      # optional, for cross-compilation
SENTRY_DSN=https://...      # optional, sentry error reporting
```

`TWITCH_CLIENT_ID` is compiled into the binary at build time.

### Build

This project uses [just](https://github.com/casey/just) as a command runner.

```bash
just dev       # Run the app
just build     # Compile debug build
```

Check out the Justfile for all other available commands.

## Special Thanks

💙 to [ProcessMemoryDataFinder](https://github.com/Piotrekol/ProcessMemoryDataFinder)/[gosumemory](https://github.com/l3lackShark/gosumemory) (stable) and [tosu](https://github.com/tosuapp/tosu) (Lazer) for memory reading strategy and initial offsets.

## License

GPLv3
