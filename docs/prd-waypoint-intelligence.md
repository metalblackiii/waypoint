# PRD: Waypoint Intelligence — Ranked Search, Architecture Context, Impact Analysis

Status: Draft
Author: Martin Burch
Date: 2026-04-18
ID: waypoint-intelligence
Branch: main
Repos: waypoint

## Problem & Outcome

AI agents waste turns exploring unfamiliar repos and lack pre-commit safety nets. Waypoint's current capabilities (map, sketch, find, callers) handle navigation but don't provide structural ranking, project-level orientation, or change impact analysis. Three targeted enhancements — selected from a gap analysis against codebase-memory-mcp — close these gaps without adopting an MCP server dependency.

Desired outcome: agents orient faster in unfamiliar repos (arch injection), find the most important symbols first (ranked find), and assess blast radius before committing (impact). Fewer wasted turns + safer commits.

## Repositories

| Repo | Role | Changes |
|------|------|---------|
| `waypoint` | Primary implementation | Rust source changes: extract, index, scan, cli, hooks, ledger, Cargo.toml |
| `dotfiles` | Operational impact (no code) | AGENTS.md Waypoint section: add advisory impact rule (2-3 lines) |

## Scope

### In Scope
1. Improve `waypoint find` ranking with structural importance weighting (new default behavior)
2. Compute architecture summary during `waypoint scan`, cache in SQLite, inject via session-start hook
3. New `waypoint impact` command mapping git diffs to affected symbols and importer blast radius
4. Add new ledger event kinds for arch context tracking (`ArchHit`, `ArchMiss`)
5. Bump version from 0.7.0 → 0.8.0
6. Update WAYPOINT.md with usage guidance for impact
7. Update AGENTS.md Waypoint section with advisory impact rule (2-3 lines)

### Out of Scope
1. Call graph / trace — staleness problem, no Codex hook support
2. Dead code detection — depends on call graph
3. New tree-sitter language grammars
4. Codex-specific hooks or configuration
5. New CLI subcommands beyond `impact` (arch is hook-injected, find is behavioral)
6. Hook-based auto-triggering of impact (manual command only)
7. JSON output format for impact (v1 is human/agent-readable text only)
8. Cross-service HTTP tracing, Cypher queries, community detection, ADR persistence
9. Gain stats reset — existing data remains valid; new event kinds track new features separately
10. `git2` crate — shell out to `git` instead (lighter, simpler, matches CLI behavior exactly)

## Requirements

### Functional

#### Feature 1: Ranked Find

- **FR-1: Two-phase ranking** — When `waypoint find` executes, the system shall first retrieve FTS5 BM25 candidates, then re-rank in Rust using structural weights: import fan-in (primary), export status (secondary), symbol kind (tertiary).
- **FR-2: Import fan-in weighting** — When re-ranking find results, the system shall rank symbols higher when they have more inbound imports (more importers = more structurally important). When fan-in data is sparse (most symbols at 0-1 importers), ranking gracefully degrades to current BM25 behavior.
- **FR-3: Export status weighting** — When re-ranking find results, the system shall rank exported symbols higher than non-exported symbols at equal import fan-in.
- **FR-4: Symbol kind weighting** — When re-ranking find results, the system shall weight functions and types higher than variables and constants at equal structural metrics.
- **FR-5: Backward-compatible output** — When displaying ranked results, the system shall use the same output format as current `waypoint find` (kind, name, signature, file, line).
- **FR-6: Both query paths** — When re-ranking, the system shall apply structural weights to both the FTS5 path and the LIKE fallback path.

#### Feature 2: Architecture Context Injection

- **FR-7: Arch computation at scan** — When `waypoint scan` runs, the system shall compute and cache: language distribution by file count (top 4 languages as percentages) and top directories by import fan-in (top 2-3 directories with inbound import counts).
- **FR-8: Arch table in SQLite** — When scan completes, the system shall persist arch summary data in a new `arch_summary` table (or equivalent), replacing any previous entry for the project.
- **FR-9: Session-start injection** — When the session-start hook fires, the system shall emit arch summary as JSON `hookSpecificOutput` with `additionalContext` containing `[waypoint] arch:` context lines, if the project has ≥20 scannable files.
- **FR-10: File count gating** — While a project has fewer than 20 scannable files, the session-start hook shall NOT emit arch context (map is sufficient for small projects).
- **FR-11: Two-line format** — When emitting arch context, the system shall produce exactly 2 lines totaling ~30 tokens:
  - Line 1: `[waypoint] arch: <Lang1> <N>%, <Lang2> <N>%, <Lang3> <N>%, <Lang4> <N>%`
  - Line 2: `[waypoint] arch: hotspots: <dir1>/ (<N> imports-in), <dir2>/ (<N> imports-in)`
