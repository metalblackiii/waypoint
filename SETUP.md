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

Each hook is a thin shell wrapper that delegates to the waypoint binary. Create these in `~/.claude/hooks/`:

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

**waypoint-pre-write.sh**
```sh
#!/usr/bin/env bash
WAYPOINT="${HOME}/.cargo/bin/waypoint"
[[ -x "$WAYPOINT" ]] || exit 0
INPUT=$(cat)
echo "$INPUT" | "$WAYPOINT" hook pre-write
```

**waypoint-post-write.sh**
```sh
#!/usr/bin/env bash
WAYPOINT="${HOME}/.cargo/bin/waypoint"
[[ -x "$WAYPOINT" ]] || exit 0
INPUT=$(cat)
echo "$INPUT" | "$WAYPOINT" hook post-write
```

**waypoint-post-failure.sh**
```sh
#!/usr/bin/env bash
WAYPOINT="${HOME}/.cargo/bin/waypoint"
[[ -x "$WAYPOINT" ]] || exit 0
INPUT=$(cat)
echo "$INPUT" | "$WAYPOINT" hook post-failure
```

Make them executable:

```sh
chmod +x ~/.claude/hooks/waypoint-*.sh
```

## 4. Register hooks in `~/.claude/settings.json`

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
      },
      {
        "matcher": "Edit|Write",
        "hooks": [{ "type": "command", "command": "~/.claude/hooks/waypoint-pre-write.sh" }]
      }
    ],
    "PostToolUse": [
      {
        "matcher": "Edit|Write",
        "hooks": [{ "type": "command", "command": "~/.claude/hooks/waypoint-post-write.sh" }]
      }
    ],
    "PostToolUseFailure": [
      {
        "matcher": "Edit|Write",
        "hooks": [{ "type": "command", "command": "~/.claude/hooks/waypoint-post-failure.sh" }]
      }
    ]
  }
}
```

## 5. Import the operating protocol

Add the Waypoint protocol to your global `~/.claude/CLAUDE.md` so Claude follows the knowledge store and trap logging rules in every project:

```markdown
@~/repos/waypoint/WAYPOINT.md
```

This assumes the waypoint repo is cloned at `~/repos/waypoint`. Adjust the path if yours differs. The `@` import resolves through symlinks, so it works even if your CLAUDE.md is deployed via symlink from a dotfiles repo.

## 6. First run

Open Claude Code in any project. The session-start hook auto-creates `.waypoint/` and runs the initial scan. Or run manually:

```sh
waypoint scan
```
