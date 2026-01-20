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
| `{mods}`    | Active mods     |
| `{link}`    | Beatmap link    |
| `{status}`  | Beatmap status  |

Default:

```
{artist} - {title} [{diff}] ({creator}) {mods} | {status} {link}
```

### Performance Points

| Placeholder | Description        |
| ----------- | ------------------ |
| `{mods}`    | Active mods        |
| `{pp_95}`   | PP at 95% accuracy |
| `{pp_97}`   | PP at 97% accuracy |
| `{pp_98}`   | PP at 98% accuracy |
| `{pp_99}`   | PP at 99% accuracy |
| `{pp_100}`  | PP at 100% (SS)    |

Default:

```
{mods} 95%: {pp_95}pp | 97%: {pp_97}pp | 98%: {pp_98}pp | 99%: {pp_99}pp | 100%: {pp_100}pp
```

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
sudo apt install libdbus-1-dev pkg-config
sudo apt install musl-tools
cargo install cross --git github.com
```

### Environment Setup

Create a `.env` file in the project root:

```env
TWITCH_CLIENT_ID=your_client_id_here
GITHUB_LATEST_RELEASE_URL=https://api.github.com/repos/medylme/osu-twitchbot/releases/latest # or set your own
TARGET_DIR=/path/to/target  # optional, for cross-compilation
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
