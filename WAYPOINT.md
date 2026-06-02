### Search: waypoint > rg > Grep tool > grep

Prefer `waypoint` for symbols/signatures and `rg` for text/shell search.

## Waypoint

Run before the corresponding action — not after:

- **Locating a function/const by name or behavior**: `waypoint find "<query>"` — indexes top-level functions/consts only; misses object-literal methods and class statics (use `rg` for those).
- **Before changing an exported signature**: `waypoint callers <name>` — cross-file import sites.
- **Reading a large or unfamiliar symbol**: `waypoint sketch <name>` → Read the returned range with `offset`/`limit`. For a known small symbol, `rg <name>` + a targeted read is faster — skip sketch.

`[waypoint] map:` is injected on file reads; if it answers the question, skip the full read.

Situational, not per-action: `ask` (file ranking; weak on concept queries), `impact` (pre-commit blast radius), `arch` (new-repo overview), `trace` (call chains — same-file only, v1). `scan`/`status`/`gain` are maintenance.
