#!/usr/bin/env node
// Measures whether `waypoint ask` is right when it answers.
//
// Cases are a JSON array of {id, project, query, verdict, accept[], session, root?}, where the repo is
// `root` when set, else ~/repos/<project>. verdict "keep"/"relabel" = `accept` lists correct top results; "abstain" =
// the repo holds no right answer, so any answer is wrong; "drop" cases are skipped.
//
// WARNING: case files hold paths from private repos and must stay outside this public repo. --dump
// refuses to write inside it for the same reason.
//
// Cases split into a tune half and a test half by session, so related queries from one session
// never straddle the split. The confidence cutoff is chosen on tune and judged only on test.
//
// Judging a scoring change (indexes drift between runs, so always measure a fresh control):
//   1. node scripts/ask-eval.mjs <cases> --rescan --cutoff 0.64,0.02 --groups --dump <dir>/control.jsonl
//   2. node scripts/ask-eval.mjs <cases> --bin target/release/waypoint --rescan --cutoff 0.64,0.02 --groups --compare <dir>/control.jsonl
//   3. node scripts/ask-eval.mjs <cases> --rescan --cutoff 0.64,0.02   (restores indexes with the installed binary)
// Add --list to steps 1 and 2 to also measure the candidate list `find` shows for a phrase miss.

import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync, openSync, closeSync, renameSync, rmSync, existsSync, realpathSync, mkdtempSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { homedir, tmpdir } from 'node:os';
import { basename, dirname, join, resolve, sep } from 'node:path';

const USAGE =
  'usage: node scripts/ask-eval.mjs <cases.json> [--bin <waypoint>] [--rescan] [--target 0.8] [--misses] [--swap]\n' +
  '       node scripts/ask-eval.mjs <cases.json> [--bin <waypoint>] [--rescan] --cutoff score,lead [--list] [--groups] [--dump <out.jsonl>] [--compare <control.jsonl>]';
const VALUE_FLAGS = new Set(['--bin', '--target', '--cutoff', '--dump', '--compare']);
const SWITCH_FLAGS = new Set(['--rescan', '--misses', '--swap', '--groups', '--list']);
const FIXED_GATE_ONLY = ['--groups', '--dump', '--compare', '--list'];
const TUNING_ONLY = ['--target', '--misses', '--swap'];

function failUsage(message) {
  console.error(`${message}\n${USAGE}`);
  process.exit(2);
}

const args = process.argv.slice(2);
const flags = {};
const positional = [];
for (let i = 0; i < args.length; i++) {
  const arg = args[i];
  if (VALUE_FLAGS.has(arg)) {
    const value = args[++i];
    if (value === undefined || value.startsWith('--')) failUsage(`${arg} needs a value`);
    flags[arg] = value;
  } else if (SWITCH_FLAGS.has(arg)) flags[arg] = true;
  else if (arg.startsWith('-')) failUsage(`unknown flag ${arg}`);
  else positional.push(arg);
}
if (positional.length !== 1) failUsage('expected exactly one cases file');
const casesPath = positional[0];
const bin = flags['--bin'] ?? 'waypoint';

// Flags are validated before any case runs, so a bad value fails fast instead of after a full
// scoring pass. Strict: `Number('')` is 0, so empty fields must be rejected by pattern.
const UNIT_NUMBER = /^(0(\.\d+)?|1(\.0+)?|\.\d+)$/;
let fixedCutoff = null;
if ('--cutoff' in flags) {
  const parts = flags['--cutoff'].split(',');
  if (parts.length !== 2 || !parts.every((p) => UNIT_NUMBER.test(p))) {
    failUsage('--cutoff needs two numbers in [0, 1]: score,lead (e.g. 0.64,0.02)');
  }
  fixedCutoff = parts.map(Number);
  for (const f of TUNING_ONLY) if (f in flags) failUsage(`${f} chooses or reads a tuned cutoff, so it cannot combine with --cutoff`);
} else {
  for (const f of FIXED_GATE_ONLY) if (f in flags) failUsage(`${f} needs --cutoff, so every case is judged by one gate`);
}
if ('--target' in flags && !UNIT_NUMBER.test(flags['--target'])) {
  failUsage('--target needs a number in [0, 1] (e.g. 0.8)');
}
const target = Number(flags['--target'] ?? '0.8');
const listMode = Boolean(flags['--list']);

