import assert from "node:assert/strict";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
const directory = process.env.PWA_SELECTION_EVIDENCE_DIR || "/var/folders/r_/_mr22dqn24d31b7460cz8z5m0000gn/T/opencode";
const source = JSON.parse(readFileSync(`${directory}/core-pwa-cycle-source.json`, "utf8"));
const destination = new URL("../fixtures/pwa-cycle-public-v1.json", import.meta.url);
assert.equal(existsSync(destination), false, "Exact cycle observations cannot be replaced.");
function cycle(receipt) {
  const { phase, count, during, before, chosenRecords, choice, start, started, read, model, finish, after, deadline } = receipt;
  for (const call of [choice, start, read, finish]) {
    assert.deepEqual(JSON.parse(call.inputRaw), call.input);
    assert.deepEqual(JSON.parse(call.envelopeRaw).value, call.completeReturn);
  }
  const record = chosenRecords.meta.find((row) => row.key === "completionState").value;
  assert.deepEqual(start.input.selection, record.selection);
  assert.deepEqual(start.input.lifecycle, record.lifecycle);
  assert.deepEqual(before.meta.find((row) => row.key === "snapshot"), after.meta.find((row) => row.key === "snapshot"));
  return { phase, count, during, before, chosenRecords, choice, start, started, read, model, finish, after, deadline };
}
const receipts = source.receipts.map((receipt) => receipt.first ? { case: receipt.case,
  first: cycle(receipt.first), second: cycle(receipt.second) } : receipt.start ? { case: receipt.case, ...cycle(receipt) }
  : { case: receipt.case, before: receipt.before, after: receipt.after, publicActionReturn: receipt.publicActionReturn,
    calls: receipt.calls.filter((call) => call.error) });
assert.equal(receipts.length, 11);
writeFileSync(destination, JSON.stringify({ hashes: source.hashes, receipts }, null, 2) + "\n");
console.log("Captured all phase cycles, two successive public Focus cycles, and four failed discharge transactions.");
