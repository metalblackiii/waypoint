#!/usr/bin/env bash
# Waypoint — SubagentStart hook
# Delegates to `waypoint hook subagent-start` to deliver the command digest
# into each Task-tool subagent's own fresh context (SessionStart's output
# never reaches subagents — separate hook, separate context window).
# Invoked via hooks.json; ${CLAUDE_PLUGIN_ROOT} in that file is the plugin's
# installation directory, injected by the Claude Code plugin runtime.

WAYPOINT="${HOME}/.cargo/bin/waypoint"
if [[ ! -x "$WAYPOINT" ]]; then
  exit 0
fi

INPUT=$(cat)
printf '%s\n' "$INPUT" | "$WAYPOINT" hook subagent-start
