# Waypoint Setup

## 1. Build and install the binary

Requires Rust 1.85+ (edition 2024).

```sh
cargo install --path .
# Installs to ~/.cargo/bin/waypoint
```

## 2. Add `.waypoint/` to your global gitignore

```sh
echo '.waypoint/' >> ~/.gitignore_global
# Or wherever your core.excludesfile points:
# git config --global core.excludesfile
```

## 3. Create the hook scripts

Each hook is a thin shell wrapper that delegates to the waypoint binary. Create these in `~/.claude/hooks/` (Claude Code) — Codex symlinks the same directory via `~/.codex/hooks`.

**waypoint-session-start.sh**
```sh
#!/usr/bin/env bash
WAYPOINT="${HOME}/.cargo/bin/waypoint"
[[ -x "$WAYPOINT" ]] || exit 0
INPUT=$(cat)
echo "$INPUT" | "$WAYPOINT" hook session-start
```

**waypoint-pre-read.sh**
```sh
#!/usr/bin/env bash
WAYPOINT="${HOME}/.cargo/bin/waypoint"
[[ -x "$WAYPOINT" ]] || exit 0
INPUT=$(cat)
echo "$INPUT" | "$WAYPOINT" hook pre-read
```

Make them executable:

```sh
chmod +x ~/.claude/hooks/waypoint-*.sh
```

## 4. Register hooks

### Claude Code — `~/.claude/settings.json`

Add these entries to the `hooks` object. Waypoint hooks should come **before** other hooks of the same type so context is available early.

```json
{
  "hooks": {
    "SessionStart": [
      {
        "matcher": "",
        "hooks": [{ "type": "command", "command": "~/.claude/hooks/waypoint-session-start.sh" }]
      }
    ],
    "PreToolUse": [
      {
        "matcher": "Read",
        "hooks": [{ "type": "command", "command": "~/.claude/hooks/waypoint-pre-read.sh" }]
      }
    ]
  }
}
```

### Codex — `~/.codex/hooks.json`

Codex uses the same hook scripts (via `~/.codex/hooks` → `~/.claude/hooks` symlink or direct copy). Requires `codex_hooks = true` in `~/.codex/config.toml`.

```json
{
  "hooks": {
    "SessionStart": [
      {
        "matcher": "",
        "hooks": [{ "type": "command", "command": "~/.codex/hooks/waypoint-session-start.sh" }]
      }
    ],
    "PreToolUse": [
      {
        "matcher": "Read",
        "hooks": [{ "type": "command", "command": "~/.codex/hooks/waypoint-pre-read.sh" }]
      }
    ]
  }
}
```

## 5. Add the operating protocol to your global agent instructions

Copy `WAYPOINT.md` as a `## Waypoint` section into your global `AGENTS.md` (or `~/.codex/AGENTS.md`). This is the recommended approach — it works across all agents (Claude Code, Codex, Cursor, etc.) and keeps the protocol in a single file you control.

```sh
cat WAYPOINT.md >> ~/.codex/AGENTS.md   # or wherever your global AGENTS.md lives
```

**Claude Code only:** If you use Claude Code exclusively, you can `@`-import instead of copying. Add to `~/.claude/CLAUDE.md`:

```markdown
@~/repos/waypoint/WAYPOINT.md
```

The `@` import stays in sync automatically when `WAYPOINT.md` updates, but only works in Claude Code.

## 6. First run

Open Claude Code or Codex in any project. The session-start hook auto-creates `.waypoint/` and runs the initial scan. Or run manually:

```sh
waypoint scan
```

To scan all repos at once:

```sh
waypoint scan --all ~/repos
```
