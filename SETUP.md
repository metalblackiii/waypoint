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

Make them executable (both locations):

```sh
chmod +x ~/.claude/hooks/waypoint-*.sh
chmod +x ~/.codex/hooks/waypoint-*.sh
```

If `~/.codex/hooks` is a symlink to `~/.claude/hooks`, either command is sufficient.

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

## 5. Add the minimal protocol to your global agent instructions

`WAYPOINT.md` is the single source of truth for the copy/paste template.
Copy the content of `WAYPOINT.md` into your global `AGENTS.md` (recommended for cross-agent portability).

If you prefer import-based sync, keep the template in `WAYPOINT.md` and add an `@` import to `~/.claude/CLAUDE.md`.

**Claude Code only (optional):** If you want auto-sync instead of copy/paste, add an `@` import in `~/.claude/CLAUDE.md`:

NOTE: Recommend installing rg (ripgrep) as it is more efficient than grep.  If not, update rules accordingly

```markdown
@/absolute/path/to/waypoint/WAYPOINT.md
```

Use your local absolute path to this repo.

## 6. First run

Open Claude Code or Codex in any project. The session-start hook auto-creates `.waypoint/` and runs the initial scan. Or run manually:

```sh
waypoint scan
```

To scan all repos at once:

```sh
waypoint scan --all /path/to/repos
```

When switching to a different repo (or when investigating another repo from your current cwd), run:

```sh
waypoint arch
# or from another repo:
waypoint arch -C /path/to/other-repo
```

This gives you the current language mix and hotspots before deeper reads.

## 7. Verify setup

Run these checks after setup:

```sh
waypoint --version
waypoint scan --check
waypoint status
```

Optional symbol check (for code repos with indexed symbols):

```sh
waypoint find "scan" --limit 5
# if find returns symbols, sketch one of them:
waypoint sketch <symbol-name-from-find-results>
```

Expected signals:

- `waypoint --version` prints a semver plus git short hash.
- `waypoint scan --check` exits successfully when the map is present and fresh.
- `waypoint status` reports map health for the current project.
- In code repos, `waypoint find "scan" --limit 5` usually returns symbols; in non-code repos it may return "No symbols found".
- If `find` returns symbols, `waypoint sketch <symbol-name-from-find-results>` returns file, line range, and signature.
- `waypoint arch` prints architecture context (`Languages`, and `Hotspots` when imports are present).

If a hook is misconfigured, open a new Claude/Codex session and confirm read operations include `[waypoint] map:` annotations.
