// R54-C04: native/WASM parity for the `rename` task operation and
// `task.carryHistory.v1`, over the shared wire fixture and its fail-closed
// cases.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { invoke } from "./aggregate_artifact/abi_host.mjs";
import { nativeResponses, assertParity } from "./aggregate_artifact/native_oracle.mjs";

const artifact = process.argv[2];
assert.ok(artifact, "usage: node tests/task_carry_history_wasm.mjs <exact-release.wasm>");
const { instance } = await WebAssembly.instantiate(readFileSync(artifact));
const fixture = JSON.parse(readFileSync(new URL("../fixtures/task-rename-v1.json", import.meta.url)));
const cases = [];

function add(name, operation, input, ok, check) {
  cases.push({ name, operation, input: JSON.stringify(input), ok, check });
}

function matches(expected) {
  return (value) => assert.deepEqual(value, expected);
}

function rejects(pattern) {
  return (error) => assert.match(error, pattern);
}

const { taskReduce, carryHistory } = fixture;
add("task-reduce-rename", taskReduce.operation, taskReduce.input, true, matches(taskReduce.output));
add("carry-history", carryHistory.operation, carryHistory.input, true, matches(carryHistory.output));
for (const [index, invalid] of fixture.invalidRenames.entries()) {
  const operations = [{ ...fixture.renameOperation, title: invalid.title }];
  add(`invalid-rename-${index}`, "task.reduce.v1", { operations }, false, rejects(/invalid task rename/));
}
add("carry-unknown-field", carryHistory.operation, { ...carryHistory.input, now: "2026-03-08T08:00:00Z" },
  false, rejects(/unknown field `now`/));
const { sessions: _sessions, ...withoutSessions } = carryHistory.input;
add("carry-missing-sessions", carryHistory.operation, withoutSessions, false, rejects(/sessions must be a JSON array/));
const badRename = { ...carryHistory.input, taskOperations: [{ ...fixture.renameOperation, title: "Draft chapter" }] };
add("carry-invalid-rename", carryHistory.operation, badRename, false, rejects(/invalid task rename/));
const recarried = { ...carryHistory.input, history: carryHistory.output.history };
add("carry-already-carried", carryHistory.operation, recarried, false, rejects(/disagrees with its session taskId/));

const expected = nativeResponses(cases);
for (const [index, item] of cases.entries()) {
  const actual = invoke(instance.exports, item);
  assertParity(item, expected[index], actual);
  const envelope = JSON.parse(actual);
  assert.equal(envelope.ok, item.ok, `${item.name}: ${envelope.error ?? "unexpected success"}`);
  item.check(item.ok ? envelope.value : envelope.error);
}
assert.deepEqual([cases.length, cases.filter((item) => !item.ok).length], [9, 7], "task rename case counts");
console.log(`Task rename: ${cases.length} raw native/WASM parity calls passed (2 fixture results, 7 rejections)`);
