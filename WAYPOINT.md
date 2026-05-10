### Search: waypoint > rg > Grep tool > grep

Prefer `waypoint` for symbols/signatures and `rg` for text/shell search.

## Waypoint

- On file reads, check `[waypoint] map:` context first. If it answers the question, skip full file read.
- When switching repos, run `waypoint arch` (or `waypoint arch -C /path/to/repo`) first for languages and hotspots.
- `waypoint find "<query>"` — don't know the exact symbol name; searches broadly. `waypoint sketch <name>` — know the exact name, about to Read; returns line range to scope the read. Symbol names only — filenames and kebab paths always miss; `find` first if unsure.
- Map context shows >~200 tok → sketch before reading.
- When changing exported signatures, run `waypoint callers <name>`.
- Before commit, run `waypoint impact` (or `waypoint impact --base <ref>`).
