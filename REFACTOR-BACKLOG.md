# Refactor Backlog

Follow-up items identified during repo-wide simplify review (2026-03-21).
Each requires separate planning — not quick fixes.

## 1. HookContext — extract shared hook preamble

**Smell:** Duplicated Code (Rule of Three met)
**Files:** `src/hook/{pre_read,pre_write,post_write}.rs`

All three hooks share an identical 11-line preamble: read stdin, extract
file_path/cwd, find project root, derive waypoint dir. `session_start` is
a partial fourth.

**Proposal:** Extract a `HookContext` struct in `hook/mod.rs` with a
`from_stdin()` constructor. Each hook's `run()` becomes context setup +
domain logic.

**Considerations:**
- `session_start` uses `ensure_initialized` instead of `waypoint_dir`
- The wp_dir-exists guard (3 occurrences) could fold into the constructor
- Needs tests for missing/malformed stdin payloads

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

## 4. Hook event name enums

**Smell:** Stringly-typed code (12+ occurrences)
**Files:** All `src/hook/*.rs`, `src/hook/mod.rs`

`"PreToolUse"` and `"PostToolUse"` appear as raw string literals in 12+
locations. `emit_hook_output` also accepts `permission: Option<&str>` where
only `"allow"`, `"deny"`, `"ask"`, or `None` are valid.

**Proposal:** Introduce `HookEventName` and `PermissionDecision` enums
with `as_str()` methods. Makes invalid values unrepresentable.

**Considerations:**
- Changes the internal API surface of all hook files
- JSON serialization must produce the same wire format
- Could combine with the HookContext refactor (#1)

---

## 5. `resolve_project_root` error propagation

**Smell:** Silent failure
**File:** `src/lib.rs:141-143`

`std::env::current_dir().unwrap_or_default()` silently falls back to an
empty `PathBuf` if the working directory is inaccessible. Downstream code
will fail confusingly.

**Proposal:** Change `resolve_project_root` to return
`Result<PathBuf, AppError>` and surface a clear error message.

**Considerations:**
- Touches every `Command` arm in `run()` (6 call sites)
- Needs a new `AppError` variant or use of the existing `Io` variant
- Should also audit the hook equivalents (they use `unwrap_or(".")`)

---

## 6. Feature Envy — GainStats display

**Smell:** Feature Envy (2 locations format the same struct)
**Files:** `src/lib.rs` (Command::Gain arm), `src/status.rs`

Both reach into `GainStats` fields to format display output. The
formatting logic belongs closer to the data.

**Proposal:** Implement `Display` for `GainStats` (verbose output) and
add `GainStats::summary_line()` (one-liner for status). Low priority.

---

## 7. `check_staleness` returns String instead of structured type

**Smell:** Primitive Obsession
**File:** `src/map/mod.rs`

Returns empty string for "up to date", human-readable string for "stale".
Call site checks `.is_empty()` as a boolean signal.

**Proposal:** Return a `StalenessReport { added, removed, modified }`
struct with `is_stale()` and `Display`. Low priority.
