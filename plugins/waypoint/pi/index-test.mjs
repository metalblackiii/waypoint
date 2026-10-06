import assert from "node:assert/strict";
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { after, test } from "node:test";

const fixtureDir = mkdtempSync(path.join(os.tmpdir(), "pi-waypoint-extension-"));
const binary = path.join(fixtureDir, "waypoint");
const sessionJson = JSON.stringify({ hookSpecificOutput: { additionalContext: "session digest" } });
const postWriteJson = JSON.stringify({ hookSpecificOutput: { additionalContext: "signature warning" } });
const absoluteTarget = JSON.stringify({ file_path: path.join(fixtureDir, "src/index.ts") }).slice(1, -1);
// The stub only warns when handed the absolute target, so the relative-path tests prove cwd resolution
writeFileSync(
	binary,
	`#!/usr/bin/env bash
INPUT=$(cat)
if [ "$2" = session-start ]; then printf '%s' '${sessionJson}'; exit 0; fi
case "$INPUT" in *'${absoluteTarget}'*) printf '%s' '${postWriteJson}' ;; esac
`,
);
chmodSync(binary, 0o755);
process.env.WAYPOINT_BINARY = binary;

const waypointExtension = (await import("./index.ts?test")).default;

after(() => {
	delete process.env.WAYPOINT_BINARY;
	rmSync(fixtureDir, { recursive: true, force: true });
});

function harness() {
	const handlers = {};
	const messages = [];
	waypointExtension({
		on: (event, handler) => {
			handlers[event] = handler;
		},
		sendMessage: (message, options) => messages.push({ message, options }),
	});
	return { handlers, messages };
}

function context() {
	return {
		cwd: fixtureDir,
		ui: { notify: () => {} },
	};
}

test("queues Waypoint session context", async () => {
	const { handlers, messages } = harness();
	await handlers.session_start({}, context());
	assert.deepEqual(messages, [
		{
			message: { customType: "waypoint", content: "session digest", display: false },
			options: { deliverAs: "nextTurn" },
		},
	]);
});

test("re-queues Waypoint session context after compaction", async () => {
	const { handlers, messages } = harness();
	await handlers.session_compact({ reason: "threshold" }, context());
	assert.deepEqual(messages, [
		{
			message: { customType: "waypoint", content: "session digest", display: false },
			options: { deliverAs: "nextTurn" },
		},
	]);
});

test("steers the digest into an overflow-compaction retry", async () => {
	const { handlers, messages } = harness();
	await handlers.session_compact({ reason: "overflow", willRetry: true }, context());
	assert.deepEqual(messages, [
		{
			message: { customType: "waypoint", content: "session digest", display: false },
			options: { deliverAs: "steer" },
		},
	]);
});

test("appends post-write context for an @-prefixed relative path", async () => {
	const { handlers } = harness();
	const updated = await handlers.tool_result(
		{ toolName: "write", input: { path: "@src/index.ts" }, content: [], isError: false },
		context(),
	);
	assert.deepEqual(updated, { content: [{ type: "text", text: "[Waypoint]\nsignature warning" }] });
});

test("ignores failed and non-writing tool results", async () => {
	const { handlers } = harness();
	const base = { input: { path: "src/index.ts" }, content: [{ type: "text", text: "x" }] };
	assert.equal(await handlers.tool_result({ ...base, toolName: "edit", isError: true }, context()), undefined);
	assert.equal(await handlers.tool_result({ ...base, toolName: "read", isError: false }, context()), undefined);
});

test("appends post-write context to a successful edit result", async () => {
	const { handlers } = harness();
	const updated = await handlers.tool_result(
		{
			toolName: "edit",
			input: { path: "src/index.ts" },
			content: [{ type: "text", text: "edited" }],
			isError: false,
		},
		context(),
	);
	assert.deepEqual(updated, {
		content: [
			{ type: "text", text: "edited" },
			{ type: "text", text: "[Waypoint]\nsignature warning" },
		],
	});
});

test("appends post-write context to a successful undo_last_edit result", async () => {
	const { handlers } = harness();
	const updated = await handlers.tool_result(
		{
			toolName: "undo_last_edit",
			input: { path: "src/index.ts" },
			content: [{ type: "text", text: "reverted" }],
			isError: false,
		},
		context(),
	);
	assert.deepEqual(updated, {
		content: [
			{ type: "text", text: "reverted" },
			{ type: "text", text: "[Waypoint]\nsignature warning" },
		],
	});
});

test("appends post-write context to a successful file-field edit result", async () => {
	const { handlers } = harness();
	const updated = await handlers.tool_result(
		{
			toolName: "edit",
			input: { file: "src/index.ts", edits: [{ anchor_from: "a1B", anchor_to: "a1B", replace_with: "x" }] },
			content: [{ type: "text", text: "edited" }],
			isError: false,
		},
		context(),
	);
	assert.deepEqual(updated, {
		content: [
			{ type: "text", text: "edited" },
			{ type: "text", text: "[Waypoint]\nsignature warning" },
		],
	});
});
