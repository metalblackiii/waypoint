# PRD: Structured Learnings Store

> Status: Ready
> Author: Martin Burch
> Date: 2026-03-27
> ID: structured-learnings
> Branch: main

## Problem & Outcome

The journal injects all learnings into session context at startup, burning tokens on entries that are mostly irrelevant to the current task. As learnings accumulate, the signal-to-noise ratio drops and context cost grows linearly. The desired outcome is contextual surfacing — learnings appear only when relevant to the file being read, matching the pattern that already works well for traps.

## Repositories

| Repo | Role | Changes |
|------|------|---------|
| `waypoint` | Primary implementation | New learning module, CLI commands, hook changes, journal slimming |

## Scope

### In Scope
1. New `waypoint learning` CLI with `add`, `search`, `list`, `prune` subcommands
2. `learnings.json` structured store with tag-based entries
3. Pre-read hook surfaces learnings matching the file being read (tag prefix match)
4. Session-start hook stops injecting learnings; updates invocation prompt
5. Remove `JournalSection::Learnings` variant; journal retains only preferences + do-not-repeat
6. `waypoint trap prune` subcommand (same UX pattern as learning prune)
7. `--all` flag on prune commands for batch multi-project pruning (same discovery as `scan --all`)
8. Cross-project support (`-C` flag) on learning commands

### Out of Scope
1. Migration of existing journal learnings to the new store — clean break, manual cleanup
2. SQLite indexing for learnings — JSON-only, reassess if entry counts grow large
3. Shared abstraction between traps and learnings — Rule of Three not met
4. Changes to trap log/search behavior (only adding prune)

## Requirements

### Functional

**Learning Store:**
- **FR-1: Add learning** — When `waypoint learning add "<entry>" --tags "<tags>"` is invoked, the system shall append a new entry to `.waypoint/learnings.json` with a unique ID, the entry text, parsed tags, and a timestamp. The `--tags` flag is required; omitting it shall produce an error.
- **FR-2: Search learnings** — When `waypoint learning search "<query>"` is invoked, the system shall return entries matching query terms across entry text and tags, ranked by relevance (same term-matching strategy as trap search).
- **FR-3: List learnings** — When `waypoint learning list` is invoked, the system shall display all entries with their IDs, tags, and dates.
- **FR-4: Prune learnings** — When `waypoint learning prune --older-than <duration>` is invoked, the system shall delete entries older than the specified duration and print the full deleted entries to stdout (for manual restoration if needed). The `--older-than` flag is required; omitting it shall produce an error suggesting `90d` as a default. Duration format is `Nd` (days only, e.g., `90d`). If pruning removes all entries, delete `learnings.json` rather than leaving an empty `[]`.
- **FR-5: Batch prune learnings** — When `waypoint learning prune --all --older-than <duration>` is invoked, the system shall discover sibling projects (same smart default as `scan --all` — from inside a project, walks up to parent and scans siblings) and prune learnings in each. `--all` and `-C` are mutually exclusive; providing both shall produce an error.
- **FR-6: Cross-project learnings** — When `-C <path>` is provided on learning commands, the system shall resolve the target project using `resolve_with_context` (read commands use `require_waypoint_dir`, write commands use `ensure_initialized`).

**Hook Changes:**
- **FR-7: Pre-read surfaces learnings** — When the pre-read hook fires and `learnings.json` exists, the system shall load it, filter entries whose tags are a prefix of (or exact match to) the file path being read, and append matching entries to the hook output as `[waypoint] learnings for <file>: ...`. If `learnings.json` does not exist, skip silently. Directory tags must end with `/` to prevent false prefix matches (e.g., `src/hook/` matches `src/hook/pre_read.rs` but not `src/hookutils.rs`).
- **FR-8: Session-start drops learnings injection** — When the session-start hook fires, the system shall inject only journal preferences and do-not-repeat sections, not learnings.
- **FR-9: Updated invocation prompts** — The session-start hook shall display `waypoint learning add "<entry>" --tags "<tags>"` as the learning invocation prompt, replacing the learnings portion of the journal prompt.

**Journal Changes:**
- **FR-10: Remove Learnings section** — The `JournalSection` enum shall have the `Learnings` variant removed. The `empty_journal()` template shall produce only `## Preferences` and `## Do-Not-Repeat` sections.

**Protocol Updates:**
- **FR-13: WAYPOINT.md updates** — The operating protocol shall be updated to reflect the new learning commands, remove journal learnings references, and document contextual surfacing behavior (see "WAYPOINT.md Protocol Changes" section below).

