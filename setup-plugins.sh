#!/bin/bash
set -euo pipefail

REPO_DIR="$(cd "$(dirname "$0")" && pwd)"
SETUP_OK=true
UNINSTALL=false
if [[ "${1:-}" == "--uninstall" ]]; then
  UNINSTALL=true
fi

if $UNINSTALL; then
  echo "Waypoint Plugin Uninstall"
  echo "========================="
  echo ""

  # --- Claude Code ---
  if CLAUDE_CMD=$(command -v claude 2>/dev/null); then
    if "$CLAUDE_CMD" plugin marketplace list 2>/dev/null | grep -q waypoint-plugins; then
      "$CLAUDE_CMD" plugin marketplace remove waypoint-plugins 2>/dev/null || true
      echo "Removed Claude Code marketplace registration"
      echo "  Cached plugin can be removed with: claude plugin uninstall waypoint"
    else
      echo "Claude Code: no marketplace registration found"
    fi
  fi

  # --- Codex ---
  CODEX_PLUGINS_DIR="$HOME/.agents/plugins"
  if [ -f "$CODEX_PLUGINS_DIR/waypoint.json" ]; then
    rm "$CODEX_PLUGINS_DIR/waypoint.json"
    echo "Removed $CODEX_PLUGINS_DIR/waypoint.json"
  else
    echo "Codex: no waypoint marketplace file found"
  fi
  # plugin_hooks not reverted — it's a feature flag that may be used by other plugins

  echo ""
  echo "Done. Restart sessions to apply."
  exit 0
fi

echo "Waypoint Plugin Setup"
echo "====================="
echo ""
echo "Repo: $REPO_DIR"
echo ""

# --- Claude Code ---
# Detect claude binary only — 'cc' collides with the system C compiler
if CLAUDE_CMD=$(command -v claude 2>/dev/null); then
  echo "Claude Code detected ($CLAUDE_CMD)"

  if "$CLAUDE_CMD" plugin marketplace list 2>/dev/null | grep -q waypoint-plugins; then
    if ! "$CLAUDE_CMD" plugin marketplace update waypoint-plugins 2>/dev/null; then
      echo "  Update failed (stale path?). Re-registering..."
      "$CLAUDE_CMD" plugin marketplace remove waypoint-plugins 2>/dev/null || true
      "$CLAUDE_CMD" plugin marketplace add "$REPO_DIR" \
        || { echo "  Warning: marketplace re-registration failed — Claude setup incomplete"; SETUP_OK=false; }
    else
      echo "  Updated marketplace registration"
    fi
  else
    echo "  Registering marketplace..."
    "$CLAUDE_CMD" plugin marketplace add "$REPO_DIR" \
      || { echo "  Warning: marketplace registration failed — Claude setup incomplete"; SETUP_OK=false; }
  fi

  echo ""
  if $SETUP_OK; then
    echo "  Installing waypoint plugin..."
    "$CLAUDE_CMD" plugin install waypoint \
      || { echo "  (install manually: claude plugin install waypoint)"; SETUP_OK=false; }
  else
    echo "  Skipping plugin install — marketplace registration failed"
  fi
  echo ""
else
  echo "Claude Code not found — skipping."
  echo ""
fi

# --- Codex ---
CODEX_PLUGINS_DIR="$HOME/.agents/plugins"

if command -v codex &>/dev/null; then
  if ! command -v jq &>/dev/null; then
    echo "Codex detected, but jq is required for marketplace setup."
    echo "  Install jq: brew install jq"
    echo "  Then re-run: $REPO_DIR/setup-plugins.sh"
    echo ""
    SETUP_OK=false
  else
    echo "Codex detected"
    mkdir -p "$CODEX_PLUGINS_DIR"

    # Write waypoint-plugins marketplace with absolute plugin path resolved from repo root
    jq --arg repo "$REPO_DIR" '
      .plugins |= map(.source.path |= ($repo + "/" + ltrimstr("./")))
    ' "$REPO_DIR/.agents/plugins/marketplace.json" > "$CODEX_PLUGINS_DIR/waypoint.json"
    echo "  Wrote $CODEX_PLUGINS_DIR/waypoint.json"

    # Ensure plugin_hooks feature flag is enabled in ~/.codex/config.toml
    CODEX_CONFIG="$HOME/.codex/config.toml"
    if [ -f "$CODEX_CONFIG" ]; then
      if grep -qE "^\s*plugin_hooks\s*=\s*true" "$CODEX_CONFIG"; then
        echo "  plugin_hooks already enabled in config.toml"
      elif grep -qE "^\s*plugin_hooks\s*=" "$CODEX_CONFIG"; then
        # Present but set to false (or another value) — update in place
        awk '/^[[:space:]]*plugin_hooks[[:space:]]*=/{sub(/=.*/, "= true")}1' \
          "$CODEX_CONFIG" > "${CODEX_CONFIG}.tmp" \
          && mv "${CODEX_CONFIG}.tmp" "$CODEX_CONFIG"
        echo "  Updated plugin_hooks to true in config.toml"
      elif grep -q "^\[features\]" "$CODEX_CONFIG"; then
        awk '/^\[features\]/{print; print "plugin_hooks = true"; next}1' \
          "$CODEX_CONFIG" > "${CODEX_CONFIG}.tmp" \
          && mv "${CODEX_CONFIG}.tmp" "$CODEX_CONFIG"
        echo "  Added plugin_hooks = true under [features] in config.toml"
      else
        printf '\n[features]\nplugin_hooks = true\n' >> "$CODEX_CONFIG"
        echo "  Added [features] + plugin_hooks = true to config.toml"
      fi
    else
      echo "  ~/.codex/config.toml not found — add manually: [features] plugin_hooks = true"
    fi

    echo ""
    echo "  To install in Codex:"
    echo "    1. Open Codex from any project directory"
    echo "    2. Type /plugins, search 'Waypoint'"
    echo "    3. Install waypoint"
    echo "    Note: waypoint is marked INSTALLED_BY_DEFAULT and may install automatically."
    echo ""
  fi
else
  echo "Codex not found — skipping."
  echo ""
fi

if $SETUP_OK; then
  echo "Done. Restart Claude Code / Codex sessions to activate hooks."
else
  echo "Done (with warnings — review output above)."
  exit 1
fi
