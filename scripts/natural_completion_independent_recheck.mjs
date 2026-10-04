import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";

const temp = process.env.PWA12_EVIDENCE_DIR || "/var/folders/r_/_mr22dqn24d31b7460cz8z5m0000gn/T/opencode";
const source = readFileSync(`${temp}/core-pwa12-independent-20261004/adversarial.mjs`, "utf8");
const oldDestination = "new URL('adversarial-receipts.json',import.meta.url)";
assert.equal(source.split(oldDestination).length, 2);
const destination = `${temp}/core-pwa12-independent-recheck.json`;
// Preserve all independent assertions and requests. Redirect only its output.
const code = source.replace(oldDestination, JSON.stringify(destination));
const result = spawnSync(process.execPath, ["--input-type=module", "--eval", code], {
  cwd: new URL("../", import.meta.url), encoding: "utf8", timeout: 120000, maxBuffer: 16 * 1024 * 1024 });
assert.equal(result.status, 0, result.stdout + result.stderr);
const evidence = JSON.parse(readFileSync(destination, "utf8"));
assert.equal(evidence.summary.checks, 146);
assert.equal(evidence.summary.passed, 146);
assert.deepEqual(evidence.summary.failures, []);
console.log(`Unchanged independent checker passes 146/146; source SHA256 ${createHash("sha256").update(source).digest("hex")}.`);
