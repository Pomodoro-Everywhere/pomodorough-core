import assert from "node:assert/strict";
import { existsSync, readFileSync, writeFileSync } from "node:fs";

const directory = process.env.PWA_SELECTION_EVIDENCE_DIR || "/var/folders/r_/_mr22dqn24d31b7460cz8z5m0000gn/T/opencode";
const source = JSON.parse(readFileSync(`${directory}/core-pwa-selection-source.json`, "utf8"));
const destination = new URL("../fixtures/pwa-selection-public-v1.json", import.meta.url);
assert.equal(existsSync(destination), false, "Exact public receipts cannot be replaced.");
const receipts = source.receipts.filter((receipt) => receipt.selected).map((receipt) => {
  for (const call of [receipt.selected, receipt.legacyCompletePlanner]) {
    assert.deepEqual(JSON.parse(call.inputRaw), call.input);
    assert.deepEqual(JSON.parse(call.envelopeRaw).value, call.completeReturn);
  }
  assert.deepEqual(receipt.selected.input.selection, receipt.rawRecordRead.completionState.selection);
  assert.deepEqual(receipt.selected.input.lifecycle, receipt.rawRecordRead.completionState.lifecycle);
  assert.equal(receipt.currentCall.inputRaw, receipt.frozenCall.inputRaw);
  assert.deepEqual(receipt.completeCurrentReturn, receipt.completeFrozenReturn);
  return receipt;
});
assert.equal(receipts.length, 2);
writeFileSync(destination, JSON.stringify({ hashes: source.hashes, receipts }, null, 2) + "\n");
console.log("Captured two exact public chooser requests, complete source returns, and persisted records.");
