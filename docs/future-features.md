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

**Status**: ✅ V1 delivered in v0.14.0. Graph boost is v2.

**V1 scope (delivered)**:
- `src/ask.rs` (285 LOC, 28 unit tests) — IDF-weighted token coverage scoring over map descriptions
- Tokenizer with camelCase/snake_case splitting for both queries and descriptions
- FTS5 symbol matching as secondary signal (reuses existing `symbols_fts` table)
- Query-shape detection: `::`, `_`, camelCase triggers code-heavy weight profile (0.35 desc / 0.65 symbol vs default 0.6 / 0.4)
- Word-boundary matching via `desc_has_word()` to prevent substring false positives
- `--explain` flag for per-signal breakdown, `--limit N` (default 10), `--context / -C` for cross-project
- `AskHit`/`AskMiss` ledger events for usage tracking
- 5 integration tests (ranked results, miss, explain, limit, cross-project)

**Design decisions (v1)**:
- In-memory IDF scoring over descriptions, not FTS5 on descriptions — max repo is ~2,800 files, instant at this scale
- FTS5 reused only for symbol matching (secondary signal)
- No graph boost in v1 — deferred to v2 pending real-world ranking quality assessment
- Quality gate concern resolved: 78-repo audit showed 100% map description coverage, 99.4% rich prose

**V2: Graph boost (not yet implemented)**:

Remaining work to improve ranking via import/call graph signals:

- **Import adjacency boost** (proposed weight: 0.3): 1-hop neighbors of high-scoring files get a score bump. Batched as a single aggregate SQL query against `imports` table to avoid N+1
- **Call graph boost** (proposed weight: 0.15): files connected via `calls` table edges get a smaller bump
- **God-file cap**: log-scaled or P95 clamp on fan-in to prevent `mod.rs`/`lib.rs`/`index.ts` from dominating rankings via sheer import count
- Weight tuning: empirical, gated on eval harness results

**V2: Eval harness**:

- Formalize smoke tests into task→expected-files pairs across waypoint + neb-www repos
- Track hit@5 and hit@10 metrics
- Gate future scoring changes (graph boost weights, new signals) on non-regression
- Prerequisite for confidently tuning graph boost weights

**Revisit when**: Real-world usage reveals ranking quality gaps that description + symbol matching alone can't resolve. The ledger's `AskHit`/`AskMiss` events provide the signal.

**Estimated effort**: V2 graph boost ~2-3 days, eval harness ~1 day.

**Origin**: sigmap evaluation (2026-04-20) identified the capability as NO-GO for adoption but GO as a native waypoint feature direction.

---

## Codebase Indexing Integration Ideas

**Source**: [research-codebase-indexing-and-waypoint.md](https://github.com/user/dotvault/blob/main/docs/research/research-codebase-indexing-and-waypoint.md) (dual-LLM research, 2026-05-10). Evaluated which ideas from the MCP codebase indexing ecosystem are worth integrating into waypoint.

**Context**: Waypoint already covers 60-70% of what dedicated indexing tools (codebase-memory-mcp, CodeGraphContext, SymDex) offer for the neb stack. The research recommended extending waypoint rather than adopting an external indexer.

### HTTP Route Extraction (`waypoint routes` / `waypoint trace`)

**What**: Index Express/Koa server routes and `fetch`/`axios` client calls during scan. Cross-repo endpoint matching via `scan --all`.

**Why it matters**: Highest-value capability gap vs codebase-memory-mcp for microservice ecosystems. "Which service calls this endpoint?" currently requires `neb-explorer` subagent reading full files across repos.

**ROI assessment** (2026-05-19, updated): High value per use but narrow audience (neb microservice work only). Highest effort of the remaining undelivered features (~1-2 weeks). Best as a dedicated investment during a block of cross-service neb work.

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

## Agent Compliance — Improving Tool Use Adoption

**Problem**: Agents default to rg/grep even when waypoint commands are listed in AGENTS.md. Instruction-file rules are read as reference material, not as enforced preconditions. Observed in sessions: `[waypoint] map:` hook context injected at >200 tok, agent calls full Read anyway.

**Root cause**: By the time the agent selects a tool, it's in "execute" mode. Text instructions don't interrupt that frame reliably.

### Hook-Based Interrupts

**`PreToolUse:Read` nudge** — when the hook injects `[waypoint] map:` and the token count is >~200, append a harder signal: "waypoint sketch not called for this file — run it first to scope the read." Currently the map context is injected but doesn't block; a more assertive message may raise compliance without requiring a full deny.

> **Addendum (2026-07-17):** the `pre-read` hook this builds on was unregistered in 276dcff and its code deleted in v0.22.0. This idea now requires rebuilding a `PreToolUse:Read` hook from scratch, not extending an existing one. (`sketch` was also dropped in v0.16.0 — the nudge target would need rethinking too.)

**`PreToolUse:Bash` rg intercept** — intercept bash calls matching `rg <pattern> <path>` where the pattern looks like a symbol (no spaces, PascalCase or camelCase). Inject: "Try `waypoint find \"<pattern>\"` first — fall back to rg only if it returns no results." Avoids blocking legitimate text searches while nudging symbol lookups.

### Instruction Strengthening

**Explicit fallback gate** — add to the search hierarchy: "Only use rg when `waypoint find`/`waypoint ask` returns no results." Turns a preference ordering into a decision rule with a concrete unlock condition.

### Instrumentation Ideas

**`waypoint audit`** — post-session command that parses the Claude Code conversation log (JSONL), finds Read tool calls not preceded by a waypoint command on the same file within N turns, and reports a compliance rate. Data-driven way to measure whether instruction changes actually move behavior.

**Compliance counter in `waypoint gain`** — alongside token savings, show "waypoint-first rate: X% of file reads were preceded by sketch/find." Makes the failure mode visible in the same report the agent already checks.

---

*Compliance ideas recorded 2026-05-26 from session exploring why AGENTS.md waypoint instructions weren't being followed.*

---

## Delivered Baseline

Implemented and now considered baseline behavior. Use this when evaluating future features to avoid reopening settled decisions without new evidence.

**v0.10.2** (2026-04-23):
- Ranked `waypoint find` is default behavior (no `--ranked` flag).
- Session-start arch context is file-count gated (`<20` files suppresses arch lines).
- `waypoint impact` is manual-only (no hook auto-trigger), text output only in v1.
- Impact risk tiers: `CRITICAL >=10`, `HIGH 5-9`, `MEDIUM 2-4`, `LOW 0-1`.
- Impact includes private/non-exported changed symbols (`0 importers`, `LOW`).
- Impact uses `std::process::Command` git calls (no `git2` dependency).
- Stale map in impact is warning-only; command still exits successfully on normal operation.
- Ledger kept existing data; `ArchHit`/`ArchMiss` were additive (no reset).
- `FindHit`/`FindMiss` ledger events for find hit rate tracking.

**v0.12.0**: PostToolUse:Edit|Write hook for incremental call index updates — resolved staleness blocker for trace.

**v0.13.0**: `waypoint trace` v1 — same-file call graph tracing with DFS traversal, direction/depth controls, and per-language skip lists.

**v0.14.0**: `waypoint ask` v1 — NL task routing with IDF-weighted token coverage, FTS5 symbol matching, query-shape detection, and `--explain` output.

**v0.17.0**: `waypoint find` "see also:" sibling-symbol footer — up to 5 other exported symbols per file, suppressed for barrel files (50+ exports), <2 remaining siblings after exclusion, or results spanning 4+ files.
