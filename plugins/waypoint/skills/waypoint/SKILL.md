---
name: waypoint
description: ALWAYS invoke for symbol, file, or feature lookup, caller checks before signature changes, or blast radius before multi-file commits, ahead of grep/rg or file reads
---

# Waypoint

Query the prebuilt index first, then read only the files it returns

- Symbol, file, or feature by name or description → `waypoint find "<query>"`
- Importers of a symbol before changing its signature → `waypoint callers <symbol>`
- Blast radius of uncommitted changes before a multi-file commit → `waypoint impact --base <ref>`

Flags: `waypoint <command> --help`. Skip `waypoint scan`; session hooks keep the index current
