import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { invoke } from "./aggregate_artifact/abi_host.mjs";
import { nativeResponses, assertParity } from "./aggregate_artifact/native_oracle.mjs";

const artifact = process.argv[2];
assert.ok(artifact, "usage: node tests/r55_c14_canonical_head.mjs <exact-release.wasm>");
const { rows } = JSON.parse(readFileSync(new URL("../fixtures/r55-c14-canonical-head-v1.json", import.meta.url)));
const { instance } = await WebAssembly.instantiate(readFileSync(artifact));

// Before R55-C14 the head that hides never-sent operations was serverHlc*,
// which covers the server's physical clock, so a Start pressed while a sync was
// in flight vanished until the next sync. user rows carry an older canonical
// pair and now project the Start; other, fallback and clock rows keep their
// stack envelope; error rows break the receiver rule (one field, canonical
// above server, not a JavaScript-safe non-negative integer), were accepted
// before and now fail closed with the row's message. The
// -duration-after-acknowledged-barrier rows hold a never-sent duration change
// that only the display after the response can admit. R55-C14-r1 adds the
// 2^53 - 1 and spelling boundaries, a head counter that decides at the
// Start's wall, the Start-then-Pause command order, and server rows: with the
// pair present, an invalid server HLC is blamed on the server HLC.
const NEVER_SENT = [["commands", "01a0fc7f-0a01-7b1e-99b1-cfb471edbf8c"], ["durationOperations", "01a0fc7f-0d02-7b1e-99b1-cfb471edbf8e"]];
const projectsStart = (envelope) =>
  NEVER_SENT.some(([queue, id]) => (envelope.value?.projectionPending?.[queue] ?? []).some((operation) => operation.id === id));
const cases = rows.map((row) => {
  const expected = JSON.parse(row.expected);
  const before = JSON.parse(row.before);
  if (row.kind === "user") {
    assert.equal(projectsStart(before), false, `${row.name} hid the Start before R55-C14`);
    assert.equal(projectsStart(expected), true, row.name);
  } else if (row.kind === "error") {
    assert.equal(before.ok, true, `${row.name} was accepted before R55-C14`);
    assert.deepEqual(expected, { error: row.message, ok: false }, row.name);
  } else if (row.kind === "server") {
    assert.match(row.message, /canonical response server HLC$/, row.name);
    assert.deepEqual(expected, { error: row.message, ok: false }, row.name);
  } else {
    assert.equal(row.expected, row.before, row.name);
    assert.equal(expected.ok, true, row.name);
    if (row.kind !== "clock") assert.equal(projectsStart(expected), false, row.name);
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

const kinds = ["user", "other", "fallback", "error", "server", "clock"];
const count = (kind) => rows.filter((row) => row.kind === kind).length;
assert.deepEqual(kinds.map(count), [14, 16, 4, 60, 16, 2], "R55-C14 case matrix");
console.log(`R55-C14: ${rows.length} complete raw native/WASM parity calls passed (`
  + kinds.map((kind) => `${count(kind)} ${kind}`).join(", ") + ")");
