### Search: waypoint > rg > Grep tool > grep

Prefer `waypoint` for symbols/signatures and `rg` for text/shell search.

## Waypoint

Run before the corresponding action — not after:

- **Locating a function/const by name or behavior**: `waypoint find "<query>"` — indexes functions, consts, object-literal methods, and class fields. Results spanning ≤3 files append a "see also:" footer of up to 5 sibling exported symbols per file.
- **Before changing an exported signature**: `waypoint callers <name>` — cross-file import sites.

`[waypoint] map:` is injected on file reads; if it answers the question, skip the full read.

Situational, not per-action: `ask` (file ranking; weak on concept queries), `impact` (pre-commit blast radius), `arch` (new-repo overview). `scan`/`status`/`gain` are maintenance.
