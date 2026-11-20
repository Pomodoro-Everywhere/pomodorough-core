import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { invoke } from "./aggregate_artifact/abi_host.mjs";
import { nativeResponses, assertParity } from "./aggregate_artifact/native_oracle.mjs";

const artifact = process.argv[2];
assert.ok(artifact, "usage: node tests/r55_c10_nested_last_intent.mjs <exact-release.wasm>");
const { rows } = JSON.parse(readFileSync(new URL("../fixtures/r55-c10-nested-last-intent-v1.json", import.meta.url)));
const { instance } = await WebAssembly.instantiate(readFileSync(artifact));

// Before R55-C10 bootstrap.workspacePlan.v1 answered a non-object lastIntent in
// either timer with raw Serde text, and workspace.legacyDependencyPlan.v1 did
// the same for a null display context. Error rows now pin the R55-C08 message
// "<path> must be a JSON object"; control rows are ok:true bases, unchanged;
// nullable rows (a null lastIntent or a null canonicalTimer) keep null meaning
// "absent" with their stack envelope.
const cases = rows.map((row) => {
  const expected = JSON.parse(row.expected);
  if (row.kind === "control") {
    assert.equal(expected.ok, true, row.name);
  } else if (row.kind === "nullable") {
    assert.equal(row.expected, row.before, row.name);
    assert.equal(expected.ok, true, row.name);
  } else {
    assert.deepEqual(expected, { error: `invalid shared-core input: ${row.path} must be a JSON object`, ok: false }, row.name);
    assert.match(row.before, /invalid shared-core JSON: /, `${row.name} leaked raw Serde text before R55-C10`);
  }
  return { name: row.name, operation: row.operation, input: row.input, ok: expected.ok, error: expected.error ?? null };
});
const native = nativeResponses(cases);
for (const [index, row] of rows.entries()) {
  let actual;
  try {
    actual = invoke(instance.exports, cases[index]);
  } catch (error) {
    assert.fail(`${row.name} trapped: ${error}`);
  }
  assertParity(cases[index], native[index], actual);
  assert.equal(actual, row.expected, `${row.name} pinned envelope`);
}

const count = (kind) => rows.filter((row) => row.kind === kind).length;
for (const operation of new Set(rows.map((row) => row.operation))) {
  assert.ok(rows.some((row) => row.kind === "control" && row.operation === operation), `R55-C10 ${operation} control`);
}
assert.deepEqual(
  ["control", "array", "number", "string", "nullable", "order", "null"].map(count),
  [2, 2, 2, 2, 4, 1, 1],
  "R55-C10 case matrix",
);
console.log(`R55-C10: ${rows.length} complete raw native/WASM parity calls passed (${count("array")} array, `
  + `${count("number")} number, ${count("string")} string, ${count("null")} null, ${count("nullable")} nullable, `
  + `${count("order")} order, ${count("control")} control)`);
