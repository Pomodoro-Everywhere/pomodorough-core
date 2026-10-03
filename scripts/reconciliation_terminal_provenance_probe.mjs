import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { retainedIntentCases } from "../tests/aggregate_artifact/terminal_provenance_cases.mjs";
import { assertSemantics } from "../tests/aggregate_artifact/semantics.mjs";

const [oracle, phase] = process.argv.slice(2);
assert.ok(oracle && ["red", "green"].includes(phase), "oracle path and red/green phase required");
const cases = [...retainedIntentCases(), ...retainedIntentCases("workspace.project.v1")];
const input = cases.map(({ operation, input }) => JSON.stringify({ operation, input })).join("\n") + "\n";
const result = spawnSync(oracle, [], { input, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
assert.ifError(result.error);
assert.equal(result.status, 0, result.stderr);
const output = result.stdout.trimEnd().split("\n");
assert.equal(output.length, cases.length);
const returns = cases.map((item, index) => {
  const envelope = JSON.parse(output[index]);
  const expected = phase === "red" ? true : item.ok;
  assert.equal(envelope.ok, expected, `${item.operation}/${item.name}: ${envelope.error}`);
  if (item.ok) assertSemantics(item, envelope.value);
  else if (phase === "green") assert.equal(envelope.error,
    "invalid shared-core input: conflicting workspace terminal timer/history");
  return { operation: item.operation, name: item.name, input: JSON.parse(item.input), envelope, rawEnvelope: output[index] };
});
const receipt = { phase, negativeScenarios: 6, negativeDispatches: 12, controlDispatches: 12,
  bypasses: phase === "red" ? 12 : 0, rejected: phase === "green" ? 12 : 0, returns };
if (process.env.CORE_PROVENANCE_RECEIPT) writeFileSync(process.env.CORE_PROVENANCE_RECEIPT, JSON.stringify(receipt, null, 2));
console.log(JSON.stringify({ ...receipt, returns: undefined }));
