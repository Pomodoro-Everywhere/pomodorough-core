import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { invoke } from "./aggregate_artifact/abi_host.mjs";
import { nativeResponses, assertParity } from "./aggregate_artifact/native_oracle.mjs";

const artifact = process.argv[2];
assert.ok(artifact, "usage: node tests/r55_c11_display_and_arrays.mjs <exact-release.wasm>");
const { rows } = JSON.parse(readFileSync(new URL("../fixtures/r55-c11-display-and-arrays-v1.json", import.meta.url)));
const { instance } = await WebAssembly.instantiate(readFileSync(artifact));

// Before R55-C11 the contents of a stored display context (missing profile,
// another profile, an extra field) and six top-level arrays answered with raw
// Serde text. error and order rows now pin the Core message; map rows pin the
// refusal of the enum map form {"pwaStorage": null} that Serde accepted; kept,
// pin and precedence rows (including the R55-C10 single-fault envelopes and
// double-fault precedence) keep their stack envelope; element and control rows
// are byte-identical.
const shaped = (message) => ({ error: `invalid shared-core input: ${message}`, ok: false });
const cases = rows.map((row) => {
  const expected = JSON.parse(row.expected);
  if (row.kind === "error" || row.kind === "order") {
    assert.deepEqual(expected, shaped(row.message), row.name);
    assert.match(row.before, /invalid shared-core JSON: /, `${row.name} leaked raw Serde text before R55-C11`);
  } else if (row.kind === "map") {
    assert.deepEqual(expected, shaped(row.message), row.name);
    assert.equal(JSON.parse(row.before).ok, true, `${row.name} was accepted before R55-C11`);
  } else {
    assert.equal(row.expected, row.before, row.name);
    if (row.message) assert.deepEqual(expected, shaped(row.message), row.name);
    assert.equal(expected.ok, row.kind === "control", row.name);
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

const kinds = ["control", "error", "map", "kept", "element", "order", "precedence", "pin"];
const count = (kind) => rows.filter((row) => row.kind === kind).length;
assert.deepEqual(kinds.map(count), [9, 30, 3, 2, 4, 1, 9, 4], "R55-C11 case matrix");
for (const operation of new Set(rows.map((row) => row.operation))) {
  assert.ok(rows.some((row) => row.kind === "control" && row.operation === operation), `R55-C11 ${operation} control`);
}
console.log(`R55-C11: ${rows.length} complete raw native/WASM parity calls passed (`
  + kinds.map((kind) => `${count(kind)} ${kind}`).join(", ") + ")");
