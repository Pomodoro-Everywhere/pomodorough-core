import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { invoke } from "./aggregate_artifact/abi_host.mjs";
import { nativeResponses, assertParity } from "./aggregate_artifact/native_oracle.mjs";

const artifact = process.argv[2];
assert.ok(artifact, "usage: node tests/r55_c07_non_object_roots.mjs <exact-release.wasm>");
const { rows } = JSON.parse(readFileSync(new URL("../fixtures/r55-c07-non-object-roots-v1.json", import.meta.url)));
const { instance } = await WebAssembly.instantiate(readFileSync(artifact));

// Before R55-C07 these operations decoded a JSON array field by field (some
// returned ok:true) and answered other non-object roots with raw Serde text.
// Control rows pin the complete envelope, unchanged by the fix.
const ROOT_NAMES = {
  "selectedTask.classify": "selected task envelope",
  "bootstrap.plan.v1": "bootstrap plan",
  "task.identity.v1": "task identity",
  "hlc.head.v1": "hlc head",
  "hlc.tick.v1": "hlc tick",
  "uuidv7.fromParts.v1": "UUIDv7 parts",
  "timer.reduce.v1": "timer reduction",
  "timer.reduce": "timer fixture",
  "timer.replay.page.v1": "timer replay page",
  "workspace.intent.v1": "workspace intent",
  "sync.batchPlan.v1": "batch plan",
  "timer.completionPlan.v1": "completion plan",
  "timer.completionState.v1": "completion state",
  "clock.observe.v1": "clock observation",
};
const ROOT_LABELS = ["array-empty", "array-positional", "null", "number", "string", "boolean"];

const cases = rows.map((row) => {
  const expected = JSON.parse(row.expected);
  if (row.kind === "root") {
    assert.deepEqual(expected, {
      error: `invalid shared-core input: ${ROOT_NAMES[row.operation]} must be a JSON object`, ok: false,
    }, row.name);
  }
  if (row.kind === "unknown-field") {
    assert.match(expected.error, /^invalid shared-core JSON: unknown field `futureField`/, row.name);
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
  const labels = rows.filter((row) => row.kind === "root" && row.operation === operation)
    .map((row) => row.name.slice(`${operation}-root-`.length));
  assert.deepEqual(labels, ROOT_LABELS, `R55-C07 ${operation} root matrix`);
  const valid = rows.find((row) => row.name === `${operation}-valid`);
  assert.equal(JSON.parse(valid.expected).ok, true, `R55-C07 ${operation} valid control`);
}
const count = (kind) => rows.filter((row) => row.kind === kind).length;
assert.deepEqual([rows.length, count("root"), count("control"), count("unknown-field")], [118, 84, 28, 6],
  "R55-C07 case matrix");
console.log(`R55-C07: ${rows.length} complete raw native/WASM parity calls passed `
  + `(${count("root")} root, ${count("control")} control, ${count("unknown-field")} unknown-field)`);
