import assert from "node:assert/strict";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";

const temp = process.env.PWA12_EVIDENCE_DIR || "/var/folders/r_/_mr22dqn24d31b7460cz8z5m0000gn/T/opencode";
const destination = new URL("../fixtures/pwa-natural-checker-v1.json", import.meta.url);
assert.equal(process.argv[2], "--capture");
assert.equal(existsSync(destination), false, "checker baseline cannot be refreshed");
const original = JSON.parse(readFileSync(`${temp}/core-pwa12-independent-20261004/adversarial-receipts.json`, "utf8"));
const failures = original.summary.failures.map((item) => item.name);
assert.equal(failures.length, 12);
const cases = failures.map((name) => {
  const receipt = original.receipts.findLast((item) => item.case === name);
  assert.ok(receipt);
  assert.deepEqual(JSON.parse(receipt.envelopeRaw), receipt.envelope);
  return { name, operation: receipt.operation, inputRaw: receipt.inputRaw,
    priorEnvelopeRaw: receipt.envelopeRaw, priorReturn: receipt.envelope };
});
const result = spawnSync("rustup", ["run", "1.97.1", "cargo", "run", "--quiet", "--locked", "--example", "artifact_parity_oracle"], {
  cwd: new URL("../", import.meta.url), encoding: "utf8", timeout: 120000, maxBuffer: 16 * 1024 * 1024,
  input: cases.map((item) => JSON.stringify({ operation: item.operation, input: item.inputRaw })).join("\n") + "\n" });
assert.equal(result.status, 0, result.stderr);
assert.deepEqual(result.stdout.trimEnd().split("\n"), cases.map((item) => item.priorEnvelopeRaw));
writeFileSync(destination, JSON.stringify({ checker: "CORE-PWA12 independent 2026-10-04", cases }, null, 2) + "\n");
console.log("Preserved 12 exact independent checker failures and complete prior native envelopes.");
