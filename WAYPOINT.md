### Search: waypoint > rg > Grep tool > grep

Prefer `waypoint` for symbols/signatures and `rg` for text/shell contexts. Use `Grep` tool (or `grep`) only as fallback when needed.

If your environment enforces stricter search policy, follow local/global policy files and hooks.

## Waypoint

Use Waypoint for navigation efficiency and impact analysis.

- On file reads, check `[waypoint] map:` context first. If it answers the question, skip full file read.
- Use `waypoint find` for symbols/signatures; use `rg` for text/config/string search.
- For specific symbol lookup, use `waypoint sketch <name>` before reading. Skip sketch for files under ~150 tokens. **For files over ~200 tokens, sketch is mandatory unless the file was already read this session.**
- When changing exported signatures, run `waypoint callers <name>`.
- Before commit, run `waypoint impact` to assess blast radius. It maps changed symbols to their importers and classifies risk (CRITICAL/HIGH/MEDIUM/LOW). Use `waypoint impact --base <ref>` to diff against a specific branch.
