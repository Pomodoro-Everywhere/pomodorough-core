import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";

const temp = process.env.PWA12_EVIDENCE_DIR || "/var/folders/r_/_mr22dqn24d31b7460cz8z5m0000gn/T/opencode";
const fixture = JSON.parse(readFileSync(new URL("../fixtures/pwa-natural-checker-v1.json", import.meta.url), "utf8"));
assert.equal(fixture.cases.length, 12);
const result = spawnSync("rustup", ["run", "1.97.1", "cargo", "run", "--quiet", "--locked", "--example", "artifact_parity_oracle"], {
  cwd: new URL("../", import.meta.url), encoding: "utf8", timeout: 120000, maxBuffer: 16 * 1024 * 1024,
  input: fixture.cases.map((item) => JSON.stringify({ operation: item.operation, input: item.inputRaw })).join("\n") + "\n" });
assert.equal(result.status, 0, result.stderr);
const responses = result.stdout.trimEnd().split("\n");
assert.equal(responses.length, fixture.cases.length);
const receipts = fixture.cases.map((item, index) => {
  const input = JSON.parse(item.inputRaw), corrected = JSON.parse(responses[index]);
  assert.equal(item.priorReturn.ok, true);
  assert.deepEqual(JSON.parse(item.priorEnvelopeRaw), item.priorReturn);
  if (item.name.startsWith("late-ack-")) {
    assert.equal(corrected.ok, true, corrected.error);
    assert.equal(item.priorReturn.value.selection.phase, "focus");
    assert.deepEqual(corrected.value.selection, input.selection);
    assert.deepEqual(corrected.value.lifecycle, input.lifecycle);
  } else {
    assert.equal(corrected.ok, false, item.name);
    assert.deepEqual(Object.keys(corrected).sort(), ["error", "ok"]);
  }
  return { ...item, correctedEnvelopeRaw: responses[index], correctedReturn: corrected };
});
writeFileSync(`${temp}/core-pwa12-checker-red-green.json`, JSON.stringify({ red: 12, green: 12, receipts }, null, 2));
console.log("12 independent checker failures preserved; corrected native passes 12/12, with no partial error values.");
