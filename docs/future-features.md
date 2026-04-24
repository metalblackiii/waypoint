# Future Features

Parked ideas with full context. Not scheduled — recorded so the reasoning survives.

## Call Graph Tracing (`waypoint trace`)

**What**: Track actual function calls (not just imports). `waypoint trace <symbol> [--direction inbound|outbound|both] [--depth N]` walks the call chain.

**Why it matters**: Single biggest capability gap vs codebase-memory-mcp. Waypoint knows imports but not call chains. "Who calls `validatePayment`?" requires knowing that `handleOrder()` calls it, not just that `checkout.rs` imports it.

**Implementation sketch**:
- New `calls` table in SQLite: `(id, source_file, source_symbol, target_symbol, target_file, line_number)`
- Extract call expressions from tree-sitter AST during scan (walk function bodies for call nodes)
- Resolve call targets against symbol registry (name matching, qualified names for methods)
- Two-pass scan: extract all symbols first, then resolve calls against complete registry
- ~800-1,200 LOC in extract.rs + index.rs + new trace module

**Why it's parked**:
- **Staleness**: Call graph data goes stale on every edit. Unlike map descriptions (tolerably stale) or impact (conservatively stale — underreports, never lies), stale call data *actively misleads* — reporting call chains that no longer exist or missing new ones.
- **No Codex hooks**: Codex doesn't support the hooks needed to trigger rescan after edits. Agents working in Codex would operate on perpetually stale call data.
- **Resolution accuracy**: Cross-file call resolution is hard. Dynamic dispatch, closures, method chains, and overloaded names all defeat simple name matching. CBM spent ~19K lines of C on their pipeline. False positives degrade trust.
- **Architectural change**: Current scan is single-pass. Call resolution requires two-pass (symbols first, then calls resolved against registry). Changes the scan pipeline, not just additive code.

**What would unblock it**:
- Background watcher or incremental rescan that keeps call data fresh between sessions
- Codex gaining hook support (specifically PreToolUse or post-edit hooks)
- Alternatively: accepting "call graph is only accurate at scan time" and making scan fast enough to run frequently (incremental scan would help)

**Estimated effort**: High (~1-2 weeks). Roughly 15-20% of current codebase size.

## Dead Code Detection (`waypoint dead`)

**What**: Find exported symbols with zero callers. `waypoint dead [--kind fn|type|all]` lists symbols nobody uses.

**Implementation sketch**:
- SQL query on `calls` table: `SELECT * FROM symbols WHERE exported = 1 AND name NOT IN (SELECT target_symbol FROM calls)`
- Exclusion mechanism for entry points, test targets, framework magic
- ~150-250 LOC

**Why it's parked**: Depends entirely on the `calls` table from trace. Without call data, "zero callers" is meaningless — you'd only detect symbols with zero *importers*, which `waypoint callers` already surfaces.

**What would unblock it**: Trace shipping first. Dead code is trivially a query on the calls table.

**Estimated effort**: Low (after trace), impossible (before trace).

## NL Task Routing (`waypoint ask`)

**What**: Rank files by relevance to a natural-language task description. `waypoint ask "<task>"` returns a scored list of the most relevant files — e.g., `waypoint ask "implement OAuth middleware"` → ranked file paths with match reasons.

**Why it matters**: Waypoint is currently symbol-name-based. If you don't know what symbol to look for, you're stuck. NL task routing removes the bootstrap problem: start from intent, not from symbol names. Evaluated `sigmap` (manojmallick/sigmap) as a candidate drop-in — its `sigmap ask` command does exactly this. Rejected it (NO-GO: 20 days old, sole contributor, MCP-dependent value). The capability is real; the right home is here.

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
