import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { invoke } from "./aggregate_artifact/abi_host.mjs";
import { nativeResponses, assertParity } from "./aggregate_artifact/native_oracle.mjs";

const artifact = process.argv[2];
assert.ok(artifact, "usage: node tests/r55_c06_fixture_duplicate_ids.mjs <exact-release.wasm>");
const { rows } = JSON.parse(readFileSync(new URL("../fixtures/r55-c06-fixture-duplicate-ids-v1.json", import.meta.url)));
const { instance } = await WebAssembly.instantiate(readFileSync(artifact));

// The legacy fixture operation `timer.reduce` reduced duplicate command ids
// before R55-C06. Each row pairs it with the `timer.reduce.v1` request that
// carries the same commands; duplicate rows must give both the v1 envelope.
const DUPLICATE = '{"error":"invalid shared-core input: duplicate timer command","ok":false}';
const LIMIT = '{"error":"invalid shared-core input: commands exceed 10000","ok":false}';
const command = { id: "c1", sequence: 1, deviceId: "device-a", timerId: "timer-a", type: "start", phase: "focus",
  durationMs: 60000, atMs: 0, wallMs: 101, counter: 0, elapsedMs: 0 };
const wire = { id: "c1", deviceId: "device-a", deviceSequence: 1, timerId: "timer-a", taskId: null, type: "start",
  phase: "focus", plannedDurationMs: 60000, occurredAt: "2026-07-20T12:00:00Z", hlcWallMs: 101, hlcCounter: 0,
  observedElapsedMs: 0 };
const limitCases = [
  { name: "limit-beats-dup/fixture", operation: "timer.reduce", expected: LIMIT,
    input: JSON.stringify({ epoch: "2026-07-20T12:00:00.000Z", nowMs: 1000, commands: Array(10001).fill(command) }) },
  { name: "limit-beats-dup/v1", operation: "timer.reduce.v1", expected: LIMIT,
    input: JSON.stringify({ now: "2026-07-20T12:00:01Z", commands: Array(10001).fill(wire) }) },
];

const pinned = rows.flatMap((row) => {
  if (row.kind === "duplicate") {
    assert.equal(row.fixture.expected, DUPLICATE, row.name);
    assert.equal(row.v1.expected, DUPLICATE, row.name);
  }
  return [
    { name: `${row.name}/fixture`, operation: "timer.reduce", input: row.fixture.input, expected: row.fixture.expected },
    { name: `${row.name}/v1`, operation: "timer.reduce.v1", input: row.v1.input, expected: row.v1.expected },
  ];
}).concat(limitCases);
const cases = pinned.map(({ name, operation, input, expected }) => {
  const envelope = JSON.parse(expected);
  return { name, operation, input, ok: envelope.ok, error: envelope.error ?? null };
});
const native = nativeResponses(cases);
for (const [index, item] of pinned.entries()) {
  let actual;
  try {
    actual = invoke(instance.exports, cases[index]);
  } catch (error) {
    assert.fail(`${item.name} trapped: ${error}`);
  }
  assertParity(cases[index], native[index], actual);
  assert.equal(actual, item.expected, `${item.name} pinned envelope`);
}

const kinds = rows.map((row) => row.kind);
assert.deepEqual([kinds.filter((kind) => kind === "duplicate").length, kinds.filter((kind) => kind === "control").length],
  [13, 10], "R55-C06 case matrix");
console.log(`R55-C06: ${pinned.length} complete raw native/WASM parity calls passed `
  + `(${rows.length} paired rows, 13 duplicate, 10 control, ${limitCases.length} limit)`);
