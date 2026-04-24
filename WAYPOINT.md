### Search: waypoint > rg > Grep tool > grep

Prefer `waypoint` for symbols/signatures and `rg` for text/shell search. Use `Grep`/`grep` only as fallback.

## Waypoint

Use Waypoint for navigation efficiency and impact analysis.

- On file reads, check `[waypoint] map:` context first. If it answers the question, skip full file read.
- When switching repos, run `waypoint arch` (or `waypoint arch -C /path/to/repo`) first for languages and hotspots.
- Use `waypoint find` for symbols/signatures; use `rg` for text/config/string search.
- For specific symbol lookup, run `waypoint sketch <name>` before reading; skip only for files under ~150 tokens. **For files over ~200 tokens, sketch is mandatory unless already read this session.**
- When changing exported signatures, run `waypoint callers <name>`.
- Before commit, run `waypoint impact` (or `waypoint impact --base <ref>`). It maps changed symbols to importers and classifies risk (CRITICAL/HIGH/MEDIUM/LOW).
