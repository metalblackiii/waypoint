# PRD: Import-Scoped Reference Tracking

> Status: Draft
> Author: Martin
> Date: 2026-04-03
> ID: import-refs
> Branch: main

## Problem & Outcome

When Claude changes an exported function's signature in a JS/Python/Rust/Go project, it greps for callers, misses some, and commits broken code. In a 1,500+ file service like neb-ms-billing, this is the #1 source of multi-file breakage. There's no compiler in the JS stack to catch it.

Waypoint should extract import relationships during scan, store them in SQLite, and automatically warn Claude via the post-write hook when an exported symbol's signature changes — listing every file that imports it.

## Repositories

| Repo | Role | Changes |
|------|------|---------|
| `waypoint` | Primary implementation | Import extraction, schema, CLI command, hook warnings |

## Scope

### In Scope
1. Extract import statements from all tree-sitter-supported languages (Rust, TS, TSX, JS, JSX, MJS, CJS, Python, Go)
2. Store imports in a new SQLite table with source file, imported name, and resolved target file
3. Resolve relative import paths to actual files (including implicit `index.js`/`index.ts` resolution)
4. `waypoint callers <symbol>` CLI command — query importers of a symbol
5. Post-write hook signature-change detection — diff old vs new exported symbol signatures
6. Post-write hook caller warning — emit `[waypoint] signature changed: N importers` when signatures change
7. Incremental import updates via post-write hook (single file re-extraction on edit)
8. Version bump to 0.5.0
9. Tests and benchmarks for all new code paths

### Out of Scope
1. Package/node_modules resolution (`@neb/microservice`, `lodash`, etc.) — only relative paths
2. tsconfig path aliases (`@/utils`, `~/components`)
3. Dynamic imports (`import()`, `require(variable)`)
4. Wildcard imports (`import * as foo`) — only named imports
5. Re-export chain following (`export { foo } from './bar'` treated as terminal)
6. Cross-repo reference tracking
7. Barrel file detection or special handling
8. Call-site extraction (tracking every `foo()` call expression)
9. Filesystem-routed controller files (loaded by framework convention, not imported)

## Requirements

### Functional

- **FR-1: Import extraction** — When `waypoint scan` processes a file with a supported extension, the system shall extract all static import statements and store them in the imports table with source file, imported symbol name(s), and raw import path.
- **FR-2: Import path resolution** — When an import uses a relative path (`./`, `../`), the system shall resolve it to the actual file path, including implicit index file resolution (`./utils` → `./utils/index.js`).
- **FR-3: Aliased import tracking** — When an import uses aliasing (`import { foo as bar }`), the system shall store the original name (`foo`), not the alias.
- **FR-4: Incremental import updates** — When the post-write hook processes a changed file, the system shall re-extract that file's imports and update the imports table (delete old, insert new).
- **FR-5: Callers command** — When a user runs `waypoint callers <symbol>`, the system shall query the imports table and symbols table to return all files that import the named symbol, with file paths and line numbers.
- **FR-6: Signature change detection** — When the post-write hook processes a file, the system shall compare the old exported symbol signatures (queried before update) with the new signatures (after re-extraction) and identify changes.
- **FR-7: Caller warning emission** — When an exported symbol's signature changes, the system shall query the imports table for all files importing that symbol from the changed file and emit a hook warning listing them.
- **FR-8: Test file inclusion** — When extracting imports, test files shall be included as importers so that signature changes surface broken tests.
- **FR-9: Common name filtering** — When emitting caller warnings, the system shall skip built-in/common names (`new`, `from`, `toString`, `map`, `filter`, `reduce`, `then`, `catch`) to reduce noise.

