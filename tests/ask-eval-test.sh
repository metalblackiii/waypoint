#!/bin/bash
# Regression coverage for scripts/ask-eval.mjs --prereg/--arm: arm flags reach every waypoint
# call, stray shell flags never do, dump rows carry the prereg hash and arm, and --compare and
# the prereg validator refuse what they should. Runs against a stub `waypoint` in a temp dir, so
# no real index, case file or ledger is touched.
# Run with `just test-scripts` (`just test` is Rust-only).
#
# shellcheck disable=SC2016  # the stub's $1/$log/$(...) are written literally into its source
set -euo pipefail

EVAL="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/scripts/ask-eval.mjs"
T="$(mktemp -d "${TMPDIR:-/tmp}/ask-eval-test.XXXXXX")"
trap 'rm -rf "$T"' EXIT
FAILURES=0
pass() { echo "PASS $1"; }
fail() { echo "FAIL $1"; FAILURES=$((FAILURES + 1)); }
# check <name> <cmd...>: passes when the command succeeds.
check() { local name="$1"; shift; if "$@"; then pass "$name"; else fail "$name"; fi; }
count_is() { [ "$(grep -c "$2" "$3" || true)" = "$1" ]; }

# Stub: logs every WAYPOINT_EXP_* it receives, returns one fixed ranking.
mkdir "$T/bin"
printf '%s\n' '#!/bin/bash' \
  "log=\"$T/env.log\"" \
  'case "$1" in' \
  '  --version) echo "waypoint stub" ;;' \
  '  scan) env | grep "^WAYPOINT_EXP_" | sed "s/^/scan /" >> "$log" || true ;;' \
  '  ask) flags=$(env | grep "^WAYPOINT_EXP_" | sort | tr "\n" " " || true)' \
  '       echo "ask ${flags:-none}" >> "$log"' \
  '       if [ -n "${STUB_STRONG:-}" ]; then label=",\"strong\":$STUB_STRONG"; else label=""; fi' \
  '       echo "[{\"path\":\"src/a.rs\",\"score\":0.9$label},{\"path\":\"src/b.rs\",\"score\":0.5}]" ;;' \
  'esac' > "$T/bin/waypoint"
chmod +x "$T/bin/waypoint"

printf '%s\n' \
  '[{"id":1,"root":"/tmp","query":"alpha beta","verdict":"keep","accept":["src/a.rs"],"session":"s1"},' \
  ' {"id":2,"root":"/tmp","query":"gamma delta","verdict":"abstain","accept":[],"session":"s2"}]' > "$T/cases.json"
printf '%s\n' '{"id":"stub-v1","bar":"prior must not lose a covered case",' \
  ' "arms":[{"name":"control","env":{}},{"name":"prior","env":{"WAYPOINT_EXP_KIND_PRIOR":"0.5"}}]}' > "$T/prereg.json"

run() { node "$EVAL" "$T/cases.json" --bin "$T/bin/waypoint" --cutoff 0.64,0.02 "$@"; }

# expect_usage <name> <cmd...>: the command must exit 2 (usage) and print a reason.
expect_usage() {
  local name="$1"; shift
  local out code=0
  out=$("$@" 2>&1) || code=$?
  if [ "$code" = 2 ] && [ -n "$out" ]; then pass "$name"; else fail "$name (exit $code)"; fi
}

# --- arm env reaches the binary; stray exports do not ---
out=$(WAYPOINT_EXP_LEAK=1 run --prereg "$T/prereg.json" --arm control --dump "$T/control.jsonl" 2>&1)
check "prereg line printed" grep -q 'prereg stub-v1' <<<"$out"
check "dropped stray flag is named" grep -q 'ignoring inherited WAYPOINT_EXP_LEAK' <<<"$out"
if grep -q 'WAYPOINT_EXP_' "$T/env.log"; then fail "control arm saw no WAYPOINT_EXP_*"; else pass "control arm saw no WAYPOINT_EXP_*"; fi
hash=$(shasum -a 256 "$T/prereg.json" | cut -d' ' -f1)
if grep -q "\"prereg\":\"$hash\"" "$T/control.jsonl" && grep -q '"arm":"control"' "$T/control.jsonl"; then
  pass "dump rows carry sha256 and arm"
else
  fail "dump rows carry sha256 and arm"
fi

rm -f "$T/env.log"
if run --prereg "$T/prereg.json" --arm prior --rescan --dump "$T/prior.jsonl" --compare "$T/control.jsonl" > /dev/null 2>&1; then
  pass "prior arm compares against control arm"
