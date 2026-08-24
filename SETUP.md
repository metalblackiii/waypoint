# Waypoint Setup

## 1. Install Rust

Waypoint requires Rust 1.85+ (edition 2024). If you don't have Rust installed (or need to upgrade):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Follow the prompts, then restart your shell or run `source ~/.cargo/env`. Verify with `rustc --version`.

## 2. Build and install the binary

```sh
cargo install --path .
# Installs to ~/.cargo/bin/waypoint
```

## 3. Add `.waypoint/` to your global gitignore

```sh
echo '.waypoint/' >> ~/.gitignore_global
# Or wherever your core.excludesfile points:
# git config --global core.excludesfile
```

## 4. Register the plugin

`setup-plugins.sh` handles hook registration for both Claude Code and Codex in one step. Run it once after `cargo install`:

```sh
./setup-plugins.sh
```

What it does:
- **Claude Code** — registers the `waypoint-plugins` marketplace and installs `waypoint@waypoint-plugins` (hooks fire automatically).
- **Codex** — registers the marketplace and installs the plugin via the `codex plugin` CLI, and ensures `plugin_hooks = true` in `~/.codex/config.toml`. A legacy hand-written `~/.agents/plugins/waypoint.json` from older versions of this script is removed automatically.

Dev iteration: `./refresh-plugins.sh` reloads the installed plugin from the current checkout (uninstall/reinstall for Claude Code, in-place cache re-sync for Codex) — no version bump needed.

To uninstall: `./setup-plugins.sh --uninstall`

After running, restart Claude Code / Codex sessions to activate the hooks.

### Alternative: manual hook setup

Skip this if you used `setup-plugins.sh`. For environments without the `claude` binary or where you need fine-grained hook control:

Create these scripts in `~/.claude/hooks/`:

**waypoint-session-start.sh**
```sh
#!/usr/bin/env bash
WAYPOINT="${HOME}/.cargo/bin/waypoint"
[[ -x "$WAYPOINT" ]] || exit 0
INPUT=$(cat)
printf '%s\n' "$INPUT" | "$WAYPOINT" hook session-start
```

**waypoint-post-write.sh**
```sh
#!/usr/bin/env bash
WAYPOINT="${HOME}/.cargo/bin/waypoint"
[[ -x "$WAYPOINT" ]] || exit 0
INPUT=$(cat)
printf '%s\n' "$INPUT" | "$WAYPOINT" hook post-write
```

**waypoint-subagent-start.sh**
```sh
#!/usr/bin/env bash
WAYPOINT="${HOME}/.cargo/bin/waypoint"
[[ -x "$WAYPOINT" ]] || exit 0
INPUT=$(cat)
printf '%s\n' "$INPUT" | "$WAYPOINT" hook subagent-start
```

Make them executable:

```sh
chmod +x ~/.claude/hooks/waypoint-*.sh
```

Then add to **`~/.claude/settings.json`** (hooks should come **before** other hooks of the same type):

```json
{
  "hooks": {
    "SessionStart": [
      {
        "matcher": "",
        "hooks": [{ "type": "command", "command": "~/.claude/hooks/waypoint-session-start.sh" }]
      }
    ],
    "PostToolUse": [
      {
        "matcher": "Edit|Write",
        "hooks": [{ "type": "command", "command": "~/.claude/hooks/waypoint-post-write.sh" }]
      }
    ],
    "SubagentStart": [
      {
        "matcher": "",
        "hooks": [{ "type": "command", "command": "~/.claude/hooks/waypoint-subagent-start.sh" }]
      }
    ]
  }
}
```

For **Codex**, the manual path uses a raw `~/.codex/hooks.json` (different from the plugin-based mechanism `setup-plugins.sh` uses). Copy the same hook scripts to `~/.codex/hooks/`, make them executable, and add the equivalent JSON to `~/.codex/hooks.json` pointing at those paths. Also ensure `plugin_hooks = true` under `[features]` in `~/.codex/config.toml`.

## 5. Agent instructions — none needed

Hooks are the sole steering surface: SessionStart injects a command digest
every session, SubagentStart delivers it to built-in subagents (Explore/Plan)
that never load `CLAUDE.md`/`AGENTS.md`, and PostToolUse keeps the map/symbol
index current in the background. Do not add waypoint guidance to
`AGENTS.md`/`CLAUDE.md` — duplicating it there muddies install/uninstall as
an on/off boundary.

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
```

Expected signals:

- `waypoint --version` prints a semver plus git short hash.
- `waypoint scan --check` exits successfully when the map is present and fresh.
- `waypoint status` reports map health for the current project.
- In code repos, `waypoint find "scan" --limit 5` usually returns symbols; in non-code repos it may return "No symbols found".
- `waypoint arch` prints architecture context (`Languages`, and `Hotspots` when imports are present).

If a hook is misconfigured, open a new Claude/Codex session and confirm the session-start message includes `[waypoint] arch:` context (large repos) or that `waypoint status` reports a healthy map.
