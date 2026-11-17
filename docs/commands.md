# Commands

When a viewer types one of your commands in chat, the bot replies using that command's format. Commands and formats are set in the Commands tab; a placeholder such as `{title}` is replaced with the current value.

The Data tab shows the current value of every placeholder. Click a value to copy its placeholder.

Replies share a cooldown (1 second by default, `0` turns it off), also set in the Commands tab.

- [Now Playing](#now-playing) (`!np`)
- [Performance Points](#performance-points) (`!pp`)
- [Rank](#rank) (`!rank`)

## Now Playing

Default command: `!np`

Default format:

```
{artist} - {title} [{diff}] ({creator}) {mods} | {status} {link}
```

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
| `{state}`   | `Playing`, `Editing`, `Song select`, `Results` or `Idle` |

### Map stats

These are adjusted for the selected mods: DT, for example, raises `{stars}`, `{bpm}`, `{ar}` and `{od}` and shortens `{length}`. An osu! map played in another mode is rated as that mode's convert.

| Placeholder   | Description                  |
| ------------- | ---------------------------- |
| `{stars}`     | Star rating                  |
| `{bpm}`       | BPM                          |
| `{length}`    | Length, first to last object |
| `{cs}`        | Circle size                  |
| `{ar}`        | Approach rate                |
| `{od}`        | Overall difficulty           |
| `{hp}`        | HP drain                     |
| `{max_combo}` | Max combo                    |
| `{objects}`   | Number of hit objects        |

## Performance Points

Default command: `!pp`

Default format:

```
95%: {pp_95}pp | 97%: {pp_97}pp | 98%: {pp_98}pp | 99%: {pp_99}pp | 100%: {pp_100}pp ({mods})
```

| Placeholder | Description        |
| ----------- | ------------------ |
| `{mods}`    | Mods               |
| `{pp_95}`   | PP at 95% accuracy |
| `{pp_97}`   | PP at 97% accuracy |
| `{pp_98}`   | PP at 98% accuracy |
| `{pp_99}`   | PP at 99% accuracy |
| `{pp_100}`  | PP at 100% (SS)    |

### Arguments

Viewers can add arguments after the command, in any order:

| Argument  | Example | Description                                  |
| --------- | ------- | -------------------------------------------- |
| Accuracy  | `98.5`  | PP for that accuracy                         |
| Combo     | `800x`  | Max combo of the score (only affects stable) |
| Misses    | `2m`    | Number of misses                             |
| Mods      | `+HDDT` | Use these mods instead of the selected ones (`+NM` for none) |

Mods alone (`!pp +HDDT`) keep the format above. With an accuracy, combo or misses (`!pp 98.5 800x 2m +HDDT`) the reply uses a second format.

Default format:

```
{acc}% | {combo}x | {misses} miss | {pp}pp ({mods})
```

| Placeholder | Description |
| ----------- | ----------- |
| `{acc}`     | Accuracy    |
| `{combo}`   | Combo       |
| `{misses}`  | Misses      |
| `{pp}`      | PP          |
| `{mods}`    | Mods        |

## Rank

Replies with the profile of the player logged in to osu!, read from the game itself.

On stable the stats follow the selected mode. On lazer they are always those of your profile's default mode.

Default command: `!rank`

Default format:

```
{username}: #{rank} | {pp}pp | {acc}% | {playcount} plays | Lv. {level}
```

| Placeholder   | Description |
| ------------- | ----------- |
| `{username}`  | Username    |
| `{rank}`      | Global rank |
| `{pp}`        | Total PP    |
| `{acc}`       | Accuracy    |
| `{playcount}` | Play count  |
| `{level}`     | Level       |
