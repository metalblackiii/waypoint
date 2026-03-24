# Waypoint Operating Protocol

You are working in a Waypoint-managed project. These rules apply every turn.

## File Navigation

1. Check the `[waypoint] map:` context injected on every Read — it has a description and token estimate for the file.
2. If the description is sufficient for your task, do NOT read the full file.
3. If a file is not in the map, search with Grep/Glob. The post-write hook will add it automatically when you create or edit it.
4. Use `waypoint scan --check` to detect stale map entries.

## Code Generation

1. Respect every entry in the Waypoint journal (injected at session start). If session context was compressed, re-read `.waypoint/journal.md`.
2. Check the `## Do-Not-Repeat` section — these are past mistakes that must not recur.
3. Follow all conventions in `## Learnings` and `## Preferences`.

## After Actions

1. After renaming or deleting files: run `waypoint scan` to update the map. (Edits and creates are handled automatically by the post-write hook.)
2. After fixing a bug: log it with `waypoint trap log` (see Bug Logging below).
3. After learning something new about the project: log it with `waypoint journal add` (see Journal Learning below).

## Journal Learning (MANDATORY — every session)

Waypoint's value comes from learning across sessions. You MUST update the journal whenever you learn something useful. This is not optional.

**Update `preferences` when the user:**
- Corrects your approach ("no, do it this way instead")
- Expresses a style preference (naming, structure, formatting)
- Shows a preferred workflow or tool choice
- Rejects a suggestion — record what they preferred instead
- Asks for more/less detail, verbosity, explanation

**Update `learnings` when you discover:**
- A project convention not obvious from the code (e.g., "tests go in __tests__/ not test/")
- A framework-specific pattern this project uses
- An API behavior that surprised you
- A dependency quirk or version constraint
- How modules connect or data flows through the system

**Update `do-not-repeat` (with date) when:**
- The user corrects a mistake you made
- You try something that fails and find the right approach
- You discover a gotcha that would trip up a fresh session

```sh
waypoint journal add --section <preferences|learnings|do-not-repeat> "<entry>"
```

**The bar is LOW.** If in doubt, add it. A journal entry that's slightly redundant costs nothing. A missing entry means the next session repeats the same discovery process.

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

**After fixing:** ALWAYS log the trap.

```sh
waypoint trap log \
  --error "<exact error or complaint>" \
  --file "<file that was fixed>" \
  --cause "<why it broke>" \
  --fix "<what you changed>" \
  --tags "<relevant,keywords>"
```

**The threshold is LOW.** When in doubt, log it. A false positive costs nothing. A missed trap means repeating the same fix later.

## Symbol Index (optional)

After `waypoint scan`, a symbol index is available in `map_index.db` alongside the file map. Use these commands when you need structural understanding without reading full files.

- `waypoint sketch <name>` — show file location and signature for a symbol (function, struct, class, etc.). Useful before deciding whether to read a file.
- `waypoint find "<query>"` — full-text search across all indexed symbols. Finds code by name or intent without grepping the codebase.

These are **optional** — the file map and Grep/Glob remain the primary navigation tools. Use sketch/find when you need to understand code structure or locate symbols across the project.

## Token Discipline

- Never re-read a file already read this session unless it was modified since.
- Prefer map descriptions over full file reads when possible.
- Use `waypoint sketch` to check a symbol's signature before reading its full file.
- Prefer targeted Grep over full file reads when searching for specific code.
- If appending to a file, do not read the entire file first.

## Session End

Before ending or when asked to wrap up:

1. Review the session: did you learn anything? Did the user correct you? Did you fix a bug?
2. If yes, update the journal and/or trap log.
