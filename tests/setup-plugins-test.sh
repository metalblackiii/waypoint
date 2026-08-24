#!/bin/bash
# Regression coverage for setup-plugins.sh's argument validation and the
# functions that mutate real user state (legacy marketplace removal,
# plugin_hooks config editing). Trimmed port of
# ptek-jira-cli/tests/setup-plugins-test.sh — no fake-CLI matrix here because
# waypoint's install paths are structurally identical to the tested upstream;
# only the waypoint-specific state mutations get local coverage.
# Not wired into `just test` (that recipe is Rust-only) — run directly.
#
# shellcheck disable=SC1090  # dynamic `source "$SCRIPT"` resolves to setup-plugins.sh at repo root
# shellcheck disable=SC2030,SC2031  # HOME is deliberately scoped to each subshell so tests never leak into the real $HOME
set -euo pipefail

SCRIPT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/setup-plugins.sh"
FAILURES=0

# Each test body runs in a subshell; fail exits it nonzero so the parent's
# `|| FAILURES=...` accumulator actually sees the failure. A fail that only
# incremented a subshell-local counter would leave the suite exiting 0.
fail() {
  echo "FAIL: $1"
  exit 1
}

pass() {
  echo "PASS: $1"
}

# ── Argument validation ──────────────────────────────────────────

(
  HOME="$(mktemp -d)"
  export HOME
  source "$SCRIPT"
  if main --bogus-flag >/dev/null 2>&1; then
    fail "main --bogus-flag should exit nonzero"
  fi
  pass "main rejects unknown flag"
) || FAILURES=$((FAILURES + 1))

(
  HOME="$(mktemp -d)"
  export HOME
  source "$SCRIPT"
  if main --reload --uninstall >/dev/null 2>&1; then
    fail "main with two flags should exit nonzero"
  fi
  pass "main rejects multiple flags"
) || FAILURES=$((FAILURES + 1))

# ── remove_legacy_codex_marketplace ──────────────────────────────

(
  HOME="$(mktemp -d)"
  export HOME
  mkdir -p "$HOME/.agents/plugins"
  echo '{"name":"waypoint-plugins","plugins":[]}' > "$HOME/.agents/plugins/waypoint.json"
  source "$SCRIPT"
  remove_legacy_codex_marketplace >/dev/null || fail "removal should return 0"
  [ ! -f "$HOME/.agents/plugins/waypoint.json" ] || fail "legacy waypoint.json should be removed"
  pass "legacy waypoint.json removed when present"
) || FAILURES=$((FAILURES + 1))

(
  HOME="$(mktemp -d)"
  export HOME
  source "$SCRIPT"
  remove_legacy_codex_marketplace >/dev/null || fail "absent legacy file should return 0"
  pass "absent legacy file is a no-op success"
) || FAILURES=$((FAILURES + 1))

(
  HOME="$(mktemp -d)"
  export HOME
  mkdir -p "$HOME/.agents/plugins"
  echo '{}' > "$HOME/.agents/plugins/waypoint.json"
  chmod 555 "$HOME/.agents/plugins"
  source "$SCRIPT"
  REMOVAL_RC=0
  remove_legacy_codex_marketplace >/dev/null 2>&1 || REMOVAL_RC=$?
  # Restore permissions before asserting so cleanup works even on failure
  chmod 755 "$HOME/.agents/plugins"
  [ "$REMOVAL_RC" -ne 0 ] || fail "unremovable legacy file should return nonzero"
  pass "unremovable legacy file returns nonzero with warning"
) || FAILURES=$((FAILURES + 1))

# ── ensure_codex_plugin_hooks ────────────────────────────────────

(
  HOME="$(mktemp -d)"
  export HOME
  mkdir -p "$HOME/.codex"
  printf '[other]\nkey = 1\n' > "$HOME/.codex/config.toml"
  source "$SCRIPT"
  ensure_codex_plugin_hooks >/dev/null
  grep -q '^\[features\]' "$HOME/.codex/config.toml" \
    || fail "config.toml should gain a [features] table"
  grep -qE '^plugin_hooks = true' "$HOME/.codex/config.toml" \
    || fail "config.toml should gain plugin_hooks = true"
  pass "plugin_hooks section appended when missing"
) || FAILURES=$((FAILURES + 1))

(
  HOME="$(mktemp -d)"
  export HOME
  mkdir -p "$HOME/.codex"
  printf '[features]\nplugin_hooks = false\n' > "$HOME/.codex/config.toml"
  source "$SCRIPT"
  ensure_codex_plugin_hooks >/dev/null
  grep -qE '^plugin_hooks = true' "$HOME/.codex/config.toml" \
    || fail "plugin_hooks = false should be updated to true"
  ! grep -qE 'plugin_hooks = false' "$HOME/.codex/config.toml" \
    || fail "plugin_hooks = false should no longer be present"
  pass "plugin_hooks = false flipped to true in place"
) || FAILURES=$((FAILURES + 1))