- **FR-12: SessionStart HookEvent** — The system shall add a `SessionStart` variant to the `HookEvent` enum and use `emit_hook_output()` for context emission (same JSON protocol as PreToolUse).
- **FR-13: Arch ledger events** — When arch context is emitted, the system shall record an `ArchHit` event in the ledger. When arch context is suppressed (gating), the system shall record an `ArchMiss` event. These events enable separate tracking of arch savings in `waypoint gain`.

#### Feature 3: Impact Analysis

- **FR-14: Shell out to git** — The system shall invoke `git diff` via `std::process::Command` for all diff operations. No `git2` crate dependency.
- **FR-15: Auto-detect diff source** — When `waypoint impact` is invoked without arguments, the system shall check for uncommitted changes (working tree + staged vs HEAD). If uncommitted changes exist, use those. Otherwise, detect the default branch via `git symbolic-ref refs/remotes/origin/HEAD` (falling back to checking if `main` exists, then `master`, then error) and diff the current branch against it.
- **FR-16: Explicit base override** — When `waypoint impact --base <ref>` is provided, the system shall diff HEAD against the specified git ref.
- **FR-17: Diff-to-symbol mapping** — When processing a diff, the system shall map changed line ranges to symbols in the symbols table whose line ranges overlap the changed hunks.
- **FR-18: All changed symbols listed** — When reporting changes, the system shall list ALL changed symbols — both exported and non-exported. Non-exported symbols appear with 0 importers and Risk: LOW.
- **FR-19: Importer fan-out** — When a changed symbol is exported, the system shall query the imports table to find all files that import it and include them as affected.
- **FR-20: Risk classification** — When reporting affected symbols, the system shall classify risk based on importer count: CRITICAL (≥10 importers), HIGH (5-9), MEDIUM (2-4), LOW (0-1).
- **FR-21: Compact output** — When displaying results, the system shall emit one line per changed symbol: `Changed: <kind> <name> (<file>:<line>) — <N> importers | Risk: <LEVEL>`, followed by a deduplicated list of affected files.
- **FR-22: No-change exit** — When no symbols are affected by the diff, the system shall exit with code 0 and print "No symbol changes detected."
- **FR-23: Stale map warning** — When the map index is older than the most recent commit, the system shall emit a warning: "Map may be stale — run `waypoint scan` for accurate results."
- **FR-24: Non-git graceful exit** — When run in a directory with no `.git`, the system shall print "Not a git repository." and exit with code 1.

#### Feature 4: Version & Ledger

- **FR-25: Version bump** — Cargo.toml version shall be updated from `0.7.0` to `0.8.0`.
- **FR-26: New ledger event kinds** — The system shall add `ArchHit` and `ArchMiss` to the `EventKind` enum. Existing events (`SessionStart`, `MapHit`, `MapMiss`, `SketchHit`, `SketchMiss`, `FirstEdit`, `FirstEditTurns`) remain unchanged. No ledger data reset.
- **FR-27: Gain display for arch** — `waypoint gain` shall display arch hit/miss stats alongside existing map and sketch stats when arch events exist.

### Non-Functional
- **Performance**: Ranked find must not add >10ms latency vs current find (two-phase query over max 20 candidates is negligible). Arch computation adds negligible time to scan (aggregation queries on already-extracted data). Impact must complete in <2s for typical diffs (<50 files changed).
- **Binary size**: No new C dependencies. Shell out to `git` adds zero binary size.
- **Security**: N/A — local tool, no network, no secrets. Git refs passed to `std::process::Command` via `arg()` (not shell interpolation) to prevent injection.
- **Compliance**: N/A

## Codebase Context

