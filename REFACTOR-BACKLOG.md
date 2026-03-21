# Refactor Backlog

Follow-up items identified during repo-wide simplify review (2026-03-21).
Each requires separate planning — not quick fixes.

## ~~1. HookContext — extract shared hook preamble~~ (done 2026-03-21)

Extracted `HookContext` struct with `from_stdin()` and `relative_path()` in
`hook/mod.rs`. All four hooks (`pre_read`, `pre_write`, `post_write`,
`session_start`) refactored. 6 unit tests added. Visibility narrowed to
`pub(crate)`. Peer-reviewed clean.

---

## 2. Atomic write helper

**Smell:** Duplicated Code (Rule of Three met)
**Files:** `src/trap.rs`, `src/journal.rs`, `src/map/mod.rs`

All three perform temp-file-then-rename atomic writes. The pattern is
identical but the serialization differs (JSON pretty, raw string, BufWriter
markdown).

**Proposal:** Extract `atomic_write(path, content)` into `project.rs`.
For `map/mod.rs` which uses `BufWriter`, consider an
`atomic_write_with(path, |writer| ...)` callback variant.

---

## 3. Map storage format for O(1) lookup

**Smell:** Performance — full parse for single lookup
**Files:** `src/map/mod.rs`, `src/hook/pre_read.rs`

`pre_read` hook parses the entire `map.md` into a `Vec<MapEntry>`, then
does a linear scan to find one entry. `update_entry` (called from
`post_write`) does a full read-modify-write cycle.

**Proposal:** Consider a binary index (SQLite or simple key-value file)
alongside `map.md` for O(1) lookups. Keep `map.md` as the
human-readable source of truth; the index is a cache.

**Tradeoffs:**
- Added complexity (two representations to keep in sync)
- Only matters at scale (5000+ files)
- Current latency is acceptable for projects under ~500 files

---

## ~~4. Hook event name enums~~ (done 2026-03-21)

Introduced `HookEvent` and `PermissionDecision` enums in `hook/mod.rs` with
`as_str()` methods. `emit_hook_output` now takes typed parameters instead of
`&str`. All 12+ call sites across 5 hook files updated. Wire format unchanged.
`emit_hook_output` narrowed to `pub(crate)`.

---

## ~~5. `resolve_project_root` error propagation~~ (done 2026-03-21)

Changed `resolve_project_root()` to return `Result<PathBuf, AppError>`.
All 6 call sites propagate via `?`. Uses existing `AppError::Io` variant.
Hook equivalents left as-is — they get cwd from stdin payload, not env.

---

## ~~6. Feature Envy — GainStats display~~ (done 2026-03-21)

Implemented `Display` for `GainStats` with RTK-style rich output: Unicode
box-drawing, `colored` crate for terminal colors, human-readable token
counts (1.0M/250.0K), hit rate meter bar with threshold colors, and daily
breakdown table with impact bars. Added `summary_line()` for status display.
Both `lib.rs` (Command::Gain) and `status.rs` now delegate to GainStats.
9 new unit tests for helpers and display.

---

## ~~7. `check_staleness` returns String instead of structured type~~ (done 2026-03-21)

Replaced String return with `StalenessReport { added, removed, modified }`
struct. Added `is_stale()` predicate and `Display` impl. Call site in
`lib.rs` uses `report.is_stale()` and `{report}` formatting. Existing
tests updated to assert on struct fields and Display output.