**Trap Prune:**
- **FR-11: Prune traps** — When `waypoint trap prune --older-than <duration>` is invoked, the system shall delete trap entries older than the specified duration and print the full deleted entries to stdout. `--older-than` is required with the same error behavior as FR-4. Duration format is `Nd` (days only). If pruning removes all entries, delete `traps.json` rather than leaving an empty `[]`.
- **FR-12: Batch prune traps** — When `waypoint trap prune --all --older-than <duration>` is invoked, the system shall discover sibling projects (same smart default as `scan --all`) and prune traps in each. `--all` and `-C` are mutually exclusive.

### Non-Functional
- **Performance**: Pre-read hook latency must not regress. JSON read + prefix filter on a small array (<100 entries) is expected to be sub-millisecond.
- **Security**: N/A — local tool, no network, no PII in learnings.
- **Compliance**: N/A

## Codebase Context

- `src/trap.rs` — Pattern to replicate for `learning.rs`. Struct, JSON read/write, search, per-file filtering. Dedup and occurrences are trap-specific and not needed for learnings.
- `src/journal.rs` — `JournalSection` enum usage, `empty_journal()` template, `add_entry()`. Remove `Learnings` variant, update template.
- `src/cli.rs` — Add `Command::Learning` with subcommands. Remove `JournalSection::Learnings`. Add `TrapCommand::Prune`.
- `src/lib.rs` — Add `pub mod learning`, wire up command dispatch.
- `src/hook/pre_read.rs` — Insert learnings lookup after map lookup, before `emit_hook_output`. Follows the same foreign-project resolution pattern already in place.
- `src/hook/session_start.rs` — Stop injecting learnings from journal. Update invocation prompts.
- `src/hook/pre_write.rs` — Reference for how traps surface contextually (pattern to follow in pre_read for learnings).
- `src/project.rs` — `resolve_foreign()`, `resolve_with_context()`, `discover_projects()` — all reused, not modified.
- `tests/integration.rs` — Add integration tests for new CLI commands and hook behavior.

## Acceptance Criteria

- **AC-1**: Given no `learnings.json` exists, when `waypoint learning add "entry" --tags "src/hook/"` is run, then `learnings.json` is created with one entry containing the text, tags, and a timestamp.
- **AC-2**: Given learnings exist, when `waypoint learning search "hook"` is run, then matching entries are returned ranked by term hits.
- **AC-3**: Given learnings exist, when `waypoint learning list` is run, then all entries are displayed with ID, tags, and date.
- **AC-4**: Given learnings exist with varying ages, when `waypoint learning prune --older-than 90d` is run, then only entries older than 90 days are removed.
- **AC-5**: Given `--older-than` is omitted from prune, when the command runs, then it exits with an error suggesting `--older-than 90d`.
- **AC-6**: Given learnings tagged `src/hook/` exist, when the pre-read hook fires for `src/hook/pre_read.rs`, then those learnings appear in the hook output as `[waypoint] learnings for src/hook/pre_read.rs: ...`.
- **AC-7**: Given learnings tagged `src/trap.rs` exist, when the pre-read hook fires for `src/hook/pre_read.rs`, then those learnings do NOT appear (no prefix match).
- **AC-8**: Given a journal with preferences and do-not-repeat entries, when session-start fires, then those sections are injected but learnings are not.
- **AC-9**: Given `waypoint learning prune --all --older-than 60d` is run from a parent directory, then sibling projects are discovered and each is pruned.
- **AC-10**: Given `waypoint trap prune --older-than 90d` is run, then trap entries older than 90 days are removed from `traps.json`.
- **AC-11**: Given `-C /path/to/other-repo` is provided, when learning commands are run, then they operate on the foreign project's `.waypoint/` directory.
- **AC-12**: Given `JournalSection::Learnings` is removed, when `cargo clippy --all-targets` runs, then no warnings or errors reference the removed variant.
- **AC-13**: Given no `learnings.json` exists, when the pre-read hook fires, then no learnings output is emitted (silent skip).
- **AC-14**: Given learnings tagged `src/hook/` exist, when the pre-read hook fires for `src/hookutils.rs`, then those learnings do NOT appear (trailing `/` prevents false prefix match).
- **AC-15**: Given `--all` and `-C` are both provided to a prune command, then the command exits with an error.
- **AC-16**: Given all learnings are pruned, then `learnings.json` is deleted (not left as empty `[]`).
- **AC-17**: Given all traps are pruned, then `traps.json` is deleted (not left as empty `[]`).
- **AC-18**: Given `waypoint learning prune --older-than 90d` removes 3 entries, then all 3 entries are printed to stdout with their full fields before deletion.

