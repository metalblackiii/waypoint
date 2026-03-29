# Waypoint Improvement Plan

The full research document and dated improvement plan live in dotvault:

`~/repos/dotvault/docs/research-second-brain-intelligence-2026-03-28.md`

## Quick Reference

**Current phase:** Baseline data collection (first-edit timing deployed 2026-03-28)

**Next checkpoint:** 2026-04-11 — review baseline data, begin learnings utilization audit

**Key dates:**

- 2026-04-11: Baseline review (>= 20 sessions needed)
- 2026-04-11 — 04-18: Learnings utilization audit
- 2026-04-18 — 04-25: Pre-flight Phase 1 (skill prototype)
- 2026-05 (early): Pre-flight Phase 2 (Rust subcommand)
- 2026-05 (late): Pre-flight Phase 3 (hook integration)

**Ratchet metric:** First-edit timing (seconds from session start to first file edit). Every change must improve or hold this metric — regressions trigger rollback.

**Decision gate pattern:** Measure baseline -> make one change -> re-measure -> decide. Never stack untested changes.
