# Future Features

Parked ideas with full context. Not scheduled — recorded so the reasoning survives.

## Call Graph Tracing (`waypoint trace`)

**Status**: ✅ V1 delivered in v0.13.0 (same-file calls). Cross-file trace is v2.

**V1 scope (delivered)**:
- `calls` table in SQLite: `(id, source_file, source_symbol, target_file, target_symbol, line_number)`
- `target_file` column present from day one — v2 cross-file requires no schema migration
- Extraction-time resolution: only validated same-file call edges are stored
- Call extraction via tree-sitter AST walking for Rust, JS/TS, Python, Go
- PostToolUse:Edit|Write hook keeps call data fresh incrementally (staleness blocker resolved)
- `waypoint trace <symbol> [--direction inbound|outbound|both] [--depth N]` with recursive DFS traversal
- Builtin skip lists per language (macros, builtins, framework globals)
- Method resolution: bare name `bar` matches qualified `Type::bar` via suffix matching

**V2: Cross-file trace (not yet implemented)**:

Remaining work to extend trace across file boundaries:

- Populate `target_file` by resolving callee names against the imports table at extraction time
- Relax the same-file constraint in `find_callees`/`find_callers_of` queries (parameter becomes `Option<&str>`)
- Handle ambiguous resolution (multiple files export same symbol name)
- Dynamic dispatch, trait objects, and aliased imports remain out of scope

**Estimated effort for v2**: Medium (~3-5 days). Schema and BFS infrastructure are in place; the work is in the resolver.

## Dead Code Detection (`waypoint dead`)

**Status**: Deprioritized — low ROI vs command-list cognitive overhead.

**What**: Find symbols with zero callers. `waypoint dead [--kind fn|type|all]` lists symbols nobody uses.

**Implementation sketch**:
- SQL query on `calls` table: `SELECT * FROM symbols WHERE exported = 0 AND kind IN ('function','method') AND name NOT IN (SELECT target_symbol FROM calls WHERE target_file = file_path)`
- Exclusion mechanism for entry points, test targets, framework magic
- ~100-150 LOC

**Why it's deprioritized**: With v1 same-file calls, only private dead symbols are reliably detectable — but language linters already catch these (Rust `dead_code`, TS `noUnusedLocals`). The unique-value version (exported symbols nobody imports across the codebase) requires v2 cross-file trace. Adding a command that duplicates linter output isn't worth the cognitive overhead for agents evaluating which waypoint command to use.

**Revisit when**: V2 cross-file trace lands. Exported dead code detection is the version linters can't do — that's when `waypoint dead` earns its command-list slot.

**Estimated effort**: Low (~1 day for same-file scope).

## NL Task Routing (`waypoint ask`)

**Status**: Recommended next feature — best ROI of remaining candidates.

**What**: Rank files by relevance to a natural-language task description. `waypoint ask "<task>"` returns a scored list of the most relevant files — e.g., `waypoint ask "implement OAuth middleware"` → ranked file paths with match reasons.

**Why it matters**: Waypoint is currently symbol-name-based. If you don't know what symbol to look for, you're stuck. NL task routing removes the bootstrap problem: start from intent, not from symbol names. This is the only candidate that changes the agent's workflow at the start of *every* task, not just niche scenarios. A better starting file set compounds — fewer wasted reads downstream.

**ROI assessment** (2026-05-19): Compared against `dead` (linters already cover private symbols; unique value needs v2 cross-file), `routes` (high value but neb-specific, highest effort), and `ask` (universal use case, leverages existing index). `ask` wins on frequency × uniqueness — agents hit the bootstrap problem ("I have a task but don't know which symbol to search for") on nearly every task.

**Quality risk**: Map description coverage determines ranking quality. Mitigation: ship it, gate on hit@5 against real tasks, and let the agent learn to skip it if rankings are poor. Thin descriptions → poor rankings is a data problem, not an architecture problem.

Evaluated `sigmap` (manojmallick/sigmap) as a candidate drop-in — its `sigmap ask` command does exactly this. Rejected it (NO-GO: 20 days old, sole contributor, MCP-dependent value). The capability is real; the right home is here.

**Implementation sketch**:
- Scoring pipeline per file: keyword match against map descriptions + symbol names (TF-IDF or simple token overlap), boosted by import-graph adjacency
- Graph boost: files imported by high-scoring files get a +weight on 1-hop neighbors (sigmap uses +0.4; tune empirically)
- Waypoint already has all inputs: map descriptions (per-file natural language), symbol names, import graph (used by `callers` and `impact`)
- New `ask` subcommand: tokenize query, score all indexed files, apply graph boost, return top-N with file path + score + matched terms
- Optional `--top N` flag (default 5–10)

**Implementable carry-over from sigmap evaluation**:
- Build only the NL retrieval capability (`ask`) as a native waypoint command.
- Keep it local-only and index-backed (reuse `map.md` + SQLite symbols/imports); no MCP dependency required.
- Return ranked files with compact "why matched" signals (matched terms + graph boost contribution).
- Add an evaluation harness before shipping: small task→expected-files benchmark, track hit@5 and hit@10.

**Explicit non-goals for v1**:
- No generated context artifact files (for example, `.github/copilot-instructions.md`).
- No quality-loop subcommands (`judge`, `validate`, `learn`).
- No adoption of third-party sigmap runtime or release cadence risk.