## Verification

- Build: `cargo clippy --all-targets`
- Test: `cargo test`
- Lint: `cargo fmt --check`
- Benchmark: `cargo bench` (check pre-read hook latency)

## Constraints

**Chosen approach**: Independent module (parallel to traps) — learnings get their own `src/learning.rs` with `LearningEntry` struct and `learnings.json` file. No shared abstraction with traps; the Rule of Three is not met. Accept minor structural duplication over premature abstraction.

- Follow existing patterns: `NewTrap` → `NewLearning`, `read_traps()` → `read_learnings()`, etc.
- `learnings.json` is the sole store — no SQLite indexing. Reassess if entry counts exceed ~100.
- **Lazy-create file lifecycle**: `learnings.json` is not created on `waypoint scan` or init. It is created on first `learning add`. `read_learnings()` returns an empty vec when the file doesn't exist (same pattern as `read_traps()`). Prune deletes the file entirely when all entries are removed. Same lifecycle applies to `traps.json`.
- **Tag normalization**: Directory tags must end with `/`. The `add` command should validate/normalize this (e.g., warn or auto-append `/` for paths that look like directories). Exact file tags have no trailing slash.
- Pre-read hook must not open additional file handles beyond the JSON read — keep latency low. Skip silently when `learnings.json` does not exist.
- `--older-than` duration parsing: support `Nd` format (days only). Document clearly — no weeks/months/hours. Keep it simple.
- Prune `--all` reuses `project::discover_projects()` — same smart default as `scan --all` (from inside a project, walks up to parent and scans siblings).
- `--all` and `-C` are mutually exclusive on prune commands — produce an error if both are provided.
- Cross-project resolution follows established patterns: `require_waypoint_dir()` for read commands, `ensure_initialized()` for write commands.
- Do not modify existing trap log/search behavior — only add the prune subcommand.
- Existing `journal.md` files with `## Learnings` sections will be manually cleaned up — no code needed to strip or migrate them.

## Open Questions

All resolved:

- [x] ~~Should `waypoint learning add` without `--tags` be allowed?~~ No — `--tags` is required.
- [x] ~~Should prune report what it deleted?~~ Yes — print full deleted entries to stdout for manual restoration.
- [x] ~~WAYPOINT.md protocol updates?~~ Included in this PRD (see FR-13).
- [x] ~~Tag normalization (trailing slash)?~~ Directory tags must end with `/`. Normalize on add.
- [x] ~~`--all` + `-C` interaction?~~ Mutually exclusive — error if both provided.
- [x] ~~Prune discovery logic?~~ Same smart default as `scan --all`.
- [x] ~~Existing journal.md with `## Learnings` section?~~ Manual cleanup — no code changes to strip it.
- [x] ~~`read_journal()` changes?~~ None — returns the full file as-is.
- [x] ~~File lifecycle (lazy-create vs init)?~~ Lazy-create: file created on first add, deleted when prune empties it. Same for traps.

## WAYPOINT.md Protocol Changes

The following sections of `WAYPOINT.md` need updating:

**Replace in "Code Generation" section:**
- Remove: reference to journal learnings in session-start context
- Keep: references to preferences and do-not-repeat

**Replace in "After Actions" section:**
- Add: "After learning something new about the project: log it with `waypoint learning add`"
- Update: invocation syntax from `waypoint journal add --section learnings` to `waypoint learning add "<entry>" --tags "<tags>"`

**Replace "Journal Learning (MANDATORY)" section with two sections:**

1. **Journal (MANDATORY)** — covers preferences and do-not-repeat only. Same triggers as today, same `waypoint journal add --section <preferences|do-not-repeat>` syntax.

2. **Learnings (MANDATORY)** — new section. Triggers: discovering project conventions, framework patterns, API behaviors, dependency quirks, module connections. Syntax: `waypoint learning add "<entry>" --tags "<file-or-dir-paths>"`. Emphasize tagging with relevant file/directory paths so learnings surface contextually on pre-read. Same "bar is LOW" philosophy.

**Update "Preferred lookup order":**
- Add note that learnings are surfaced automatically on pre-read (alongside map descriptions) — no manual step needed.

**Update "Session End":**
- Replace "update the journal" with "update the journal and/or log learnings"
