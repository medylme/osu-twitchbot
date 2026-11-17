# Building from Source

## Prerequisites

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

## Environment Setup

Create a `.env` file in the project root:

```env
TWITCH_CLIENT_ID=your_client_id_here
GITHUB_LATEST_RELEASE_URL=https://api.github.com/repos/medylme/osu-twitchbot/releases/latest # or set your own
TARGET_DIR=/path/to/target  # optional, for cross-compilation
DIST_DIR=/path/to/dist      # optional, for cross-compilation
SENTRY_DSN=https://...      # optional, sentry error reporting
```

`TWITCH_CLIENT_ID` is compiled into the binary at build time.

## Build

This project uses [just](https://github.com/casey/just) as a command runner.

```bash
just dev       # Run the app
just build     # Compile debug build
```

Check out the Justfile for all other available commands.
