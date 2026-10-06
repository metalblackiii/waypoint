import assert from "node:assert/strict";
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { after, test } from "node:test";

import { resolveToolPath, runWaypointHook } from "./hook-runner.mjs";

test("resolves tool paths the way pi's write/edit tools do", () => {
	const cwd = "/work/repo";
	const home = os.homedir();
	const cases = [
		["src/a.ts", "/work/repo/src/a.ts"],
		["/abs/b.ts", "/abs/b.ts"],
		["@src/c.ts", "/work/repo/src/c.ts"],
		["@/abs/d.ts", "/abs/d.ts"],
		["~/e.ts", path.join(home, "e.ts")],
		["@~/f.ts", path.join(home, "f.ts")],
		["~", home],
		["file:///abs/g.ts", "/abs/g.ts"],
		["src/h i.ts", "/work/repo/src/h i.ts"],
		["../j.ts", "/work/j.ts"],
	];
	for (const [input, expected] of cases) assert.equal(resolveToolPath(input, cwd), expected, input);
});

const fixtureDir = mkdtempSync(path.join(os.tmpdir(), "pi-waypoint-hook-"));
after(() => rmSync(fixtureDir, { recursive: true, force: true }));

let stubCount = 0;
function stubHook(body) {
	const scriptPath = path.join(fixtureDir, `stub-${stubCount++}.sh`);
	writeFileSync(scriptPath, `#!/usr/bin/env bash\n${body}\n`);
	chmodSync(scriptPath, 0o755);
	return scriptPath;
}

function hookJson(context = "") {
	return JSON.stringify({ hookSpecificOutput: { additionalContext: context } });
}

test("runs a waypoint hook with JSON stdin and returns additional context", async () => {
	const script = stubHook(
		[
			"INPUT=$(cat)",
			`case "$INPUT" in *'\"cwd\":\"/workspace\"'*) printf '%s' '${hookJson("waypoint context")}' ;; *) exit 4 ;; esac`,
		].join("\n"),
	);
	assert.deepEqual(await runWaypointHook("session-start", { cwd: "/workspace" }, fixtureDir, { binary: script }), {
		ok: true,
		context: "waypoint context",
	});
});

test("empty hook output is a successful no-op", async () => {
	const script = stubHook("exit 0");
	assert.deepEqual(await runWaypointHook("post-write", {}, fixtureDir, { binary: script }), {
		ok: true,
		context: "",
	});
});

test("missing binary is a silent no-op, like the shell hooks", async () => {
	assert.deepEqual(await runWaypointHook("session-start", {}, fixtureDir, { binary: path.join(fixtureDir, "missing") }), {
		ok: true,
		context: "",
	});
});

test("malformed output, nonzero exit, and timeout fail closed", async () => {
	const malformed = stubHook("printf '%s' 'not json'");
	const crashed = stubHook("exit 2");
	const hanging = stubHook("sleep 10");

	assert.equal((await runWaypointHook("session-start", {}, fixtureDir, { binary: malformed })).ok, false);
	assert.equal((await runWaypointHook("session-start", {}, fixtureDir, { binary: crashed })).ok, false);
	assert.equal(
		(await runWaypointHook("session-start", {}, fixtureDir, { binary: hanging, timeoutMs: 100 })).ok,
		false,
	);
});
