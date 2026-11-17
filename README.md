<p align="center">
  <img src="assets/icon.png" alt="osu-twitchbot icon" width="128">
</p>

# osu! twitchbot

A lightweight desktop app that reads what you are doing in osu! and answers Twitch chat commands about it. Supports both Stable and Lazer.

## Commands

| Viewer types          | The bot replies with                                         |
| --------------------- | ------------------------------------------------------------ |
| `!np`                 | The current map, mods and a link                             |
| `!pp`                 | PP for the current map at 95%, 97%, 98%, 99% and 100%        |
| `!pp 98.5 2m +HDDT`   | PP for that score: accuracy, combo, misses and mods of choice |
| `!rank`               | Your rank, PP, accuracy, play count and level                |

```
Artist - Title [Difficulty] (Creator) +HDDT | Ranked https://osu.ppy.sh/b/123456
95%: 350pp | 97%: 400pp | 98%: 450pp | 99%: 500pp | 100%: 550pp (+HDDT)
98.5% | 800x | 2 miss | 312pp (+HDDT)
Player: #12,345 | 8,123pp | 98.76% | 54,321 plays | Lv. 100
```

Command names, replies and the cooldown are all yours to change. See [Commands](docs/commands.md) for every placeholder, including map stats such as star rating, BPM and length.

## Requirements

- Windows or Linux (x86_64); Linux needs a little setup, see [Linux notes](docs/linux.md)
- osu!stable or osu!lazer
- A Twitch account

## Getting Started

1. Download the [latest release](https://github.com/medylme/osu-twitchbot/releases/latest) and launch it; it detects your osu! instance automatically.
2. Visit the [companion website](https://osu-twitchbot.dyl.blue/) to get your Twitch OAuth token.
3. Copy and enter your token, then press "Connect".
4. Optionally, change the commands and their replies in the Commands tab.

Closing the window minimizes the app to the system tray so the bot keeps running; reopen it or quit from the tray icon.

The Data tab shows everything the bot currently reads from osu!. Click any value there to copy its placeholder.

## Command-Line Arguments

| Argument        | Description                            |
| --------------- | -------------------------------------- |
| `--theme`, `-t` | `light`, `dark`, or `system` (default) |
| `--no-update`   | Disable auto-updater on start          |

## File Locations

Settings are stored as `settings.toml`; logs are written to a `latest.log` (truncated each run) alongside timestamped archives. The Console tab has **Open Logs Folder** and **Open Config Folder** buttons.

|          | Windows                                          | Linux                                  |
| -------- | ------------------------------------------------ | -------------------------------------- |
| Settings | `%APPDATA%\osu-twitchbot\config\settings.toml`   | `~/.config/osu-twitchbot/settings.toml` |
| Logs     | `%LOCALAPPDATA%\osu-twitchbot\logs\`             | `~/.local/share/osu-twitchbot/logs/`   |

## Building from Source

See [Building from source](docs/building.md).

## Special Thanks

💙 to [ProcessMemoryDataFinder](https://github.com/Piotrekol/ProcessMemoryDataFinder)/[gosumemory](https://github.com/l3lackShark/gosumemory) (stable) and [tosu](https://github.com/tosuapp/tosu) (Lazer) for memory reading strategy and initial offsets.

## License

GPLv3
