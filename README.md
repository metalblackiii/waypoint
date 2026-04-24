# Waypoint

Project intelligence for Claude Code or Codex. Gives your AI assistant a file map and symbol index — saving 65-80% token overhead on codebase orientation.

## Why Waypoint

- Reduces blind full-file reads with map-first context.
- Speeds up code navigation with symbol search (`waypoint find`, `waypoint sketch`).
- Makes change risk visible before commit (`waypoint impact`).
- Works across multiple local repos, not just the current one.

## Language support

- First-class parsing (tree-sitter): Rust, TypeScript, JavaScript, Python, Go.
- Additional formats: regex-based fallback heuristics for many other source/config file types.

## Quickstart (2 minutes)

```sh
cargo install --path .
waypoint scan
waypoint status
waypoint impact
```

For hook setup and global agent instructions, see [SETUP.md](SETUP.md).

## What it does

Waypoint runs as Claude Code/Codex hooks, injecting context automatically:

| Hook | Trigger | What happens |
|------|---------|--------------|
| **session-start** | New conversation | Auto-scans if no map exists or map is stale, then injects 2-line `[waypoint] arch:` context for repos with >=20 scannable files. |
| **pre-read** | Before the AI Agent reads a file | Injects file description and token estimate from the map (works across projects) |

Session-start arch context details:

- **Map is stale** means map age is over 7 days or file-count drift is over 3%.
- **Scannable files** means files included by scan after `.gitignore`/filtering rules.
- Arch context appears only when scannable file count is `>=20`.
- Output shape is always 2 lines:
  - `[waypoint] arch: <top languages by %>`
  - `[waypoint] arch: hotspots: <top dirs by imports-in>`

## What lives where

```
.waypoint/           ← per-project, gitignored
  map.md             ← file descriptions + token estimates + architecture summary section (human-readable source of truth)
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
waypoint scan --all /path/to/repos  # Explicit parent directory
```

### `waypoint sketch`

Look up a symbol's signature and location without reading the full file.

```sh
waypoint sketch <symbol-name-from-find-results>  # shows file, line range, and signature
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
waypoint callers STATUS_CODES -C /path/to/repos/another-project  # another project
```

### `waypoint arch`

Print cached architecture summary for a project in the same hook format (`lang dist` + optional `hotspots`).

```sh
waypoint arch
waypoint arch -C /path/to/repos/another-project
```

If summary data is missing or stale, waypoint prints guidance to run `waypoint scan`.

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

## Typical workflow

```sh
# 1) Build map context
waypoint scan

# 2) Get architecture context (especially when switching repos)
waypoint arch
# or:
waypoint arch -C /path/to/other-repo

# 3) Locate symbols before opening files
waypoint find "scan" --limit 5
waypoint sketch <symbol-name-from-find-results>

# 4) Make changes, then check blast radius
waypoint impact
```

## Setup

For installation, hooks, and global agent configuration (`WAYPOINT.md` copy/import flow), see [SETUP.md](SETUP.md).

## Known limitations

- Symbol checks are repo-dependent: `waypoint find`/`sketch` may return no matches in non-code or low-symbol repos.
- `waypoint gain` is an estimated upper bound ("max savings"), not a counterfactual measurement of exact tokens that would have been spent without waypoint.
- `waypoint impact` depends on a fresh map/index; if you see a staleness warning, run `waypoint scan` first.
- Hook-powered context injection requires hook setup; without configured hooks, context lines are not auto-injected.
- `waypoint impact` output is text-only in v1 (no JSON mode yet).
- Hidden files/directories (`.`-prefixed) are intentionally excluded from scan/indexing to avoid over-indexing system/metadata paths; dotfile-heavy repos may see more lookup misses.

## Cross-project map lookups

When Claude reads a file outside the current project, the pre-read hook automatically resolves the file's own project root and serves map context from that project's `.waypoint/` directory. This works for sibling repos, nested repos (submodules), and any waypoint-managed project on disk.

Arch context is emitted at session start for the session root repo. On cross-repo pre-read, you get `[waypoint] foreign:` plus file map context for the target repo. For explicit cross-repo architecture lookup at any time, use `waypoint arch -C <repo>`.

Typical cross-repo signals:

```text
[waypoint] foreign: /path/to/other-repo
[waypoint] map: ...
```

For this to work, the target project needs to have been scanned at least once. Pre-warm all your repos in one pass:

```sh
waypoint scan --all /path/to/repos
```

Maps stay current because session-start rescans when the map is stale (older than 7 days or file count drifted more than 3%). For repos you don't touch often, a periodic re-scan keeps them fresh — just re-run `waypoint scan --all`.
