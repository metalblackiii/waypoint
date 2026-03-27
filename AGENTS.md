# Waypoint

Project intelligence for Claude Code — hooks, file map, journal, traps, ledger.

## Non-Negotiables

- **Never use `.unwrap()` or `.expect()`** — clippy denies both. Use `?` propagation or return `Result`. The one exception: `re()` helper in `extract.rs` for compile-time-constant regex patterns.
- **Never hand-format Rust code** — `cargo fmt` runs automatically via PostToolUse hook on `.rs` edits. Let it be authoritative.
- Hooks must never set `permissionDecision` — advisory only, use `None` to defer to the agent's permission system.

## Build / Test / Validate

- `cargo clippy --all-targets` — must be clean (pedantic warnings + deny all)
- `cargo test` — unit tests in each source file, integration tests in `tests/`
- `cargo bench` — divan benchmarks in `benches/hook_latency.rs`
- CI runs fmt check, clippy, and test in parallel on ubuntu-latest

## Architecture

- `map.md` is the human-readable source of truth. `map_index.db` is a SQLite cache for O(1) lookups — it can be deleted and will rebuild on next `waypoint scan`.
- `map_index.db` also contains a `symbols` table (structured symbol data from tree-sitter) and a `symbols_fts` FTS5 table for full-text search. Both rebuild on `waypoint scan`.
- `waypoint sketch <name>` queries symbols by exact name; `waypoint find "<query>"` uses FTS5 with LIKE fallback.
- `waypoint scan --all [PATH]` discovers immediate child git repos and scans each. Initializes `.waypoint/` if missing. Smart default: from inside a project, walks up to parent and scans siblings.
- `atomic_write_with(path, |writer| ...)` in `project.rs` — use this for all file writes that need crash safety. The closure receives `&mut BufWriter<File>`.
- SQLite integers must be `i64`, not `usize` — rusqlite 0.39 dropped `FromSql` for `usize`.

## Conventions

- Conventional commits: `type(scope): message`
- Unit tests: `#[cfg(test)] mod tests` at the bottom of each source file
- Integration tests: `tests/` directory, using `assert_cmd` + `predicates`
- Benchmarks: `divan` with `args = [1000, 3000, 5000, 9000]` for scale testing