// WARNING: mirrors `CANDIDATE_FLOOR` and `CANDIDATE_MAX` in src/ask.rs; change both together or
// --list stops describing the candidate list `find` shows.
const LIST_FLOOR = 0.4;
const LIST_SIZE = 3;

// Canonical paths follow symlinks, so a link into the repo cannot smuggle a dump past the guard.
// A dangling link is left unresolved here; writeDump renames over the link, never through it.
const canonical = (path) => {
  const absolute = resolve(path);
  return existsSync(absolute) ? realpathSync(absolute) : join(realpathSync(dirname(absolute)), basename(absolute));
};
const REPO_ROOT = realpathSync(resolve(import.meta.dirname, '..'));
const dumpPath = flags['--dump'] ? canonical(flags['--dump']) : null;
if (dumpPath) {
  if (dumpPath === REPO_ROOT || dumpPath.startsWith(REPO_ROOT + sep)) {
    failUsage('--dump rows carry private case paths; write them outside this repo');
  }
  const inputs = [casesPath, flags['--compare']].filter(Boolean).map(canonical);
  if (inputs.includes(dumpPath)) failUsage('--dump would overwrite an input file; pick another path');
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

// A control only means something against the same labels and gate: a relabelled case or a
// different cutoff changes outcomes with no code change, which would pass for a ranker effect.
const caseHash = (c) =>
  createHash('sha1')
    .update(JSON.stringify([c.query, c.verdict, c.accept, c.root ?? null, c.project ?? null]))
    .digest('hex');
const control = flags['--compare'] ? readControl(flags['--compare']) : null;

function readControl(path) {
  const mismatch = (why) => failUsage(`--compare file ${path} does not match this run: ${why}`);
  const byId = new Map();
  for (const line of readFileSync(path, 'utf8').split('\n')) {
    if (!line.trim()) continue;
    const row = JSON.parse(line);
    if (byId.has(row.id)) mismatch(`case #${row.id} appears twice`);
    byId.set(row.id, row);
  }
  if (byId.size !== cases.length) mismatch(`${byId.size} rows for ${cases.length} cases`);
  for (const c of cases) {
    const row = byId.get(c.id);
    if (!row) mismatch(`case #${c.id} missing`);
    if (row.caseHash !== caseHash(c)) mismatch(`case #${c.id} query or labels differ`);
    if (JSON.stringify(row.cutoff) !== JSON.stringify(fixedCutoff)) mismatch(`cutoff ${JSON.stringify(row.cutoff)}, this run ${fixedCutoff.join(',')}`);
    // Controls written before --list existed carry no `list` field; they measured no list.
    if ((row.list ?? false) !== listMode) mismatch(`list ${row.list ?? false}, this run ${listMode}`);
  }
  return byId;
}
const scratchLedger = mkdtempSync(join(tmpdir(), 'wp-ask-eval-'));
process.on('exit', () => rmSync(scratchLedger, { recursive: true, force: true }));
const runEnv = { ...process.env, WAYPOINT_DATA_DIR: scratchLedger };
/// How deep to look for the correct file when reporting where it ranked.
const RANK_DEPTH = 20;
const caseRoot = (c) => (c.root ? c.root.replace(/^~/, homedir()) : join(homedir(), 'repos', c.project));

const binVersion = execFileSync(bin, ['--version'], { encoding: 'utf8', env: runEnv }).trim();
// Every scored repo is rescanned, with the binary under test, so indexes match the code being judged.
if (flags['--rescan']) {
  const roots = [...new Set(cases.map(caseRoot))];
  for (const root of roots) execFileSync(bin, ['scan', root], { stdio: ['ignore', 'ignore', 'inherit'], env: runEnv });
  console.log(`rescanned ${roots.length} repos with ${binVersion}`);
}

function rank(c) {
  // --json gives full-precision scores, so cutoffs here match `ask::candidates` exactly.
  const out = execFileSync(bin, ['ask', c.query, '--limit', String(RANK_DEPTH), '--json', '-C', caseRoot(c)], {
    encoding: 'utf8',
    env: runEnv,
  });
  return JSON.parse(out).map((r) => ({ path: r.path, score: r.score }));
}

const scored = cases.map((c) => {
  const ranked = rank(c);
  const top = ranked[0];
  const list = ranked.slice(0, LIST_SIZE).filter((r) => r.score >= LIST_FLOOR);
  return {
    ...c,
    positive: c.verdict !== 'abstain',
    top: top?.path ?? null,
    score: top?.score ?? 0,
    margin: top ? top.score - (ranked[1]?.score ?? 0) : 0,
    correct: Boolean(top) && c.verdict !== 'abstain' && c.accept.includes(top.path),
    correctRank: ranked.findIndex((r) => c.accept.includes(r.path)) + 1 || null,
    correctScore: ranked.find((r) => c.accept.includes(r.path))?.score ?? null,
    listLength: list.length,
    // 1-based position of the right file in the shown list; null when absent or for abstain cases.
    listed: c.verdict === 'abstain' ? null : list.findIndex((r) => c.accept.includes(r.path)) + 1 || null,
  };
});

const passes = (c, minScore, minMargin) => Boolean(c.top) && c.score >= minScore && c.margin >= minMargin;

function measure(set, minScore, minMargin) {
  const answered = set.filter((c) => passes(c, minScore, minMargin));
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

const pct = (x) => (x === null ? '-' : `${(100 * x).toFixed(0)}%`);
const row = (label, m) =>
  console.log(
    `${label.padEnd(28)} answered ${String(m.answered).padStart(3)}  right ${String(m.right).padStart(3)}  precision ${pct(m.precision).padStart(4)}  ` +
      `coverage ${pct(m.coverage).padStart(4)} of ${m.positives}  silent on no-answer ${m.silentNegatives}/${m.negatives}`,
  );

console.log(binVersion);
if (fixedCutoff) reportFixedGate(...fixedCutoff);
else reportTunedGate();

// --cutoff score,lead measures a fixed gate (e.g. the one `ask::candidates` labels `strong`) on
// every case; nothing is chosen from the data, so no half is spent on tuning.
function reportFixedGate(minScore, minMargin) {
  console.log(`cases: ${scored.length}\n`);
  row(`all, cutoff ${minScore}/${minMargin}`, measure(scored, minScore, minMargin));
  if (listMode) printListMeasure();
  const rows = scored.map((c) => ({
    id: c.id,
    caseHash: caseHash(c),
    cutoff: [minScore, minMargin],
    verdict: c.verdict,
    answered: passes(c, minScore, minMargin),
    correct: c.correct,
    score: c.score,
    margin: c.margin,
    top: c.top,
    correctRank: c.correctRank,
    list: listMode,
    listed: listMode ? c.listed : null,
    accept: c.accept,
  }));
  if (flags['--groups']) printMissGroups(minScore, minMargin);
  if (dumpPath) {
    writeDump(rows);
    console.log(`\nwrote ${rows.length} rows to ${dumpPath}`);
  }
  if (control) printComparison(rows);
}

// WARNING: write a fresh 0600 file and rename it into place. Writing to the path directly
// would go through a hard link or dangling symlink to the target (possibly the case file or
// a path inside this repo) and briefly expose rows under an existing file's looser mode.
function writeDump(rows) {
  const staging = `${dumpPath}.${process.pid}.tmp`;
  // Opened outside the try: a failed exclusive open created nothing, so there is nothing to remove.
  const fd = openSync(staging, 'wx', 0o600);
  let open = true;
  try {
    writeFileSync(fd, rows.map((r) => JSON.stringify(r)).join('\n') + '\n');
    closeSync(fd);
    open = false;
    renameSync(staging, dumpPath);
  } catch (cause) {
    if (open) closeSync(fd);
    rmSync(staging, { force: true });
    throw new Error(`could not write --dump ${dumpPath}`, { cause });
  }
}

// The candidate list `find` prints for a phrase miss: the first LIST_SIZE ranked files at or
// above LIST_FLOOR, shown whether or not the gate passes.
function printListMeasure() {
  const answerable = scored.filter((c) => c.positive);
  const noAnswer = scored.filter((c) => !c.positive);
  const answerableLists = answerable.filter((c) => c.listLength > 0);
  const lists = scored.filter((c) => c.listLength > 0);
  const entries = lists.reduce((sum, c) => sum + c.listLength, 0);
  const ratio = (n, d) => `${n}/${d} (${pct(d ? n / d : null)})`;
  console.log(`\nlist (top ${LIST_SIZE}, score >= ${LIST_FLOOR}):`);
  console.log(`  right file listed          ${ratio(answerable.filter((c) => c.listed).length, answerable.length)}`);
  console.log(`  right file first           ${ratio(answerable.filter((c) => c.listed === 1).length, answerable.length)}`);
  console.log(`  lists without right file   ${ratio(answerableLists.filter((c) => !c.listed).length, answerableLists.length)}`);
  console.log(`  no-answer cases with list  ${ratio(noAnswer.filter((c) => c.listLength > 0).length, noAnswer.length)}`);
  console.log(`  average entries per list   ${lists.length ? (entries / lists.length).toFixed(1) : '-'}`);
}

// Each group needs a different kind of fix, so one coverage number hides which change helps.
function printMissGroups(minScore, minMargin) {
  const groups = new Map([
    ['covered', 0],
    ['calibration: right file first, under score bar', 0],
    ['tie: right file first, under lead bar', 0],
    [`ranking: right file at rank 2-${RANK_DEPTH}`, 0],
    [`vocabulary gap: right file not in top ${RANK_DEPTH}`, 0],
  ]);
  const [covered, calibration, tie, ranking, vocabulary] = groups.keys();
  for (const c of scored.filter((x) => x.positive)) {
    let group;
    if (passes(c, minScore, minMargin) && c.correct) group = covered;
    else if (!c.correctRank) group = vocabulary;
    else if (c.correctRank > 1) group = ranking;
    else if (c.score < minScore) group = calibration;
    else group = tie;
    groups.set(group, groups.get(group) + 1);
  }
  console.log('\nanswerable cases by group:');
  for (const [group, count] of groups) console.log(`  ${String(count).padStart(3)}  ${group}`);
}

// Pairs rows by case id and lists every case whose answer under the gate changed.
function printComparison(rows) {
  const outcome = (r) => (!r.answered ? 'silent' : r.correct ? 'right' : 'wrong');
  const describe = (r) => `${outcome(r)} ${r.score.toFixed(3)}/${r.margin.toFixed(3)} ${r.top ?? '-'}`;
  const tally = new Map();
  const changed = [];
  for (const r of rows) {
    const before = control.get(r.id);
    if (before.answered === r.answered && (!r.answered || before.top === r.top)) continue;
    const shift = `${outcome(before)} -> ${outcome(r)}`;
    tally.set(shift, (tally.get(shift) ?? 0) + 1);
    changed.push({ r, before });
  }
  console.log(`\nchanged answers vs control: ${changed.length}`);
  for (const [shift, count] of [...tally].sort()) console.log(`  ${String(count).padStart(3)}  ${shift}`);
  for (const { r, before } of changed) {
    console.log(`  #${r.id} [${r.verdict}] ${describe(before)}  =>  ${describe(r)}; want ${r.accept.join(' | ') || 'nothing'}`);
  }
  if (!listMode) return;
  const position = (r) => (r.listed ? `listed #${r.listed}` : 'not listed');
  const moved = rows.filter((r) => control.get(r.id).listed !== r.listed);
  console.log(`\nchanged list positions vs control: ${moved.length}`);
  for (const r of moved) {
    console.log(`  #${r.id} [${r.verdict}] ${position(control.get(r.id))}  =>  ${position(r)}; want ${r.accept.join(' | ') || 'nothing'}`);
  }
}

function reportTunedGate() {
  // --swap trades the halves: the cutoff is chosen on the other half and judged on this one,
  // giving a second held-out estimate. Use it for variance checks, never for ranker changes.
  const swap = Boolean(flags['--swap']);
  const isTune = (c) => (createHash('sha1').update(c.session).digest()[0] % 2 === 0) !== swap;
  const tune = scored.filter(isTune);
  const test = scored.filter((c) => !isTune(c));

  const steps = (max, step) => Array.from({ length: Math.round(max / step) + 1 }, (_, i) => Number((i * step).toFixed(3)));
  let best = null;
  for (const minScore of steps(1, 0.02)) {
    for (const minMargin of steps(0.5, 0.02)) {
      const m = measure(tune, minScore, minMargin);
      if (m.precision === null || m.precision < target) continue;
      if (!best || m.right > best.m.right || (m.right === best.m.right && m.answered < best.m.answered)) best = { minScore, minMargin, m };
    }
  }

  console.log(`cases: ${scored.length} (tune ${tune.length}, test ${test.length}); target precision ${pct(target)}\n`);
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
}
