# Waypoint

Project intelligence for Claude Code. Gives your AI assistant a file map and symbol index — saving 65-80% token overhead on codebase orientation.

## What it does

Waypoint runs as Claude Code hooks, injecting context automatically:

| Hook | Trigger | What happens |
|------|---------|--------------|
| **session-start** | New conversation | Auto-scans if no map exists or map is stale. |
| **pre-read** | Before Claude reads a file | Injects file description and token estimate from the map (works across projects) |

## What lives where

```
.waypoint/           ← per-project, gitignored
  map.md             ← file descriptions + token estimates (human-readable source of truth)
  map_index.db       ← SQLite index for O(1) map lookups + FTS5 symbol search + import tracking

~/Library/Application Support/waypoint/
  ledger.db          ← SQLite analytics (90-day retention)
```

## CLI commands

### `waypoint scan`

Walks the project respecting `.gitignore`, extracts descriptions using tree-sitter (Rust, TypeScript, JavaScript, Python, Go) with regex fallback for 15+ other formats. Writes `.waypoint/map.md`.

```sh
waypoint scan              # Generate/regenerate the map
waypoint scan --check      # Exit non-zero if map is stale
waypoint scan --all        # Scan all immediate child git repos (smart default: walks up if inside a project)
waypoint scan --all ~/repos  # Explicit parent directory
```

### `waypoint sketch`

Look up a symbol's signature and location without reading the full file.

```sh
waypoint sketch SessionStart      # shows file, line range, and signature
```

### `waypoint find`

Full-text search across all indexed symbols (function names, structs, classes, types).

```sh
waypoint find "token savings"     # BM25-ranked results from the symbol index
waypoint find "scan" --limit 5
```

### `waypoint callers`

Find all files that import a given symbol. Queries the imports table (populated by `waypoint scan`) joined against the symbols table to validate targets.

```sh
waypoint callers AppError              # current project
waypoint callers STATUS_CODES -C ~/repos/neb-ms-billing  # another project
```

### `waypoint gain`

Token savings analytics from the ledger.

```sh
waypoint gain            # current project
waypoint gain --global   # all projects
```

### `waypoint status`

Health check — map freshness, ledger summary.

```sh
waypoint status
```

## Getting Claude to use it

The hooks handle the automatic plumbing (map lookups, context injection). Import `WAYPOINT.md` into your global `~/.claude/CLAUDE.md` to give Claude the operating protocol — token discipline and navigation rules:

```markdown
@~/repos/waypoint/WAYPOINT.md
```

See [SETUP.md](SETUP.md) for full details.

## Cross-project map lookups

When Claude reads a file outside the current project, the pre-read hook automatically resolves the file's own project root and serves map context from that project's `.waypoint/` directory. This works for sibling repos, nested repos (submodules), and any waypoint-managed project on disk.

For this to work, the target project needs to have been scanned at least once. Pre-warm all your repos in one pass:

```sh
waypoint scan --all ~/repos
```

Maps stay current because session-start rescans when the map is stale (older than 7 days or file count drifted more than 3%). For repos you don't touch often, a periodic re-scan keeps them fresh — just re-run `waypoint scan --all`.

## Setup

See [SETUP.md](SETUP.md) for full installation instructions (binary, hooks, settings.json).