**Why it's parked**:
- Map description quality determines result quality — gaps in map coverage produce poor rankings
- No evaluation harness yet to measure hit@5 against real tasks in a target codebase
- Low urgency: `waypoint find` + `waypoint sketch` cover the common case when you know the symbol name

**What would unblock it**:
- Map coverage reaching ~80%+ of meaningful files (descriptions present and non-trivial)
- A small benchmark set of task → relevant files pairs for a target codebase to validate ranking quality before shipping

**Estimated effort**: Medium (~3–5 days). Scoring logic is new but the graph traversal and index are already built.

---

*NL task routing recorded 2026-04-20 after evaluating sigmap as a candidate (NO-GO for adoption, GO as a native waypoint feature direction).*

---

## Codebase Indexing Integration Ideas

**Source**: [research-codebase-indexing-and-waypoint.md](https://github.com/user/dotvault/blob/main/docs/research/research-codebase-indexing-and-waypoint.md) (dual-LLM research, 2026-05-10). Evaluated which ideas from the MCP codebase indexing ecosystem are worth integrating into waypoint.

**Context**: Waypoint already covers 60-70% of what dedicated indexing tools (codebase-memory-mcp, CodeGraphContext, SymDex) offer for the neb stack. The research recommended extending waypoint rather than adopting an external indexer.

### HTTP Route Extraction (`waypoint routes` / `waypoint trace`)

**What**: Index Express/Koa server routes and `fetch`/`axios` client calls during scan. Cross-repo endpoint matching via `scan --all`.

**Why it matters**: Highest-value capability gap vs codebase-memory-mcp for microservice ecosystems. "Which service calls this endpoint?" currently requires `neb-explorer` subagent reading full files across repos.

**ROI assessment** (2026-05-19): High value per use but narrow audience (neb microservice work only). Highest effort of the three candidates (~1-2 weeks). Best as a dedicated investment during a block of cross-service neb work, not as the next general-purpose command.

**Implementation sketch**:
- New `http_routes` table: `(id, file_path, method, path_pattern, handler_symbol, line_number)`
- New `http_calls` table: `(id, file_path, method, url_pattern, line_number)`
- Extract routes via tree-sitter (Express `app.get('/...')` patterns) + regex fallback
- Extract client calls via regex (`fetch('/api/...')`, `axios.post(...)`)
- Cross-repo matching: query sibling project indexes discovered by `scan --all`
- New commands: `waypoint routes [--method GET]`, `waypoint trace <endpoint>`

**Estimated effort**: Medium (~1-2 weeks). Builds on existing scan pipeline and cross-project infrastructure.

### Community Detection

**What**: Louvain clustering on the import graph to identify logical modules that don't align with directory structure.

**Why it matters**: Reveals unexpected coupling, helps scope code reviews, could enhance `waypoint impact` with "this change affects community X."

**Implementation sketch**:
- Louvain algorithm on import adjacency (~100 lines of Rust, no external deps)
- New `communities` table or fold into `arch_summary`
- Surface via `waypoint arch` or new `waypoint communities` command

**Why it's parked**: Import graph may be too shallow (2-3 hops) for meaningful clusters. Higher value after HTTP routes or call graph add edge density.

**Estimated effort**: Low (~1-2 days).

### Byte-Precise Symbol Offsets

**What**: Add `byte_start`/`byte_end` nullable columns alongside existing `line_start`/`line_end`. Tree-sitter already provides byte offsets.

**Why it matters**: Unlocks precision for future features (call graph edges, incremental scan, route extraction). No user-visible change — CLI output stays line-based.

**Estimated effort**: Low (~half day). Schema migration + populate during extract.

### Find Hit Rate Instrumentation

**Status**: ✅ Delivered in v0.10.2. `FindHit`/`FindMiss` events recorded in the ledger. `waypoint gain` now shows Find rate alongside Sketch rate. Tracks end-to-end `waypoint find` success (FTS5 + LIKE fallback combined), not FTS5-specific miss rate. Data gate for whether semantic/embedding search earns its weight — if >20% of find queries miss, vector search is justified.

### Ideas Explicitly Deferred

- **PageRank-style ranking**: Fan-in is sufficient at current graph scale. Revisit when call graph edges increase density.
- **Full embedding model (ONNX + MiniLM)**: ~30MB binary weight, too heavy for core. If find miss rate justifies it, lightest path is TF-IDF vectors in sqlite-vec (no ML model). Full embeddings should be optional/sidecar.
- **MCP server interface**: Low urgency — hooks serve the primary consumer (Claude Code) well. Revisit if other tools need to query waypoint's index.

---

*Indexing integration ideas recorded 2026-05-10 from codebase indexing research review.*

---

## Delivered Baseline (2026-04-23)

Implemented and now considered baseline behavior:

- Ranked `waypoint find` is default behavior (no `--ranked` flag).
- Session-start arch context is file-count gated (`<20` files suppresses arch lines).
- `waypoint impact` is manual-only (no hook auto-trigger), text output only in v1.
- Impact risk tiers: `CRITICAL >=10`, `HIGH 5-9`, `MEDIUM 2-4`, `LOW 0-1`.
- Impact includes private/non-exported changed symbols (`0 importers`, `LOW`).
- Impact uses `std::process::Command` git calls (no `git2` dependency).
- Stale map in impact is warning-only; command still exits successfully on normal operation.
- Ledger kept existing data; `ArchHit`/`ArchMiss` were additive (no reset).

Use this baseline when evaluating future features to avoid reopening settled v1 decisions without new evidence.
