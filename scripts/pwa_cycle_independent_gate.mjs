import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
const directory = process.env.PWA_SELECTION_EVIDENCE_DIR || "/var/folders/r_/_mr22dqn24d31b7460cz8z5m0000gn/T/opencode";
const source = readFileSync(`${directory}/core-pwa12-independent-20261004/adversarial.mjs`, "utf8");
assert.equal(createHash("sha256").update(source).digest("hex"), "785373950266ae43f0b5216b1134088ccba7af7b28adb93daf6b026f653b8f4b");
const oldOutput = "new URL('adversarial-receipts.json',import.meta.url)", destination = `${directory}/core-pwa-cycle-independent.json`;
assert.equal(source.split(oldOutput).length, 2);
const result = spawnSync(process.execPath, ["--input-type=module", "--eval", source.replace(oldOutput, JSON.stringify(destination))], {
  cwd: new URL("../", import.meta.url), encoding: "utf8", timeout: 120000, maxBuffer: 16 * 1024 * 1024 });
assert.equal(result.status, 1, "The unchanged checker must reject its three old unbacked-discharge assumptions.");
const receipt = JSON.parse(readFileSync(destination, "utf8"));
const expected = ["presentation-versus-obligation-durable-finish", "wrong-consumed-identity-timerId", "wrong-consumed-identity-phase"];
assert.equal(receipt.summary.checks, 146); assert.equal(receipt.summary.passed, 143);
assert.deepEqual(receipt.summary.failures.map((item) => item.name), expected);
for (const failure of receipt.summary.failures) {
  assert.match(failure.error, /consumed Finish lacks raw or durable original evidence/);
  const call = receipt.receipts.find((item) => item.case === failure.name);
  assert.equal(call.envelope.ok, false); assert.deepEqual(Object.keys(call.envelope).sort(), ["error", "ok"]);
}
console.log("Unchanged independent checker preserves 143 checks and rejects exactly three unsupported fabricated markers. Original assertions and receipt remain intact.");
