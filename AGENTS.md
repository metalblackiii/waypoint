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

## Plugin Setup

See `SETUP.md` for installation and plugin registration (`./setup-plugins.sh`).

## Architecture

- `map.md` is the human-readable source of truth. `map_index.db` is a SQLite cache for O(1) lookups — it can be deleted and will rebuild on next `waypoint scan`.
- `map_index.db` tables: `map_entries` (file descriptions), `symbols` (tree-sitter), `symbols_fts` (FTS5), `imports` (cross-file relationships), `calls` (same-file call edges). All rebuild on `waypoint scan`; `calls` also updates incrementally via the `PostToolUse:Edit|Write` hook.
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

## Checklists

### New Command

1. Add CLI variant in `src/cli.rs` and dispatch in `src/lib.rs`
2. Update `COMMAND_DIGEST` in `src/hook/session_start.rs`
3. Add integration tests in `tests/integration.rs`
4. Update `WAYPOINT.md` (command table)
5. Update `SETUP.md` if the command is useful for setup verification
6. Document in Architecture above only if it introduces new data model concepts

### New Hook

1. Add hook implementation in `src/hook/`
2. Add hook script in `plugins/waypoint/hooks/`
3. Register in `plugins/waypoint/hooks/hooks.json`
4. Update `SETUP.md` (manual hook section + hook list)
5. Bump version — a new hook type is a minor feature