(
  HOME="$(mktemp -d)"
  export HOME
  mkdir -p "$HOME/.codex"
  printf '[features]\nplugin_hooks = true\nother = 1\n' > "$HOME/.codex/config.toml"
  BEFORE="$(cat "$HOME/.codex/config.toml")"
  source "$SCRIPT"
  ensure_codex_plugin_hooks >/dev/null
  [ "$(cat "$HOME/.codex/config.toml")" = "$BEFORE" ] \
    || fail "already-enabled config should not be modified"
  pass "already-enabled config left untouched"
) || FAILURES=$((FAILURES + 1))

(
  HOME="$(mktemp -d)"
  export HOME
  mkdir -p "$HOME/.codex"
  # plugin_hooks living in a DIFFERENT table belongs to someone else:
  # it must be left untouched and [features].plugin_hooks must be created.
  printf '[sandbox]\nplugin_hooks = false\n' > "$HOME/.codex/config.toml"
  source "$SCRIPT"
  ensure_codex_plugin_hooks >/dev/null
  grep -qE '^plugin_hooks = false' "$HOME/.codex/config.toml" \
    || fail "foreign table's plugin_hooks = false must be left untouched"
  grep -q '^\[features\]' "$HOME/.codex/config.toml" \
    || fail "[features] table should be created"
  awk '/^\[/{t=($0 ~ /^\[features\]/)} t && /^plugin_hooks = true/{found=1} END{exit !found}' \
    "$HOME/.codex/config.toml" \
    || fail "[features].plugin_hooks = true should be created"
  pass "plugin_hooks in a foreign table is not touched; [features] gets its own"
) || FAILURES=$((FAILURES + 1))

(
  HOME="$(mktemp -d)"
  export HOME
  mkdir -p "$HOME/.codex"
  # TOML permits whitespace inside header brackets — '[ features ]' is the
  # same table as '[features]' and must be edited, not duplicated.
  printf '[ features ]\nplugin_hooks = false\n' > "$HOME/.codex/config.toml"
  source "$SCRIPT"
  ensure_codex_plugin_hooks >/dev/null
  grep -qE '^plugin_hooks = true' "$HOME/.codex/config.toml" \
    || fail "plugin_hooks under '[ features ]' should be updated to true"
  [ "$(grep -c 'features' "$HOME/.codex/config.toml")" -eq 1 ] \
    || fail "no duplicate [features] table should be appended"
  pass "whitespace-formatted '[ features ]' header recognized"
) || FAILURES=$((FAILURES + 1))

(
  HOME="$(mktemp -d)"
  export HOME
  mkdir -p "$HOME/.codex"
  # Leading whitespace before the header is valid TOML too
  printf '  [features]\nplugin_hooks = false\n' > "$HOME/.codex/config.toml"
  source "$SCRIPT"
  ensure_codex_plugin_hooks >/dev/null
  grep -qE '^plugin_hooks = true' "$HOME/.codex/config.toml" \
    || fail "plugin_hooks under indented '[features]' should be updated to true"
  [ "$(grep -c 'features' "$HOME/.codex/config.toml")" -eq 1 ] \
    || fail "no duplicate table for indented header"
  pass "leading-whitespace '[features]' header recognized"
) || FAILURES=$((FAILURES + 1))

(
  HOME="$(mktemp -d)"
  export HOME
  mkdir -p "$HOME/.codex"
  # Quoted key spells the same table: ["features"]
  printf '["features"]\nplugin_hooks = false\n' > "$HOME/.codex/config.toml"
  source "$SCRIPT"
  ensure_codex_plugin_hooks >/dev/null
  grep -qE '^plugin_hooks = true' "$HOME/.codex/config.toml" \
    || fail "plugin_hooks under '[\"features\"]' should be updated to true"
  [ "$(grep -c 'features' "$HOME/.codex/config.toml")" -eq 1 ] \
    || fail "no duplicate table for quoted-key header"
  pass "quoted-key '[\"features\"]' header recognized"
) || FAILURES=$((FAILURES + 1))

(
  HOME="$(mktemp -d)"
  export HOME
  mkdir -p "$HOME/.codex"
  # Single-quoted (literal-string) key spells the same table: ['features']
  printf "['features']\nplugin_hooks = false\n" > "$HOME/.codex/config.toml"
  source "$SCRIPT"
  ensure_codex_plugin_hooks >/dev/null
  grep -qE '^plugin_hooks = true' "$HOME/.codex/config.toml" \
    || fail "plugin_hooks under \"['features']\" should be updated to true"
  [ "$(grep -c 'features' "$HOME/.codex/config.toml")" -eq 1 ] \
    || fail "no duplicate table for single-quoted-key header"
  pass "single-quoted-key \"['features']\" header recognized"
) || FAILURES=$((FAILURES + 1))

(
  HOME="$(mktemp -d)"
  export HOME
  source "$SCRIPT"
  ensure_codex_plugin_hooks | grep -q "not found" \
    || fail "missing config.toml should print advisory and return 0"
  pass "missing config.toml is advisory, not fatal"
) || FAILURES=$((FAILURES + 1))

echo ""
if [ "$FAILURES" -gt 0 ]; then
  echo "$FAILURES test(s) failed"
  exit 1
fi
echo "All tests passed"
