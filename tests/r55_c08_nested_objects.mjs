import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { invoke } from "./aggregate_artifact/abi_host.mjs";
import { nativeResponses, assertParity } from "./aggregate_artifact/native_oracle.mjs";

const artifact = process.argv[2];
assert.ok(artifact, "usage: node tests/r55_c08_nested_objects.mjs <exact-release.wasm>");
const { rows } = JSON.parse(readFileSync(new URL("../fixtures/r55-c08-nested-objects-v1.json", import.meta.url)));
const { instance } = await WebAssembly.instantiate(readFileSync(artifact));

// Before R55-C08 Serde read nested structs from a positional JSON array (many
// such rows returned ok:true) and answered a nested null or number with raw
// Serde text. Each non-control row replaces one nested object of an ok:true
// base; control rows are the bases, with their complete envelope unchanged.
// Tolerant rows cover positions that skip a malformed persisted value.
// Nullable rows set an optional position to null, which still means "absent":
// they keep their c07 envelope and never get the C08 message.
const cases = rows.map((row) => {
  const expected = JSON.parse(row.expected);
  if (row.kind === "control" || row.kind === "tolerant") {
    assert.equal(expected.ok, true, row.name);
  } else if (row.kind === "nullable") {
    assert.equal(row.expected, row.before, row.name);
    assert.ok(!(expected.error ?? "").includes(`${row.path} must be a JSON object`), row.name);
  } else {
    assert.deepEqual(expected, { error: `invalid shared-core input: ${row.path} must be a JSON object`, ok: false }, row.name);
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
const arrayPaths = new Set(rows.filter((row) => row.kind === "array").map((row) => `${row.operation} ${row.path}`));
assert.equal(arrayPaths.size, count("array"), "R55-C08 one array row per nested path");
for (const operation of new Set(rows.map((row) => row.operation))) {
  assert.ok(rows.some((row) => row.kind === "control" && row.operation === operation), `R55-C08 ${operation} control`);
}
assert.deepEqual([count("array"), count("null"), count("number"), count("tolerant"), count("nullable")], [75, 43, 15, 6, 31],
  "R55-C08 case matrix");
console.log(`R55-C08: ${rows.length} complete raw native/WASM parity calls passed (${count("array")} array, `
  + `${count("null")} null, ${count("number")} number, ${count("tolerant")} tolerant, ${count("nullable")} nullable, `
  + `${count("control")} control)`);
