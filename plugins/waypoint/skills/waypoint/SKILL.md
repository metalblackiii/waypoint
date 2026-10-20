---
name: waypoint
description: ALWAYS invoke for symbol, file, or feature lookup, caller checks before signature changes, or blast radius before multi-file commits, ahead of grep/rg or file reads
---

# Waypoint

Query the prebuilt index first, then read only the files it returns

- Symbol, file, or feature → `waypoint find "<name or phrase>"`; symbol and file rows are index matches on words in the name or path, not on meaning
- `Files ranked by matching words` block → candidates, not answers: open a `strong` file first; treat each `possible` file as a lead to check against its `matched:` words
- `Leads in sibling repos` block → only names and paths missed here: when the task is scoped to this repo, `rg` it first; otherwise check each listed absolute path against its `matched:` words
- `No match … Use rg.` or `No symbols found` → switch to `rg`; do not reword and retry `find`
- Importers of a symbol before changing its signature → `waypoint callers <symbol>`
- Blast radius of uncommitted changes before a multi-file commit → `waypoint impact --base <ref>`

Flags: `waypoint <command> --help`. Skip `waypoint scan`; session hooks keep the index current
