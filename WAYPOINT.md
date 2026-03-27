# Waypoint Operating Protocol

You are working in a Waypoint-managed project. These rules apply every turn.

## File Navigation

1. Check the `[waypoint] map:` context injected on every Read — it has a description and token estimate for the file.
2. If the description is sufficient for your task, do NOT read the full file.
3. When you need a specific symbol (function, class, type), use `waypoint sketch <name>` before reading the file — it gives you the signature and line range. **Skip sketch for files under ~150 tokens** (check the map annotation — roughly 10-15 lines of code).
4. `waypoint find` vs `Grep` — use the right tool for the job:
   - `waypoint find "<query>"` — symbol names, function signatures, struct/class definitions
   - `Grep` — string literals, comments, config values, error messages, non-code text
5. If a file is not in the map or symbol index, search with Grep/Glob. The post-write hook will add it automatically when you create or edit it.
6. Use `waypoint scan --check` to detect stale map entries.

## Code Generation

1. Respect every entry in the Waypoint journal (injected at session start). If session context was compressed, re-read `.waypoint/journal.md`.
2. Check the `## Do-Not-Repeat` section — these are past mistakes that must not recur.
3. Follow all conventions in `## Preferences`.
4. Watch for `[waypoint] learnings for <file>:` annotations on pre-read — these are contextual learnings relevant to the file you're reading.

## After Actions

1. After renaming or deleting files: run `waypoint scan` to update the map. (Edits and creates are handled automatically by the post-write hook.)
2. Traps, learnings, and journal writes are **batched at session end** — note key details (error messages, file paths, root causes) inline in your responses as you go, then write them all in the Session End pass. The only inline write is `waypoint scan` for renames/deletes.

## Journal (MANDATORY — every session)

The journal stores preferences and past mistakes. You MUST update it when relevant. This is not optional.

**Update `preferences` when the user:**
- Corrects your approach ("no, do it this way instead")
- Expresses a style preference (naming, structure, formatting)
- Shows a preferred workflow or tool choice
- Rejects a suggestion — record what they preferred instead
- Asks for more/less detail, verbosity, explanation

**Update `do-not-repeat` (with date) when:**
- The user corrects a mistake you made
- You try something that fails and find the right approach
- You discover a gotcha that would trip up a fresh session

```sh
waypoint journal add --section <preferences|do-not-repeat> "<entry>"
```

## Learnings (MANDATORY — every session)

Learnings are contextual knowledge stored in `.waypoint/learnings.json` and surfaced automatically on pre-read when the file being read matches a learning's tags. You MUST log learnings when you discover something useful.

**Log a learning when you discover:**
- A project convention not obvious from the code (e.g., "tests go in __tests__/ not test/")
- A framework-specific pattern this project uses
- An API behavior that surprised you
- A dependency quirk or version constraint
- How modules connect or data flows through the system

```sh
waypoint learning add "<entry>" --tags "<file-or-dir-paths>"
```

**Tagging is critical.** Tag learnings with the file paths or directory prefixes they relate to. Directory tags must end with `/`. Learnings surface automatically on pre-read when a file matches a tag — untagged learnings never surface contextually.

Examples:
- `--tags "src/map/index.rs"` — surfaces when reading that specific file
- `--tags "src/hook/"` — surfaces when reading any file under `src/hook/`
- `--tags "src/trap.rs,src/learning.rs"` — surfaces for either file

**The bar is LOW.** If in doubt, add it. A learning that's slightly redundant costs nothing. A missing learning means the next session repeats the same discovery process.

## Bug Logging (MANDATORY)

**Log a trap whenever ANY of these happen:**
- The user reports an error, bug, or problem
- A test fails or a command produces an error
- You fix something that was broken
- You edit a file more than twice to get it right
- An import, module, or dependency is missing or wrong
- A runtime error, type error, or syntax error occurs
- A build, lint, or type check fails
- A feature doesn't work as expected
- You change error handling, try/catch blocks, or validation logic
- The user says something "doesn't work", "is broken", or "shows wrong X"

**Before fixing:** Search existing traps — the fix may already be known.

```sh
waypoint trap search "<keyword>"
```

**After fixing:** Note the details (error, file, cause, fix) inline in your response — the actual `trap log` call happens in the Session End pass. Template for reference:

```sh
waypoint trap log \
  --error "<exact error or complaint>" \
  --file "<file that was fixed>" \
  --cause "<why it broke>" \
  --fix "<what you changed>" \
  --tags "<relevant,keywords>"
```

**The threshold is LOW.** When in doubt, log it. A false positive costs nothing. A missed trap means repeating the same fix later.

## Symbol Index

After `waypoint scan`, a symbol index is available in `map_index.db` alongside the file map.

- `waypoint sketch <name>` — show file location and signature for a symbol (function, struct, class, etc.). **Use this before reading a file** when you need a specific function, class, or type — it returns the signature and location without spending tokens on the full file.
- `waypoint find "<query>"` — full-text search across all indexed symbols. Use this for symbol lookups (names, signatures, definitions). For string literals, comments, config values, and error messages, use Grep instead.

**Preferred lookup order:**
1. Map description (injected on Read) — often sufficient, zero cost. Learnings for the file are also surfaced here automatically.
2. `waypoint sketch` / `waypoint find` — precise symbol info, minimal tokens
3. Grep/Glob — when the symbol index doesn't cover what you need (comments, string literals, config values)
4. Full file Read — last resort for understanding surrounding context

## Cross-Project Work

When you read or edit a file outside the cwd project, the hooks resolve the correct project automatically. Watch for these annotations:

- `[waypoint] foreign: /path/to/other-repo` — the pre-read hook detected a foreign project with waypoint data. Remember this path.
- Pre-write and post-write hooks automatically check traps and update maps in the foreign project.

**When working in a foreign project, use `-C` to target its waypoint data:**

```sh
waypoint sketch -C /path/to/other-repo SymbolName
waypoint find -C /path/to/other-repo "query"
waypoint trap search -C /path/to/other-repo "keyword"
waypoint learning add -C /path/to/other-repo "entry" --tags "src/"
waypoint journal add -C /path/to/other-repo --section preferences "entry"
```

**`trap log --file` auto-resolves** — no `-C` needed. The `--file` path determines which project's traps to write to:

```sh
waypoint trap log --file /path/to/other-repo/src/foo.js --error "..." --cause "..." --fix "..." --tags "..."
```

This writes to `other-repo/.waypoint/traps.json` with a project-relative file path (`src/foo.js`).

**Key rules:**
- Use the full path from the `[waypoint] foreign:` annotation as the `-C` value
- Journal entries, learnings, and traps belong to the project they're about — don't log neb-www learnings in neb-entitlements
- If `-C` fails with "no .waypoint/ directory", the foreign project hasn't been scanned yet — run `waypoint scan` from that repo first

## Token Discipline

- Never re-read a file already read this session unless it was modified since.
- Prefer map descriptions over full file reads when possible.
- Use `waypoint sketch` to check a symbol's signature before reading its full file.
- Prefer targeted Grep over full file reads when searching for specific code.
- If appending to a file, do not read the entire file first.

## Session End

All writes are batched here. Before ending or when asked to wrap up:

1. **Traps:** Log every bug you fixed or error you encountered (search first to avoid duplicates).
2. **Learnings:** Log anything you discovered about the project — conventions, quirks, connections.
3. **Journal:** Update `preferences` if the user corrected your approach or expressed a preference. Update `do-not-repeat` if you made a mistake or discovered a gotcha.
4. If nothing happened worth logging, that's fine — not every session produces writes.
