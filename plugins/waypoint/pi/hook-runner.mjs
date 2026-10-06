/**
 * Spawns `waypoint hook <name>` with the Claude hook stdin contract and
 * parses hookSpecificOutput.additionalContext.
 *
 * WARNING: pi.exec hardcodes stdin to "ignore", so this uses node:child_process
 * to feed the hook its JSON payload.
 */

import { accessSync, constants } from "node:fs";
import { spawn } from "node:child_process";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const UNICODE_SPACES = /[\u00A0\u2000-\u200A\u202F\u205F\u3000]/g;

/**
 * Resolve a tool's file argument to the absolute path pi's write/edit tools actually touched.
 *
 * SHORTCUT: mirrors pi 1.0.2's unexported `resolveToCwd` (dist/core/tools/path-utils.js) minus its
 * Windows drive rewrite. Re-sync when pi changes that function; a drifted copy makes post-write
 * skip edits silently.
 */
export function resolveToolPath(filePath, cwd) {
	let normalized = filePath.replace(UNICODE_SPACES, " ");
	if (normalized.startsWith("@")) normalized = normalized.slice(1);
	if (normalized === "~") return os.homedir();
	if (normalized.startsWith("~/")) normalized = path.join(os.homedir(), normalized.slice(2));
	else if (normalized.startsWith("file://")) normalized = fileURLToPath(normalized);
	return path.resolve(cwd, normalized);
}

export const DEFAULT_HOOK_TIMEOUT_MS = 5000;

// WHY: same binary the plugin's shell hooks run, so all three agents index with one build
const DEFAULT_BINARY = path.join(os.homedir(), ".cargo", "bin", "waypoint");

function isExecutable(file) {
	try {
		accessSync(file, constants.X_OK);
		return true;
	} catch {
		return false;
	}
}

export function runWaypointHook(
	hookName,
	payload,
	cwd,
	{ binary = process.env.WAYPOINT_BINARY ?? DEFAULT_BINARY, timeoutMs = DEFAULT_HOOK_TIMEOUT_MS } = {},
) {
	// Matches the shell hooks' `[[ -x "$WAYPOINT" ]] || exit 0`: an uninstalled waypoint is a no-op, not a failure
	if (!isExecutable(binary)) return Promise.resolve({ ok: true, context: "" });

	return new Promise((resolve) => {
		let settled = false;
		let stdout = "";
		const finish = (outcome) => {
			if (settled) return;
			settled = true;
			resolve(outcome);
		};
		let child;
		try {
			child = spawn(binary, ["hook", hookName], {
				cwd,
				detached: true,
				stdio: ["pipe", "pipe", "ignore"],
				env: process.env,
			});
		} catch {
			finish({ ok: false, context: "" });
			return;
		}

		const killGroup = () => {
			if (!child.pid) return;
			try {
				process.kill(-child.pid, "SIGKILL");
			} catch (error) {
				if (error?.code !== "ESRCH") child.kill("SIGKILL");
			}
		};
		const killTimer = setTimeout(() => {
			killGroup();
			child.kill("SIGKILL");
			child.stdout.destroy();
			child.stdin.destroy();
			finish({ ok: false, context: "" });
		}, timeoutMs);

		child.stdout.on("data", (chunk) => {
			stdout += chunk.toString();
		});
		child.stdin.on("error", () => {});
		child.on("error", () => {
			clearTimeout(killTimer);
			finish({ ok: false, context: "" });
		});
		child.on("close", (code) => {
			clearTimeout(killTimer);
			if (settled || code !== 0) return finish({ ok: false, context: "" });
			const output = stdout.trim();
			if (!output) return finish({ ok: true, context: "" });
			try {
				const hookSpecific = JSON.parse(output).hookSpecificOutput ?? {};
				const context = hookSpecific.additionalContext ?? "";
				if (typeof context !== "string") return finish({ ok: false, context: "" });
				finish({ ok: true, context });
			} catch {
				finish({ ok: false, context: "" });
			}
		});

		child.stdin.write(JSON.stringify(payload));
		child.stdin.end();
	});
}
