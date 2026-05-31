#!/usr/bin/env bash
# Waypoint — UserPromptSubmit hook
# Delegates to `waypoint hook user-prompt-submit` for the callers/impact nudge.
# Invoked via hooks.json; ${CLAUDE_PLUGIN_ROOT} in that file is the plugin's
# installation directory, injected by the Claude Code plugin runtime.

WAYPOINT="${HOME}/.cargo/bin/waypoint"
if [[ ! -x "$WAYPOINT" ]]; then
  exit 0
fi

INPUT=$(cat)
printf '%s\n' "$INPUT" | "$WAYPOINT" hook user-prompt-submit