- `src/map/index.rs:294-297` — FTS5 query in `find_symbols()`: `SELECT f.name, f.kind, f.signature, f.file_path FROM symbols_fts f WHERE symbols_fts MATCH ?1 ORDER BY f.rank LIMIT ?2`. Ranked find replaces this with a two-phase approach: FTS5 returns candidates, Rust re-ranks with structural weights.
- `src/map/index.rs:319-321` — Enrichment query already fetches `exported` flag per symbol. Ranked find extends this to also fetch fan-in count.
- `src/map/index.rs:341-346` — LIKE fallback path already sorts `ORDER BY exported DESC`. Ranked find adds fan-in weighting here too.
- `src/map/index.rs:228-242` — `find_importers()` returns `(source_file, line_number)` pairs. Impact reuses this. Ranked find needs a count variant (new query).
- `src/map/extract.rs:973-1001` — `extract_symbols()`. No changes needed — already captures exported flag and line ranges.
- `src/map/scan.rs:1-218` — Scan pipeline. Arch computation hooks in after symbol/import extraction completes.
- `src/hook/session_start.rs:12-35` — Currently emits zero stdout. Needs `emit_hook_output()` call with arch context.
- `src/hook/mod.rs:53-65` — `HookEvent` enum has only `PreToolUse`. Needs `SessionStart` variant.
- `src/hook/mod.rs:113-135` — `emit_hook_output()` — shared JSON emission function. Already works for context injection.
- `src/cli.rs:18` — Command enum. Impact adds a new variant.
- `src/ledger.rs` — `EventKind` enum. Add `ArchHit`, `ArchMiss`. Update `gain` display to show arch stats.
- `Cargo.toml:3` — Version `0.7.0` → `0.8.0`.
- `tests/integration.rs` — Integration test suite (166 tests). New features need test coverage here.

### Conventions to Follow
- Graceful degradation: if SQLite index unavailable, fall back silently (never crash hooks)
- Atomic writes: use transactions for all SQLite mutations
- Token estimation: use existing `estimate_tokens()` for any new token-aware features
- Hook output: JSON `hookSpecificOutput` format with `additionalContext` field
- Git commands: use `std::process::Command` with `.arg()` per argument (no shell interpolation)

## Acceptance Criteria

### Ranked Find
- **AC-1**: Given a repo with symbols of varying import fan-in, when `waypoint find "<query>"` matches multiple symbols, then symbols with higher import fan-in appear first in results.
- **AC-2**: Given two symbols with equal fan-in where one is exported and one is not, when both match a find query, then the exported symbol ranks higher.
- **AC-3**: Given existing find behavior, when output format is inspected, then it matches current format (kind, name, signature, file, line) — no breaking changes.
- **AC-4**: Given a repo where most symbols have 0-1 importers, when `waypoint find` runs, then results are still returned (graceful degradation to BM25 ordering).

### Architecture Context Injection
- **AC-5**: Given a project with ≥20 scannable files, when session-start hook fires, then output includes `[waypoint] arch:` lines with language percentages and top directories by import fan-in.
- **AC-6**: Given a project with <20 scannable files, when session-start hook fires, then NO `[waypoint] arch:` lines appear.
- **AC-7**: Given arch context emission, when output is measured, then it is exactly 2 lines totaling ~30 tokens.
- **AC-8**: Given a scan that completes, when the database is inspected, then arch summary data is persisted and matches computed values.
- **AC-9**: Given arch context emitted on session start, when `waypoint gain` runs, then `ArchHit` events appear in the stats.

### Impact Analysis
- **AC-10**: Given uncommitted changes to a file containing exported symbols, when `waypoint impact` runs, then output lists each changed symbol with importer count and risk tier.
- **AC-11**: Given uncommitted changes to a file containing non-exported (private) symbols, when `waypoint impact` runs, then private symbols appear with 0 importers and Risk: LOW.
- **AC-12**: Given no uncommitted changes but a branch diverged from main, when `waypoint impact` runs, then output shows branch-level symbol changes and affected importers.
- **AC-13**: Given `waypoint impact --base develop`, when run, then diff is computed against the `develop` ref specifically.
- **AC-14**: Given a changed symbol with 12 importers, when impact reports it, then risk is classified as CRITICAL.
- **AC-15**: Given no symbol changes in the diff, when `waypoint impact` runs, then it prints "No symbol changes detected." and exits 0.
- **AC-16**: Given a stale map (index older than latest commit), when `waypoint impact` runs, then a staleness warning is emitted before results.
- **AC-17**: Given a directory with no `.git`, when `waypoint impact` runs, then it prints "Not a git repository." and exits 1.

