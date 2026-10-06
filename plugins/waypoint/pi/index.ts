/**
 * Adapts Waypoint's Claude hook commands to Pi's extension lifecycle.
 *
 * SessionStart context is queued as a custom message on session start and
 * again after compaction. Successful write/edit/undo results are sent through
 * Waypoint's post-write hook so exported-signature warnings still reach the
 * model. Pi has no SubagentStart event, so subagents get no digest from here.
 */

import type { ExtensionAPI, ExtensionContext } from "@earendil-works/pi-coding-agent";

import { resolveToolPath, runWaypointHook } from "./hook-runner.mjs";

// WARNING: undo_last_edit (pi-better-edit) rewrites a file too; a writing tool missing here leaves the index stale silently
const WRITING_TOOLS = new Set(["write", "edit", "undo_last_edit"]);

export default function waypoint(pi: ExtensionAPI) {
	let warnedHookFailure = false;

	const warnHookFailure = (ctx: ExtensionContext) => {
		if (warnedHookFailure) return;
		warnedHookFailure = true;
		ctx.ui.notify("Waypoint sync skipped: the hook could not complete; the existing index may be stale", "warning");
	};

	const queueSessionDigest = async (ctx: ExtensionContext, deliverAs: "nextTurn" | "steer") => {
		const hook = await runWaypointHook("session-start", { cwd: ctx.cwd }, ctx.cwd);
		if (!hook.ok) warnHookFailure(ctx);
		if (!hook.context) return;
		pi.sendMessage({ customType: "waypoint", content: hook.context, display: false }, { deliverAs });
	};

	pi.on("session_start", async (_event, ctx) => {
		await queueSessionDigest(ctx, "nextTurn");
	});

	pi.on("session_compact", async (event, ctx) => {
		// WARNING: overflow compaction (willRetry) resumes via agent.continue(), which drains steered
		// messages but never nextTurn ones; steering a settled run instead would buy an extra LLM turn
		await queueSessionDigest(ctx, event.willRetry ? "steer" : "nextTurn");
	});

	pi.on("tool_result", async (event, ctx) => {
		if (!WRITING_TOOLS.has(event.toolName) || event.isError) return undefined;
		// WHY: pi-better-edit's edit names its target `file`; without the fallback post-write skips silently
		const filePath = event.input.path ?? event.input.file;
		if (typeof filePath !== "string" || !filePath) return undefined;

		const hook = await runWaypointHook(
			"post-write",
			{
				cwd: ctx.cwd,
				tool_name: event.toolName,
				// WHY: pi tools accept relative, ~, and @ paths; post-write strips an absolute project root and skips anything else
				tool_input: { file_path: resolveToolPath(filePath, ctx.cwd) },
			},
			ctx.cwd,
		);
		if (!hook.ok) {
			warnHookFailure(ctx);
			return undefined;
		}
		if (!hook.context) return undefined;

		return {
			content: [...event.content, { type: "text", text: `[Waypoint]\n${hook.context}` }],
		};
	});
}
