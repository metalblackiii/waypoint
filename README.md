# Waypoint

Project intelligence for Claude Code. Gives your AI assistant a file map, cross-session memory, and bug fix dedup — saving 65-80% token overhead on codebase orientation.

## What it does

Waypoint runs as Claude Code hooks, injecting context automatically:

| Hook | Trigger | What happens |
|------|---------|--------------|
| **session-start** | New conversation | Injects journal (preferences, learnings, do-not-repeat). Auto-scans if no map exists. |
| **pre-read** | Before Claude reads a file | Injects file description and token estimate from the map |
| **pre-write** | Before Claude edits a file | Surfaces known bug traps for that file |
| **post-write** | After Claude edits a file | Incrementally updates the file's map entry |
| **post-failure** | After a tool error | Suggests searching traps for known fixes |

## What lives where

```
.waypoint/           ← per-project, gitignored
  map.md             ← file descriptions + token estimates
  journal.md         ← cross-session memory (preferences, learnings, do-not-repeat)
  traps.json         ← bug fix log with dedup

~/Library/Application Support/waypoint/
  ledger.db          ← SQLite analytics (90-day retention)
```

## CLI commands

### `waypoint scan`

Walks the project respecting `.gitignore`, extracts descriptions using tree-sitter (Rust, TypeScript, JavaScript, Python, Go) with regex fallback for 15+ other formats. Writes `.waypoint/map.md`.

```sh
waypoint scan           # Generate/regenerate the map
waypoint scan --check   # Exit non-zero if map is stale
```

### `waypoint journal add`

Add entries to the cross-session journal. Claude sees these at the start of every conversation.

```sh
# Things you want Claude to always do
waypoint journal add --section preferences "Use jiff instead of chrono for timezone-aware dates"

# Things Claude learned during a session
waypoint journal add --section learnings "tree-sitter grammar crates use tree-sitter-language as intermediary"

# Mistakes Claude should not repeat
waypoint journal add --section do-not-repeat "Don't use unbuffered File writes — wrap in BufWriter"
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

### `waypoint gain`

Token savings analytics from the ledger.

```sh
waypoint gain
```

### `waypoint status`

Health check — map freshness, entry counts, journal size, trap count.

```sh
waypoint status
```

## Getting Claude to use it

The hooks handle the automatic plumbing (map lookups, context injection, incremental updates). To get Claude to *actively record* learnings and traps, import `WAYPOINT.md` into your global `~/.claude/CLAUDE.md`:

```markdown
@~/repos/waypoint/WAYPOINT.md
```

This gives Claude the operating protocol — mandatory journal updates on corrections, learning logging triggers, bug trap rules, and token discipline. See [SETUP.md](SETUP.md) for full details.

## Setup

See [SETUP.md](SETUP.md) for full installation instructions (binary, hooks, settings.json).
