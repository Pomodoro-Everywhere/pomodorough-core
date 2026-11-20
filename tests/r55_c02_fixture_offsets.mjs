import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { invoke } from "./aggregate_artifact/abi_host.mjs";
import { nativeResponses, assertParity } from "./aggregate_artifact/native_oracle.mjs";

const artifact = process.argv[2];
assert.ok(artifact, "usage: node tests/r55_c02_fixture_offsets.mjs <exact-release.wasm>");
const { rows } = JSON.parse(readFileSync(new URL("../fixtures/r55-c02-fixture-offsets-v1.json", import.meta.url)));
const { instance } = await WebAssembly.instantiate(readFileSync(artifact));

// Overflow rows trapped in WASM before R55-C02. Inputs stay raw strings so i64
// extremes reach the artifact unrounded. Control rows pin the complete envelope
// captured before the fix.
const cases = rows.map((row) => {
  const expected = JSON.parse(row.expected);
  if (row.kind === "overflow") {
    assert.deepEqual(expected, { error: `invalid shared-core input: ${row.field} out of range`, ok: false }, row.name);
  }
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
  const key = `${row.field || "none"}/${row.kind}`;
  counts[key] = (counts[key] ?? 0) + 1;
}

assert.deepEqual(counts, {
  "nowMs/overflow": 6, "nowMs/control": 6,
  "commands.atMs/overflow": 7, "commands.atMs/control": 11,
  "none/control": 1,
}, "R55-C02 case matrix");
console.log(`R55-C02: ${rows.length} complete raw native/WASM parity calls passed `
  + `(${rows.filter((row) => row.kind === "overflow").length} overflow, `
  + `${rows.filter((row) => row.kind === "control").length} control)`);
