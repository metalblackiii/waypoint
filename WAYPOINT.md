# Waypoint Operating Protocol

You are working in a Waypoint-managed project. These rules apply every turn.

## File Navigation

1. Check the `[waypoint] map:` context injected on every Read — it has a description and token estimate for the file.
2. If the description is sufficient for your task, do NOT read the full file. The map description answering the question is enough — no sketch, no read.
3. When you need a specific symbol (function, class, type), use `waypoint sketch <name>` before reading the file — it gives you the signature and line range. **Skip sketch for files under ~150 tokens** (check the map annotation — roughly 10-15 lines of code). **For files over ~200 tokens, sketch is mandatory unless the file was already read this session.**
4. `waypoint find` vs `Grep` — use the right tool for the job:
   - `waypoint find "<query>"` — symbol names, function signatures, struct/class definitions
   - `Grep` — string literals, comments, config values, error messages, non-code text
5. When changing an exported function's signature, run `waypoint callers <name>` to find all files that import it.

## After Actions

Map freshness is maintained by the session-start hook, which rescans automatically when the map is older than 7 days or file count has drifted more than 3%. Content-only edits (same file count) do not trigger an automatic rescan until the next session. For mid-session freshness after significant edits, run `waypoint scan` manually.

Same-directory renames are auto-cleaned on next scan. Cross-directory moves may leave a stale entry — run `waypoint scan` if you move a file to a different directory.

## Cross-Project Work

Hooks resolve foreign projects automatically. Watch for `[waypoint] foreign: /path/to/other-repo` annotations on pre-read.

## Token Discipline

- Never re-read a file already read this session unless it was modified since.
- If appending to a file, do not read the entire file first.

## Session End

No batched writes required — the session-start hook handles index freshness on the next session. Note: session-start only rescans when the map is stale or file count drifted; content-only edits (same file count) won't update descriptions until you run `waypoint scan` manually.
