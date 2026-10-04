import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";

const root = new URL("../../server/", import.meta.url);
const temp = process.env.PWA12_EVIDENCE_DIR || "/var/folders/r_/_mr22dqn24d31b7460cz8z5m0000gn/T/opencode";
const sha256 = createHash("sha256").update(readFileSync(new URL("web/pomodorough_core.wasm", root))).digest("hex");
assert.equal(sha256, "845090328b2f44056480c3930e9bb684a3874b8f3cbcbd4253ddd92f67c6f5d6");
const args = ["--test", "--test-name-pattern=naturally expired canonical timer", "web/p222-completion-atomicity.test.js"];
const red = spawnSync(process.execPath, args, { cwd: root, encoding: "utf8", timeout: 120000 });
assert.equal(red.status, 1); assert.match(red.stdout, /fail 4/);
const green = spawnSync(process.execPath, ["--require", new URL("natural_completion_native_bridge.cjs", import.meta.url).pathname, ...args],
  { cwd: root, encoding: "utf8", timeout: 120000 });
assert.equal(green.status, 0, green.stdout + green.stderr); assert.match(green.stdout, /pass 4/);
writeFileSync(`${temp}/core-pwa12-original-four-red-green.json`, JSON.stringify({ officialVersion: "0.45.0", officialSha256: sha256,
  baseline: { status: red.status, stdout: red.stdout, stderr: red.stderr },
  correctedNative: { status: green.status, stdout: green.stdout, stderr: green.stderr } }, null, 2));
console.log("Original four PWA tests: official 0.45 fails 4/4; unchanged public route with native Core passes 4/4.");
