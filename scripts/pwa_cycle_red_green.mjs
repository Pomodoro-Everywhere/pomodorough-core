import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
const directory = process.env.PWA_SELECTION_EVIDENCE_DIR || "/var/folders/r_/_mr22dqn24d31b7460cz8z5m0000gn/T/opencode";
const fixture = JSON.parse(readFileSync(new URL("../fixtures/pwa-cycle-public-v1.json", import.meta.url), "utf8"));
const receipts = [];
function dispatch(operation, input, old) {
  const inputRaw = JSON.stringify(input), executable = old ? `${directory}/core-pwa-cycle-rejected/debug/examples/artifact_parity_oracle`
    : new URL("../target/debug/examples/artifact_parity_oracle", import.meta.url).pathname;
  const result = spawnSync(executable, [], { input: JSON.stringify({ operation, input: inputRaw }) + "\n", encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  const envelopeRaw = result.stdout.trimEnd(), envelope = JSON.parse(envelopeRaw);
  return { inputRaw, envelopeRaw, envelope };
}
for (const receipt of fixture.receipts.filter((receipt) => receipt.start && !receipt.during)) {
  for (const field of ["start", "read"]) {
    const input = structuredClone(receipt[field].input); delete input.lifecycle.finishEvidence;
    const operation = receipt[field].operation, old = dispatch(operation, input, true), current = dispatch(operation, input, false);
    assert.equal(old.envelope.ok, true); assert.equal(current.envelope.ok, true);
    if (field === "start") { assert.equal(old.envelope.value.selection.explicit, true); assert.equal(current.envelope.value.selection.explicit, false); }
    else {
      assert.ok(!old.envelope.value.availableIntents.includes("finish")); assert.ok(current.envelope.value.availableIntents.includes("finish"));
      assert.equal(current.envelope.value.display.phase, receipt.model.display.phase);
    }
    assert.equal(old.inputRaw, current.inputRaw);
    receipts.push({ defect: field === "start" ? "choice-outlives-cycle" : "old-terminal-hides-new-expiry", phase: receipt.phase, old, current });
  }
}
for (const receipt of fixture.receipts.filter((receipt) => receipt.case.startsWith("invented-"))) {
  const call = receipt.calls.find((call) => call.operation === "workspace.completionMutation.v1");
  const old = dispatch(call.operation, call.input, true), current = dispatch(call.operation, call.input, false);
  assert.equal(old.envelope.ok, true); assert.equal(old.envelope.value.reason, "alreadyConsumed");
  assert.equal(current.envelope.ok, false); assert.deepEqual(Object.keys(current.envelope).sort(), ["error", "ok"]);
  assert.match(current.envelope.error, /consumed Finish lacks/);
  receipts.push({ defect: "fabricated-discharge", owner: receipt.case, old, current });
}
writeFileSync(`${directory}/core-pwa-cycle-red-green.json`, JSON.stringify({ receipts }, null, 2));
console.log("10 exact baseline defects fail the rejected branch and pass the repaired native authority.");