### Version & Ledger
- **AC-18**: Given the built binary, when `waypoint --version` runs, then it reports version 0.8.0.
- **AC-19**: Given arch events in the ledger, when `waypoint gain` runs, then arch hit rate is displayed alongside map and sketch rates.

## Verification

- Build: `cargo build`
- Test: `cargo test`
- Lint: `cargo clippy -- -D warnings`
- Smoke: `waypoint scan && waypoint find "scan" && waypoint impact` (against waypoint repo itself)
- Hook smoke: verify session-start hook emits `[waypoint] arch:` on a ≥20-file repo
- Gain smoke: verify `waypoint gain` displays arch stats after session-start hook fires

## Constraints

**Chosen approach**: Parallel independence — all three features touch different code paths and have no hard dependencies. Implement in any order.

- Do not add a `--ranked` flag to find — ranked behavior is the new default
- Ranked find uses a two-phase approach: FTS5 returns BM25 candidates → Rust re-ranks with structural weights. Do not attempt custom FTS5 auxiliary ranking functions.
- Do not add JSON output to impact in this iteration
- Do not add hook-based auto-triggering of impact
- Do not gate arch injection on session familiarity — file count only (<20 files = suppress)
- Arch output is exactly 2 lines: language distribution + hotspots. Do not grow it.
- Impact risk thresholds (CRITICAL ≥10, HIGH 5-9, MEDIUM 2-4, LOW 0-1) are initial values — may tune later, but ship with these
- Shell out to `git` via `std::process::Command` — do NOT add `git2` crate. Use `.arg()` per argument to prevent shell injection.
- Default branch detection: `git symbolic-ref refs/remotes/origin/HEAD` → check `main` exists → check `master` exists → error
- Impact lists ALL changed symbols (exported and non-exported). Non-exported show 0 importers / Risk: LOW.
- Impact operates on the existing symbols/imports index — do not build a separate index or cache
- No gain stats reset. New `ArchHit`/`ArchMiss` event kinds track arch savings separately from existing events.
- Keep AGENTS.md changes minimal — advisory only: "Run `waypoint impact` before committing to assess blast radius." Not a gate.
- Version bump to 0.8.0 in Cargo.toml

## Resolved Questions

Questions surfaced during req-analyst and grill rounds, now resolved:

| Question | Resolution | Rationale |
|----------|-----------|-----------|
| SessionStart `additionalContext` support? | Confirmed working | User tested previously; Codex also supports it |
| `git2` vs shell out? | Shell out to `git` | Lighter, simpler, no C dep, matches CLI behavior exactly. Dev tool always has git on PATH. |
| Default branch detection? | `origin/HEAD` → main → master → error | Canonical git approach with safe fallbacks |
| Impact staleness handling? | Warning only (FR-23) | Impact underreports when stale (conservative), never lies. Warning is sufficient. |
| Private symbols in impact output? | List all changed symbols | Complete picture. Private symbols show 0 importers / LOW risk. |
| Arch content format? | 2 lines: languages + hotspots (~30 tok) | Minimal, actionable. Agents know the stack and where gravity is. |
| Fan-in sparse in small repos? | Accept graceful degradation | Falls back to BM25 ordering. No harm, no extra code. |
| AGENTS.md impact rule philosophy? | Advisory, not gating | Agents already have self-review gates. Impact is signal, not a stop sign. |
| Gain stats reset? | No reset, add new event kinds | Existing data valid. `ArchHit`/`ArchMiss` track new feature separately. |
| Ranked find weighting configurable? | Hardcoded for v1 | Tune based on real usage. Avoid premature configurability. |
| Impact non-zero exit on high risk? | No — always exit 0 on success | Surprising behavior. CI gating can be added later if needed. |

## Open Questions

- [ ] Arch hotspot metric: use import fan-in count (importers pointing into directory) or total import edges (imports originating from + pointing into)? Fan-in is simpler and more actionable — leaning fan-in.
