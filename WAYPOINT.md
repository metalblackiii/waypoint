### Search: waypoint > rg > Grep tool > grep

Prefer `waypoint` for symbols/signatures and `rg` for text/shell search.

## Waypoint

Run before the corresponding action — not after discovering you needed it:

- **Before opening files for a new task**: `waypoint ask "<task>"` — ranks files by relevance (`--explain` to debug signal)
- **Before reading a file (symbol name known)**: `waypoint sketch <name>` — returns line range; pass it to Read as `offset`/`limit`
- **Before reading (symbol name unknown)**: `waypoint find "<query>"` — symbol names only; kebab paths miss
- **Before changing an exported signature**: `waypoint callers <name>` — all import sites
- **Before refactoring a call chain**: `waypoint trace <symbol>` — `--direction inbound|outbound|both`, `--depth N`
- **Before committing**: `waypoint impact` (or `--base <ref>` for non-default base)
- **On new repo or session**: `waypoint arch`

`[waypoint] map:` is injected on file reads. If it answers the question, skip the full read. If >~200 tok, run `waypoint sketch <name>` to scope the Read first.