else
  fail "prior arm compares against control arm"
fi
check "every ask saw the arm flag" count_is 2 '^ask WAYPOINT_EXP_KIND_PRIOR=0.5 $' "$T/env.log"
check "scan saw the arm flag" grep -q '^scan WAYPOINT_EXP_KIND_PRIOR=0.5' "$T/env.log"

# --- --compare refusals ---
sed 's/must not lose/may lose/' "$T/prereg.json" > "$T/edited.json"
expect_usage "compare refuses a control from another prereg" run --prereg "$T/edited.json" --arm prior --compare "$T/control.jsonl"
expect_usage "compare refuses a control from the arm under test" run --prereg "$T/prereg.json" --arm prior --compare "$T/prior.jsonl"
sed 's/,"prereg":[^,]*,"arm":[^,]*//' "$T/control.jsonl" > "$T/legacy.jsonl"
expect_usage "compare refuses a pre-prereg control in a prereg run" run --prereg "$T/prereg.json" --arm prior --compare "$T/legacy.jsonl"
expect_usage "compare refuses a prereg control in a plain run" run --compare "$T/control.jsonl"
sed 's/,"arm":"control"//' "$T/control.jsonl" > "$T/no-arm.jsonl"
expect_usage "compare refuses a control row with no arm" run --prereg "$T/prereg.json" --arm prior --compare "$T/no-arm.jsonl"

# --- prereg validation ---
expect_usage "--prereg without --arm" run --prereg "$T/prereg.json"
expect_usage "--arm without --prereg" run --arm prior
expect_usage "unknown arm" run --prereg "$T/prereg.json" --arm nope
expect_usage "--prereg without --cutoff" node "$EVAL" "$T/cases.json" --bin "$T/bin/waypoint" --prereg "$T/prereg.json" --arm control
expect_usage "missing prereg file" run --prereg "$T/absent.json" --arm control
echo 'not json' > "$T/broken.json"
expect_usage "invalid JSON" run --prereg "$T/broken.json" --arm a
echo '{"id":"x","bar":"b","arms":[{"name":"a","env":{"PATH":"/evil"}}]}' > "$T/bad-env.json"
expect_usage "non-WAYPOINT_EXP_ env key" run --prereg "$T/bad-env.json" --arm a
echo '{"id":"x","bar":"b","arms":[{"name":"a","env":{"WAYPOINT_EXP_X":1}}]}' > "$T/num-env.json"
expect_usage "non-string flag value" run --prereg "$T/num-env.json" --arm a
echo '{"id":"x","bar":" ","arms":[{"name":"a","env":{}}]}' > "$T/no-bar.json"
expect_usage "blank bar" run --prereg "$T/no-bar.json" --arm a
echo '{"id":"x","bar":"b","arms":[{"name":"a","env":{}},{"name":"a","env":{}}]}' > "$T/dup.json"
expect_usage "duplicate arm" run --prereg "$T/dup.json" --arm a
expect_usage "dump over the prereg file" run --prereg "$T/prereg.json" --arm control --dump "$T/prereg.json"

# --- plain runs are unchanged apart from null prereg/arm ---
if run --dump "$T/plain.jsonl" > /dev/null 2>&1 && grep -q '"prereg":null,"arm":null' "$T/plain.jsonl"; then
  pass "plain run writes null prereg and arm"
else
  fail "plain run writes null prereg and arm"
fi

# --- the binary's own strong label is recorded and checked against the JS gate ---
# Stub scores 0.9/0.5 pass the 0.64/0.02 gate on both cases.
out=$(STUB_STRONG=true run --dump "$T/strong.jsonl" 2>&1)
check "dump rows carry the binary's strong label" count_is 2 '"strong":true' "$T/strong.jsonl"
check "agreeing label reports 0 disagreements" grep -q 'binary strong vs gate: 0 disagree' <<<"$out"
out=$(STUB_STRONG=false run 2>&1)
check "disagreeing label is counted and named" grep -q 'binary strong vs gate: 2 disagree (#1 #2)' <<<"$out"
out=$(run --dump "$T/unlabelled.jsonl" 2>&1)
check "missing label dumps null" count_is 2 '"strong":null' "$T/unlabelled.jsonl"
check "missing label prints no comparison" bash -c '! grep -q "binary strong" <<<"$1"' _ "$out"

echo "failures: $FAILURES"
[ "$FAILURES" = 0 ]
