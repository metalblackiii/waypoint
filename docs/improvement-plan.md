# Waypoint Improvement Plan

The full research document lives in dotvault:

`~/repos/dotvault/docs/research-second-brain-intelligence-2026-03-28.md`

## Quick Reference

**Current phase:** Baseline review — 124 sessions collected (exceeds 20 target)

**Ratchet metric:** First-edit timing (milliseconds from session start to first file edit). Every change must improve or hold this metric.

**Decision gate pattern:** Measure baseline -> make one change -> re-measure -> decide. Never stack untested changes.

## Baseline Data (2026-03-29 — 2026-04-03)

124 sessions across 9 projects (excluding tmp paths from integration tests).

**Overall:** median ~0.1s, mean 0.3s, max 5.5s

**Distribution:**

| Bucket | Sessions | % |
|--------|----------|---|
| < 0.1s | 59 | 47.6% |
| 0.1–0.5s | 47 | 37.9% |
| 0.5–1.0s | 13 | 10.5% |
| 1.0–3.0s | 4 | 3.2% |
| > 3.0s | 1 | 0.8% |

85.5% of sessions reach first edit in under 0.5s. The 5 slowest sessions (>1s) were all in waypoint or neb-www — likely complex/exploratory tasks.

**Per-project:**

| Project | Sessions | Avg (s) | Max (s) |
|---------|----------|---------|---------|
| waypoint | 42 | 0.3 | 5.5 |
| dotfiles | 24 | 0.2 | 0.7 |
| neb-www | 21 | 0.3 | 2.0 |
| ai-plugins-poc | 20 | 0.2 | 0.8 |
| ptek-ai-playbook | 8 | 0.2 | 0.6 |
| ptek-jira-cli | 4 | 0.1 | 0.2 |
| neb-ms-billing | 2 | 0.2 | 0.3 |
| neb-ms-registry | 2 | 0.3 | 0.6 |
| neb-microservice | 1 | 0.2 | 0.2 |

**Daily trend:**

| Date | Sessions | Avg (s) |
|------|----------|---------|
| 03-29 | 19 | 0.2 |
| 03-30 | 24 | 0.3 |
| 03-31 | 27 | 0.5 |
| 04-01 | 16 | 0.3 |
| 04-02 | 7 | 0.2 |
| 04-03 | 31 | 0.1 |

## Shipped Since Plan Written

| Date | Commit | Feature | Plan Item |
|------|--------|---------|-----------|
| 03-28 | `5934c91` | First-edit timing instrumentation | Critical path #1 |
| 03-28 | `ebe6a5d` | Remove learning subsystem | Invalidates learnings-related items |
| 03-29 | `4a05bd7` | Git hash in --version | — |
| 03-29 | `7586fac` | `status --all` across sibling projects | — |
| 03-31 | `00e4a6e` | JS/TS class method extraction (symbol index) | Enriches pre-flight substrate |
| 03-31 | `18298c2` | Auto-clean stale map entries, inline trap logging | Partially addresses pre-compact (#7) |
| 04-03 | `c8c21d6` | Import-scoped reference tracking | Tier 3 #13 (shipped early) |
| 04-03 | `55f0dc4` | Rust crate-root import resolution | Extension of above |
| 04-03 | `8604a1c` | `callers` command | Extension of above |
| 04-03 | `8bb3ffd` | `trap delete` | — |

## Critical Path — Updated

| Target | Milestone | Status | Notes |
|--------|-----------|--------|-------|
| ~~2026-03-28~~ | First-edit timing instrumentation | **Done** | 124 sessions collected |
| ~~2026-04-11~~ | Baseline review (>= 20 sessions) | **Done** (early) | See baseline data above |
| ~~2026-04-11 — 04-18~~ | Learnings utilization audit | **Moot** | Learning subsystem removed; nothing to audit |
| **Next** | Pre-flight Phase 1 — skill prototype | Not started | Baseline is ready; can begin when bandwidth allows |
| After Phase 1 | Pre-flight Phase 2 — `waypoint preflight` Rust subcommand | Not started | Only if Phase 1 validates hypothesis |
| After Phase 2 | Pre-flight Phase 3 — hook integration | Not started | Highest interaction-effect risk |

## Independent Items — Updated

| Item | Effort | Value | Status |
|------|--------|-------|--------|
| ~~Learning & trap pruning with relevance decay~~ | — | — | **Moot** — learnings removed; trap prune exists |
| ~~Reflexion-style cause explanations~~ | — | — | **Moot** — learnings removed; trap cause field remains free-text |
| BM25 upgrade for trap search | Medium | Medium-High | Not started — grows more valuable as traps accumulate |
| Task-start trap surfacing | Low | Medium | Not started |
| ~~Pre-compact state snapshot~~ | — | — | **Partially addressed** — inline trap logging eliminates the batching risk |
| Import graph / callers | High | High | **Done** — `c8c21d6`, `callers` command shipped |

## Decision Gates — Updated

| Gate | Question | Outcome |
|------|----------|---------|
| ~~Enough sessions?~~ | >= 20 sessions collected? | **Yes** — 124 sessions |
| ~~Learning hits improve first-edit?~~ | Do learning hits help? | **Moot** — subsystem removed |
| **Next** | Does Phase 1 skill prototype improve first-edit on opaque prompts? | Build Phase 2 if yes; stop if no |
| After Phase 3 | Does hook injection regress first-edit? | Roll back to Phase 2 if yes |

## Observations from Baseline

1. **First-edit timing is already fast.** 85% of sessions are under 0.5s. The ratchet metric may need to shift from "move the average" to "reduce outliers" — the 5 sessions >1s are where the most time is lost.

2. **Outliers cluster in waypoint and neb-www.** These are the most complex repos. Pre-flight targeting may help most here — exactly where opaque prompts are most common.

3. **The metric is in milliseconds, not turns.** The Clarté research measured first-edit *turn number*. Waypoint measures wall-clock time from session start to first edit event. These are correlated but not identical — a session with fast turns still shows low ms even if it takes 5 turns to reach first edit. Consider adding turn-count tracking if Phase 1 needs finer signal.

4. **Daily trend shows improvement over time** — 0.5s avg on 03-31 down to 0.1s on 04-03. Could be feature improvements, or could be task-mix variance. More data needed to separate signal from noise.
