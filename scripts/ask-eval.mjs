#!/usr/bin/env node
// Measures whether `waypoint ask` is right when it answers.
//
// Usage: node scripts/ask-eval.mjs <cases.json> [--bin <waypoint>] [--target 0.8] [--cutoff score,lead] [--misses] [--swap]
//
// Cases are a JSON array of {id, project, query, verdict, accept[], session, root?}, where the repo is
// `root` when set, else ~/repos/<project>. verdict "keep"/"relabel" = `accept` lists correct top results; "abstain" =
// the repo holds no right answer, so any answer is wrong; "drop" cases are skipped.
//
// WARNING: case files hold paths from private repos and must stay outside this public repo.
//
// Cases split into a tune half and a test half by session, so related queries from one session
// never straddle the split. The confidence cutoff is chosen on tune and judged only on test.

import { execFileSync } from 'node:child_process';
import { readFileSync, mkdtempSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { homedir, tmpdir } from 'node:os';
import { join } from 'node:path';

const args = process.argv.slice(2);
const VALUE_FLAGS = new Set(['--bin', '--target', '--cutoff']);
const flags = {};
const positional = [];
for (let i = 0; i < args.length; i++) {
  if (VALUE_FLAGS.has(args[i])) flags[args[i]] = args[++i];
  else if (args[i].startsWith('--')) flags[args[i]] = true;
  else positional.push(args[i]);
}
const casesPath = positional[0];
const bin = flags['--bin'] ?? 'waypoint';
const target = Number(flags['--target'] ?? '0.8');
if (!casesPath) {
  console.error('usage: node scripts/ask-eval.mjs <cases.json> [--bin <waypoint>] [--target 0.8] [--cutoff score,lead] [--misses] [--swap]');
  process.exit(2);
}

// Flags are validated before any case runs, so a bad value fails fast instead of after a full
// scoring pass. Strict: `Number('')` is 0, so empty fields must be rejected by pattern.
const UNIT_NUMBER = /^(0(\.\d+)?|1(\.0+)?|\.\d+)$/;
function failUsage(message) {
  console.error(message);
  process.exit(2);
}
let fixedCutoff = null;
if ('--cutoff' in flags) {
  const parts = String(flags['--cutoff'] ?? '').split(',');
  if (parts.length !== 2 || !parts.every((p) => UNIT_NUMBER.test(p))) {
    failUsage('--cutoff needs two numbers in [0, 1]: score,lead (e.g. 0.64,0.02)');
  }
  fixedCutoff = parts.map(Number);
}
if ('--target' in flags && !UNIT_NUMBER.test(String(flags['--target'] ?? ''))) {
  failUsage('--target needs a number in [0, 1] (e.g. 0.8)');
}

// A mistyped verdict would silently count as answerable and skew every metric, so reject it.
const VERDICTS = new Set(['keep', 'relabel', 'abstain', 'drop']);
const allCases = JSON.parse(readFileSync(casesPath, 'utf8'));
for (const c of allCases) {
  if (!VERDICTS.has(c.verdict)) throw new Error(`case #${c.id}: unknown verdict ${JSON.stringify(c.verdict)}`);
  if (!Array.isArray(c.accept)) throw new Error(`case #${c.id}: accept must be an array`);
  if ((c.verdict === 'keep' || c.verdict === 'relabel') && c.accept.length === 0) {
    throw new Error(`case #${c.id}: verdict ${c.verdict} needs at least one accepted path`);
  }
  if (c.verdict === 'abstain' && c.accept.length > 0) {
    throw new Error(`case #${c.id}: abstain means no right answer, so accept must be empty`);
  }
}
const cases = allCases.filter((c) => c.verdict !== 'drop');
const scratchLedger = mkdtempSync(join(tmpdir(), 'wp-ask-eval-'));
/// How deep to look for the correct file when reporting where it ranked.
const RANK_DEPTH = 20;

function rank(c) {
  const root = c.root ? c.root.replace(/^~/, homedir()) : join(homedir(), 'repos', c.project);
  // --json gives full-precision scores, so cutoffs here match `ask::confident` exactly.
  const out = execFileSync(bin, ['ask', c.query, '--limit', String(RANK_DEPTH), '--json', '-C', root], {
    encoding: 'utf8',
    env: { ...process.env, WAYPOINT_DATA_DIR: scratchLedger },
  });
  return JSON.parse(out).map((r) => ({ path: r.path, score: r.score }));
}

const scored = cases.map((c) => {
  const ranked = rank(c);
  const top = ranked[0];
  return {
    ...c,
    positive: c.verdict !== 'abstain',
    top: top?.path ?? null,
    score: top?.score ?? 0,
    margin: top ? top.score - (ranked[1]?.score ?? 0) : 0,
    correct: Boolean(top) && c.verdict !== 'abstain' && c.accept.includes(top.path),
    correctRank: ranked.findIndex((r) => c.accept.includes(r.path)) + 1 || null,
    correctScore: ranked.find((r) => c.accept.includes(r.path))?.score ?? null,
  };
});

// --swap trades the halves: the cutoff is chosen on the other half and judged on this one,
// giving a second held-out estimate. Use it for variance checks, never for ranker changes.
const swap = Boolean(flags['--swap']);
const isTune = (c) => (createHash('sha1').update(c.session).digest()[0] % 2 === 0) !== swap;
const tune = scored.filter(isTune);
const test = scored.filter((c) => !isTune(c));

function measure(set, minScore, minMargin) {
  const answered = set.filter((c) => c.top && c.score >= minScore && c.margin >= minMargin);
  const right = answered.filter((c) => c.correct).length;
  const positives = set.filter((c) => c.positive).length;
  const negatives = set.length - positives;
  const silentNegatives = set.filter((c) => !c.positive && !answered.includes(c)).length;
  return {
    answered: answered.length,
    right,
    precision: answered.length ? right / answered.length : null,
    coverage: positives ? right / positives : 0,
    positives,
    silentNegatives,
    negatives,
  };
}

const steps = (max, step) => Array.from({ length: Math.round(max / step) + 1 }, (_, i) => Number((i * step).toFixed(3)));
let best = null;
for (const minScore of steps(1, 0.02)) {
  for (const minMargin of steps(0.5, 0.02)) {
    const m = measure(tune, minScore, minMargin);
    if (m.precision === null || m.precision < target) continue;
    if (!best || m.right > best.m.right || (m.right === best.m.right && m.answered < best.m.answered)) best = { minScore, minMargin, m };
  }
}

const pct = (x) => (x === null ? '-' : `${(100 * x).toFixed(0)}%`);
const row = (label, m) =>
  console.log(
    `${label.padEnd(28)} answered ${String(m.answered).padStart(3)}  right ${String(m.right).padStart(3)}  precision ${pct(m.precision).padStart(4)}  ` +
      `coverage ${pct(m.coverage).padStart(4)} of ${m.positives}  silent on no-answer ${m.silentNegatives}/${m.negatives}`,
  );

console.log(`cases: ${scored.length} (tune ${tune.length}, test ${test.length}); target precision ${pct(target)}\n`);
// --cutoff score,lead measures a fixed gate (e.g. the one `ask::confident` ships) on every
// case; nothing is chosen from the data, so no half is spent on tuning.
if (fixedCutoff) {
  const [minScore, minMargin] = fixedCutoff;
  row(`all, cutoff ${minScore}/${minMargin}`, measure(scored, minScore, minMargin));
  process.exit(0);
}
row('tune, no cutoff', measure(tune, 0, 0));
row('test, no cutoff', measure(test, 0, 0));
if (best) {
  console.log(`\ncutoff chosen on tune: score >= ${best.minScore}, margin over runner-up >= ${best.minMargin}`);
  row('tune, with cutoff', best.m);
  row('test, with cutoff', measure(test, best.minScore, best.minMargin));
} else {
  console.log(`\nno cutoff reaches ${pct(target)} precision on tune`);
}

// WARNING: lists tune cases only. Reading test-half failures while changing the ranker turns
// the test half into a second tune set, and its score stops predicting real behavior.
if (flags['--misses']) {
  console.log('\nwrong top results (tune half only):');
  for (const c of tune.filter((x) => x.top && !x.correct).sort((a, b) => b.score - a.score)) {
    const where = c.positive ? (c.correctRank ? `right file at #${c.correctRank} (${c.correctScore.toFixed(2)})` : `right file not in top ${RANK_DEPTH}`) : 'no right answer';
    console.log(`  #${c.id} [${c.project}] ${c.query.slice(0, 50)} -> ${c.top} (${c.score.toFixed(2)}); ${where}; want ${c.accept.join(' | ') || 'nothing'}`);
  }
}
