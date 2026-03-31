# Waypoint

Project intelligence for Claude Code. Gives your AI assistant a file map and bug fix dedup — saving 65-80% token overhead on codebase orientation.

## What it does

Waypoint runs as Claude Code hooks, injecting context automatically:

| Hook | Trigger | What happens |
|------|---------|--------------|
| **session-start** | New conversation | Auto-scans if no map exists. Injects trap log reminder. |
| **pre-read** | Before Claude reads a file | Injects file description and token estimate from the map (works across projects) |
| **pre-write** | Before Claude edits a file | Surfaces known bug traps for that file |
| **post-write** | After Claude edits a file | Incrementally updates the file's map entry |
| **post-failure** | After a tool error | Suggests searching traps for known fixes |

## What lives where

```
.waypoint/           ← per-project, gitignored
  map.md             ← file descriptions + token estimates (human-readable source of truth)
  map_index.db       ← SQLite index for O(1) map lookups + FTS5 symbol search
  traps.json         ← bug fix log with dedup

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

### `waypoint trap log`

Record a bug fix so Claude doesn't repeat it. Deduplicates by Jaccard similarity per file.

```sh
waypoint trap log \
  --error "FromSql not implemented for usize" \
  --file "src/ledger.rs" \
  --cause "rusqlite 0.39 dropped FromSql for usize" \
  --fix "Change count fields from usize to i64" \
  --tags "rusqlite,upgrade"
```

### `waypoint trap search`

Search traps by keyword.

```sh
waypoint trap search "FromSql"
```

### `waypoint trap prune`

Remove old trap entries.

```sh
waypoint trap prune --older-than 90d
waypoint trap prune --older-than 90d --all   # prune across all sibling projects
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

### `waypoint gain`

Token savings analytics from the ledger.

```sh
waypoint gain            # current project
waypoint gain --global   # all projects
```

### `waypoint status`

Health check — map freshness, trap count, ledger summary.

```sh
waypoint status
```

## Getting Claude to use it

The hooks handle the automatic plumbing (map lookups, context injection, incremental updates). To get Claude to *actively record* traps, import `WAYPOINT.md` into your global `~/.claude/CLAUDE.md`:

```markdown
@~/repos/waypoint/WAYPOINT.md
```

This gives Claude the operating protocol — bug trap rules and token discipline. See [SETUP.md](SETUP.md) for full details.

## Cross-project map lookups

When Claude reads a file outside the current project, the pre-read hook automatically resolves the file's own project root and serves map context from that project's `.waypoint/` directory. This works for sibling repos, nested repos (submodules), and any waypoint-managed project on disk.

For this to work, the target project needs to have been scanned at least once. Pre-warm all your repos in one pass:

```sh
waypoint scan --all ~/repos
```

Maps stay current in projects you actively edit (the post-write hook updates entries incrementally). For repos you don't touch often, a periodic re-scan keeps them fresh — just re-run `waypoint scan --all`.

## Setup

See [SETUP.md](SETUP.md) for full installation instructions (binary, hooks, settings.json).
