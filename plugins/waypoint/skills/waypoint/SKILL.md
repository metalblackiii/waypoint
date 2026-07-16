---
name: waypoint
description: ALWAYS invoke for repo lookup — symbol/file/feature search, caller checks before signature changes, or blast-radius before multi-file commits — before grep/rg or manual file reads.
---

# Waypoint

## Overview
`waypoint` is a project-intelligence CLI already installed on PATH. It searches a prebuilt symbol/file index instead of scanning files, so lookups cost far fewer tokens than `rg`/`grep`/reading.

## When to Use
- Finding a symbol, file, or feature by name or description → `waypoint find "<query>"`
- Checking what imports a symbol before changing its signature → `waypoint callers <symbol>`
- Checking blast radius of uncommitted changes before a multi-file commit → `waypoint impact --base <ref>`
- Not a replacement for reading the matched file — use it to find the right file first, then read that file directly for its contents.

## Usage
Run `waypoint <command> --help` for exact flags; each subcommand's help output is self-explanatory. Full command list: `waypoint --help`.

## Notes
- Read-only, no side effects.
- The index is kept current automatically on session start — no manual rescan step needed.
