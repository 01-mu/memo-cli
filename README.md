# memo-cli

`memo` is a CLI tool that lets you safely store shell commands and explicitly reuse them later.

## Usage

```sh
# save last command from zsh history and list
memo

# save an explicit command
memo save gh pr list --search "label:release sort:updated-desc" --state open --limit 20

# list (default 10) or filtered
memo
memo gh pr

# copy, print, or run by number
memo 3
memo print 3
memo run 3
```

Notes:
- `memo` with no args saves the most recent zsh history command (excluding `memo` itself) and then lists entries.
- `memo <query>` only narrows what you see; it does not save anything.
- Use `memo save <cmd...>` to save explicitly.
- `memo print <N>` is for piping or editing (e.g. `memo print 3 | pbcopy`).

## Storage

SQLite database at `$XDG_STATE_HOME/memo/memo.sqlite3` (fallback: `~/.local/state/memo/memo.sqlite3`).

## Build

```sh
cargo install --path .
```

## Zsh Integration

Source the widget to make `memo␠` open a selector and insert the chosen command:

```sh
source /path/to/memo-cli/scripts/memo.zsh
```
