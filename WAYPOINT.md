# Waypoint Operating Protocol

You are working in a Waypoint-managed project. These rules apply every turn.

## File Navigation

1. Check the `[waypoint] map:` context injected on every Read — it has a description and token estimate for the file.
2. If the description is sufficient for your task, do NOT read the full file.
3. When you need a specific symbol (function, class, type), use `waypoint sketch <name>` before reading the file — it gives you the signature and line range. **Skip sketch for files under ~150 tokens** (check the map annotation — roughly 10-15 lines of code).
4. `waypoint find` vs `Grep` — use the right tool for the job:
   - `waypoint find "<query>"` — symbol names, function signatures, struct/class definitions
   - `Grep` — string literals, comments, config values, error messages, non-code text

## After Actions

1. After renaming or deleting files: run `waypoint scan` to update the map. (Edits and creates are handled automatically by the post-write hook.)
2. Traps are **batched at session end** — note key details (error messages, file paths, root causes) inline in your responses as you go, then write them all in the Session End pass. The only inline write is `waypoint scan` for renames/deletes.

## Bug Logging (MANDATORY)

**Log a trap whenever:**
- The user reports a bug or unexpected behavior
- A test, build, lint, or type check fails
- You fix something that was broken
- You edit a file more than twice to get it right

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

When in doubt, log it — a false positive costs nothing.

## Cross-Project Work

Hooks resolve foreign projects automatically. Watch for `[waypoint] foreign: /path/to/other-repo` annotations on pre-read.

When working in a foreign project, `trap log --file` auto-resolves — no `-C` needed.

- If `-C` fails with "no .waypoint/ directory", run `waypoint scan` from that repo first

## Token Discipline

- Never re-read a file already read this session unless it was modified since.
- If appending to a file, do not read the entire file first.

## Session End

All writes are batched here. Before ending or when asked to wrap up:

1. **Traps:** Log every bug you fixed or error you encountered (search first to avoid duplicates).
2. If nothing happened worth logging, that's fine — not every session produces writes.
