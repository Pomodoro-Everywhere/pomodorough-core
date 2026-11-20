import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { invoke } from "./aggregate_artifact/abi_host.mjs";
import { nativeResponses, assertParity } from "./aggregate_artifact/native_oracle.mjs";

const artifact = process.argv[2];
assert.ok(artifact, "usage: node tests/r55_c05_non_object_roots.mjs <exact-release.wasm>");
const { rows } = JSON.parse(readFileSync(new URL("../fixtures/r55-c05-non-object-roots-v1.json", import.meta.url)));
const { instance } = await WebAssembly.instantiate(readFileSync(artifact));

// Before R55-C05 these roots returned "invalid shared-core input:  must be a
// JSON object" (empty path). Control rows pin the complete envelope captured
// before the fix.
const ROOT_NAMES = {
  "workspace.completionMutation.v1": "completion mutation",
  "workspace.readModel.v1": "read model",
  "workspace.ownershipPlan.v1": "ownership plan",
};
const ROOTS = ["[]", "[{}]", "\"x\"", "7", "true", "null"];

const cases = rows.map((row) => {
  const expected = JSON.parse(row.expected);
  if (row.kind === "root") {
    assert.deepEqual(expected, {
      error: `invalid shared-core input: ${ROOT_NAMES[row.operation]} must be a JSON object`, ok: false,
    }, row.name);
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

for (const operation of Object.keys(ROOT_NAMES)) {
  const inputs = rows.filter((row) => row.kind === "root" && row.operation === operation).map((row) => row.input);
  assert.deepEqual(inputs.sort(), [...ROOTS].sort(), `R55-C05 ${operation} root matrix`);
}
const controls = rows.filter((row) => row.kind === "control").length;
assert.deepEqual([rows.length, controls], [37, 19], "R55-C05 case matrix");
console.log(`R55-C05: ${rows.length} complete raw native/WASM parity calls passed `
  + `(${rows.length - controls} root, ${controls} control)`);