### Non-Functional
- **Performance**: Post-write hook latency must remain under 5ms. Import extraction during full scan may add ~30-40% to scan time but should not change the order of magnitude.
- **Performance**: The imports table for a 1,500-file project should be under 15K rows. SQLite handles this trivially.
- **Reliability**: Missing some callers is acceptable (advisory, not a guarantee). False negatives are OK; false positives (warning about files that don't actually call the symbol) should be minimized.
- **Compatibility**: Existing scan, sketch, find, trap, and map.md behavior must not change.

## Codebase Context

### Import Extraction Target (extract.rs)
- `src/map/extract.rs` (1,874 LOC) — Multi-language tree-sitter symbol extraction. Per-language handlers: `collect_rust_symbols()`, `collect_js_symbols()`, `collect_python_symbols()`, `collect_go_symbols()`. New `extract_imports()` function follows the same pattern — one handler per language, same AST walk infrastructure.
- JS/TS import node types: `import_statement` (source field has the path), `import_clause` children have named/default bindings
- Python: `import_from_statement` (module_name + name children)
- Rust: `use_declaration` (scoped_identifier or use_list children)
- Go: `import_spec` (path field)

### Schema Extension (index.rs)
- `src/map/index.rs` (594 LOC) — SQLite schema + CRUD. New `imports` table alongside existing `map_entries`, `symbols`, `symbols_fts`. Pattern matches `update_file_symbols()` for incremental updates.
- Existing `symbols` table already has `exported` flag — used to filter which signature changes trigger warnings.

### Scan Pipeline (scan.rs)
- `src/map/scan.rs` (198 LOC) — File walk + extraction. `extract_imports()` wired alongside `extract_symbols()` in the per-file loop. New field on `ScanOutput`.

### Hook Integration (post_write.rs)
- `src/hook/post_write.rs` (264 LOC) — Already does: read file, extract symbols, update index. New: query old symbols before delete, compare signatures after re-extraction, query importers if changed.
- `update_file_symbols()` currently does DELETE then INSERT. Signature diff requires querying old symbols *before* the delete.

### CLI (cli.rs, lib.rs)
- `src/cli.rs` (128 LOC) — Clap command enum. New `Callers` variant with `symbol` arg and optional `context`.
- `src/lib.rs` (389 LOC) — Command dispatch. Pattern matches `Sketch`/`Find` commands.

### Real-World Import Patterns (neb stack)
- ESM source: `import { named } from './relative/path.js'`
- CJS in some tests: `const { foo } = require('./bar')`
- Facade index.js files in neb-ms-billing: `import foo from './foo'; export { foo }` — these are two-step, not `export from`, and are a small minority
- No `export * from` anywhere in the neb codebase
- No barrel file convention
- No tsconfig path aliases in use

### Tree-Sitter Node Types Per Language

**JavaScript/TypeScript (`import_statement`):**
```
(import_statement
  (import_clause
    (named_imports
      (import_specifier name: (identifier) alias: (identifier)?)))
  source: (string))
```

**Python (`import_from_statement`):**
```
(import_from_statement
  module_name: (dotted_name)
  name: (dotted_name))
```

**Rust (`use_declaration`):**
```
(use_declaration
  argument: (scoped_identifier | use_list | scoped_use_list))
```

**Go (`import_spec`):**
```
(import_declaration
  (import_spec
    name: (package_identifier)?
    path: (interpreted_string_literal)))
```

## Acceptance Criteria

- **AC-1**: Given a JS file with `import { foo, bar } from './utils.js'`, when `waypoint scan` runs, then the imports table contains two rows: `(file, "foo", "utils.js")` and `(file, "bar", "utils.js")`.
- **AC-2**: Given a JS file with `import { foo as renamed } from './utils.js'`, when `waypoint scan` runs, then the imports table contains `(file, "foo", "utils.js")` — original name, not alias.
- **AC-3**: Given a Python file with `from .utils import helper`, when `waypoint scan` runs, then the imports table contains `(file, "helper", resolved_path)`.
- **AC-4**: Given a Rust file with `use crate::map::scan::ScanOutput`, when `waypoint scan` runs, then the imports table contains `(file, "ScanOutput", resolved_path)`.
- **AC-5**: Given files A and B where A imports `process_data` from B, when running `waypoint callers process_data`, then file A appears in the output with its path.
- **AC-6**: Given file B exports `pub fn process_data(x: i32)` and file A imports it, when B is edited to change the signature to `pub fn process_data(x: i32, y: i32)`, then the post-write hook emits `[waypoint] signature changed for process_data: 1 importer — A`.
- **AC-7**: Given a file with `import default from './foo.js'`, when `waypoint scan` runs, then the imports table contains `(file, "default", "foo.js")`.
- **AC-8**: Given a file with `const { foo } = require('./bar')`, when `waypoint scan` runs, then the imports table contains `(file, "foo", "bar")`.
- **AC-9**: Given file B has a private (non-exported) function whose signature changes, when B is edited, then the post-write hook does NOT emit a caller warning.
- **AC-10**: Given the post-write hook detects a signature change, the total hook execution time remains under 5ms (verified via bench).
- **AC-11**: Given existing `waypoint scan`, `sketch`, `find`, and `trap` commands, when import tracking is added, then all existing tests continue to pass with no behavior changes.

## Verification

- Build: `cargo build`
- Lint: `cargo clippy --all-targets`
- Test: `cargo test`
- Bench: `cargo bench` (hook latency in `benches/hook_latency.rs`)
- Format: `cargo fmt --check`

## Constraints

**Chosen approach**: Hybrid flat imports (Approach C) — flat `imports` table with string columns, no foreign keys to symbols. Join at query time. Leaves upgrade path to relational linking without migration. Chosen because the hook warning value is the same either way, and this ships faster with simpler incremental updates.

**Patterns to follow:**
- Per-language extraction handlers in `extract.rs` — one function per language, same as `collect_*_symbols()`
- SQLite table pattern in `index.rs` — `CREATE TABLE IF NOT EXISTS`, `open_index()`, transacted rebuilds
- Incremental update pattern — `delete WHERE file_path = ?` then insert, matching `update_file_symbols()`
- Hook output format — `[waypoint] ...` prefix, emitted via `emit_hook_output()`
- Test pattern — `#[cfg(test)] mod tests` at bottom of each file, `tempfile::TempDir` for isolation

**Things NOT to do:**
- Do not add foreign keys from imports to symbols — keep the table flat
- Do not resolve package imports (`@neb/*`, `lodash`) — relative paths only
- Do not follow re-export chains
- Do not extract call expressions — only import statements
- Do not change map.md format
- Do not change existing hook response structure
- Do not add new tree-sitter grammar dependencies — use existing 6 language parsers
- Never use `.unwrap()` or `.expect()` — `?` propagation or `Result` return

**Import table schema:**
```sql
CREATE TABLE IF NOT EXISTS imports (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_file TEXT NOT NULL,
    imported_name TEXT NOT NULL,
    target_path TEXT NOT NULL,
    raw_path TEXT NOT NULL,
    line_number INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_imports_source ON imports(source_file);
CREATE INDEX IF NOT EXISTS idx_imports_target ON imports(target_path);
CREATE INDEX IF NOT EXISTS idx_imports_name ON imports(imported_name);
```

**Import struct:**
```rust
pub struct Import {
    pub source_file: String,
    pub imported_name: String,
    pub target_path: String,    // resolved relative path
    pub raw_path: String,       // original import string
    pub line_number: i64,
}
```

## Phases

### Phase 1: Import extraction + storage
- Add `extract_imports()` to `extract.rs` with per-language handlers
- Add `imports` table to `index.rs` with `rebuild_imports()` and `update_file_imports()`
- Wire into `scan.rs` pipeline — new field on `ScanOutput`
- Wire into `post_write.rs` for incremental updates
- Unit tests for each language's import extraction
- Integration test: scan a project, verify imports table populated

### Phase 2: Callers command
- Add `Callers` variant to CLI enum in `cli.rs`
- Add `find_importers()` query to `index.rs` — join imports and symbols tables
- Dispatch in `lib.rs`
- Format output matching `sketch`/`find` style
- Integration test: scan + callers query

### Phase 3: Signature-change warnings
- In `post_write.rs`: query old exported symbols before delete
- Compare old vs new signatures after re-extraction
- Query importers for changed symbols
- Emit `[waypoint] signature changed for <name>: N importers — file1, file2, ...`
- Filter out common names (FR-9)
- Unit test for signature diff logic
- Unit test for warning emission
- Bench: verify hook latency stays under 5ms

### Phase 4: Version bump
- Bump `Cargo.toml` to 0.5.0
- `cargo build` to update `Cargo.lock`
- Update WAYPOINT.md if any protocol changes needed

## Open Questions

- [x] **Callers line numbers:** Yes — `waypoint callers` shows the line number of each import statement. Helps Claude navigate directly.
- [x] **Truncation:** Yes — show first 5 importers with count, then `... and N more`. Hint `→ run: waypoint callers <symbol>` for the full list. Keeps hook output bounded; Claude can query on demand.
- [x] **CJS require:** Top-level destructuring only (`const { foo } = require('./bar')`). No property access tracking (`utils.foo`).
- [x] **Go imports:** Extract import path for the `imports` table (so `waypoint callers` finds them), but skip signature-change warnings for Go. Go's package-level imports don't map to the per-symbol model. Revisit if Go enters the stack.
