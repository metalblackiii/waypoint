### Search: waypoint > rg > Grep tool > grep

Prefer `waypoint` for symbols/signatures and `rg` for text/shell search.

## Waypoint

### Hook Context

- On file reads, check `[waypoint] map:` context first. If it answers the question, skip full file read.
- Map context shows >~200 tok → sketch before reading.

### When to Use Each Command

| Situation | Command |
|-----------|---------|
| Switching repos or first session | `waypoint arch` — languages and hotspots |
| About to read a file, know the symbol name | `waypoint sketch <name>` — returns line range to scope the read |
| Don't know the exact symbol name | `waypoint find "<query>"` — broad FTS search. Symbol names only — filenames and kebab paths miss; `find` first if unsure |
| Starting a task, need to know which files to touch | `waypoint ask "<task description>"` — ranks files by relevance to a natural-language task. `--explain` for signal breakdown |
| Changing an exported signature | `waypoint callers <name>` — all files importing that symbol |
| Understanding control flow before refactoring | `waypoint trace <symbol>` — walk same-file call chains. `--direction inbound` (who calls this?), `outbound` (what does this call?), or `both` (default). `--depth N` limits hops |
| Before committing | `waypoint impact` (or `waypoint impact --base <ref>`) — blast radius of changes |
