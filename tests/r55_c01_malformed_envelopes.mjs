import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { invoke } from "./aggregate_artifact/abi_host.mjs";
import { nativeResponses, assertParity } from "./aggregate_artifact/native_oracle.mjs";

const artifact = process.argv[2];
assert.ok(artifact, "usage: node tests/r55_c01_malformed_envelopes.mjs <exact-release.wasm>");
const { rows } = JSON.parse(readFileSync(new URL("../fixtures/r55-c01-malformed-envelopes-v1.json", import.meta.url)));
const { instance } = await WebAssembly.instantiate(readFileSync(artifact));
const REASONS = {
  completionWorkspace: "invalid shared-core input: workspace must be a JSON object",
  completionPreviousWorkspace: "invalid shared-core input: previousWorkspace must be a JSON object",
  dependencyQueues: "invalid shared-core input: missing raw legacy queue",
  preferenceIdentity: "invalid shared-core input: invalid retained legacy identity",
};

// Malformed rows unwound natively and trapped in WASM before R55-C01. Control
// rows pin the complete envelope captured before the fix.
const cases = rows.map((row) => {
  const expected = JSON.parse(row.expected);
  if (row.kind === "malformed") assert.deepEqual(expected, { error: REASONS[row.site], ok: false }, row.name);
  return { name: row.name, operation: row.operation, input: row.input, ok: expected.ok, error: expected.error ?? null };
});
const native = nativeResponses(cases);
const counts = {};
for (const [index, row] of rows.entries()) {
  let actual;
  try {
    actual = invoke(instance.exports, cases[index]);
  } catch (error) {
    assert.fail(`${row.name} trapped: ${error}`);
  }
  assertParity(cases[index], native[index], actual);
  assert.equal(actual, row.expected, `${row.name} pinned envelope`);
  const key = `${row.site}/${row.kind}`;
  counts[key] = (counts[key] ?? 0) + 1;
}

assert.deepEqual(counts, {
  "completionWorkspace/malformed": 12, "completionWorkspace/control": 10,
  "completionPreviousWorkspace/malformed": 8, "completionPreviousWorkspace/control": 5,
  "dependencyQueues/malformed": 8, "dependencyQueues/control": 7,
  "preferenceIdentity/malformed": 12, "preferenceIdentity/control": 7,
}, "R55-C01 case matrix");
console.log(`R55-C01: ${rows.length} complete raw native/WASM parity calls passed `
  + `(${rows.filter((row) => row.kind === "malformed").length} malformed, `
  + `${rows.filter((row) => row.kind === "control").length} control)`);
