# Waypoint

Project intelligence for Claude Code — hooks, file map, symbol index, ledger.

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
- `map_index.db` also contains a `symbols` table (structured symbol data from tree-sitter), a `symbols_fts` FTS5 table for full-text search, and an `imports` table tracking cross-file import relationships. All rebuild on `waypoint scan`.
- `waypoint sketch <name>` queries symbols by exact name; `waypoint find "<query>"` uses FTS5 with LIKE fallback; `waypoint callers <name>` queries the imports table joined against symbols to find all files importing a given symbol.
- `waypoint scan --all [PATH]` discovers immediate child git repos and scans each. Initializes `.waypoint/` if missing. Smart default: from inside a project, walks up to parent and scans siblings.
- `atomic_write_with(path, |writer| ...)` in `project.rs` — use this for all file writes that need crash safety. The closure receives `&mut BufWriter<File>`.
- SQLite integers must be `i64`, not `usize` — rusqlite 0.39 dropped `FromSql` for `usize`.

## Versioning

- SemVer in `Cargo.toml`, git short hash embedded at build time via `build.rs`
- `waypoint --version` prints `waypoint <semver> (<git-short-hash>)`
- **One version bump per feature branch.** Bump in the first commit that adds or changes functionality. If the branch already has a bump (check `git diff main -- Cargo.toml`), don't bump again
- Bump minor (`0.x.0`) for new features or breaking changes. Bump patch (`0.0.x`) for bugfixes only
- After bumping, run `cargo build` to update `Cargo.lock` — commit both together or `Cargo.lock` will be stale

## Conventions

- Conventional commits: `type(scope): message`
- Unit tests: `#[cfg(test)] mod tests` at the bottom of each source file
- Integration tests: `tests/` directory, using `assert_cmd` + `predicates`
- Benchmarks: `divan` with `args = [1000, 3000, 5000, 9000]` for scale testing
