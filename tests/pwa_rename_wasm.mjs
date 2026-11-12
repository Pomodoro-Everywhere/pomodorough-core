// R54-S11: native/WASM parity for the rename request the PWA really sends.
// The PWA always sends `lifecycle`, so Core checks the request against the
// strict schema first. Core 0.55.0 had no `renameTask` there and rejected
// every PWA rename with "intent.kind must be a JSON string enum".
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { invoke } from "./aggregate_artifact/abi_host.mjs";
import { nativeResponses, assertParity } from "./aggregate_artifact/native_oracle.mjs";

const artifact = process.argv[2];
assert.ok(artifact, "usage: node tests/pwa_rename_wasm.mjs <exact-release.wasm>");
const { instance } = await WebAssembly.instantiate(readFileSync(artifact));
const recorded = JSON.parse(readFileSync(new URL("../fixtures/pwa-rename-request-v1.json", import.meta.url))).request;
const OPERATION = "workspace.intent.v1";
const cases = [];

function add(name, input, ok, check) {
  cases.push({ name, operation: OPERATION, input: JSON.stringify(input), ok, check });
}

function withIntent(intent) {
  return { ...structuredClone(recorded), intent };
}

function renamed(value) {
  const { taskId } = recorded.intent;
  assert.deepEqual(value.projection.tasks.map((task) => task.title), ["Beta"]);
  const [beta] = value.projection.tasks;
  assert.notEqual(beta.id, taskId);
  assert.equal(value.projection.selectedTaskId, beta.id);
  assert.ok(value.operations.taskOperations.some((item) => item.type === "upsert" && item.taskId === beta.id
    && item.title === "Beta"), "upsert of the new title");
  assert.ok(value.operations.taskOperations.some((item) => item.taskId === taskId && item.type === "rename"
    && item.title === "Beta"), "the old identity leaves through a rename that names the new title");
  assert.deepEqual(value.lifecycle, recorded.lifecycle);
}

function strictRejection(error) {
  assert.match(error, /intent\./);
  assert.doesNotMatch(error, /intent\.kind must be a JSON string enum/);
}

add("recorded-pwa-rename", recorded, true, renamed);
const { taskId } = recorded.intent;
add("missing-title", withIntent({ kind: "renameTask", taskId }), false, strictRejection);
add("missing-task-id", withIntent({ kind: "renameTask", title: "Beta" }), false, strictRejection);
add("unknown-field", withIntent({ kind: "renameTask", taskId, title: "Beta", note: "x" }), false, strictRejection);
add("null-task-id", withIntent({ kind: "renameTask", taskId: null, title: "Beta" }), false, strictRejection);
add("numeric-title", withIntent({ kind: "renameTask", taskId, title: 7 }), false, strictRejection);

const expected = nativeResponses(cases);
for (const [index, item] of cases.entries()) {
  const actual = invoke(instance.exports, item);
  assertParity(item, expected[index], actual);
  const envelope = JSON.parse(actual);
  assert.equal(envelope.ok, item.ok, `${item.name}: ${envelope.error ?? "unexpected success"}`);
  item.check(item.ok ? envelope.value : envelope.error);
}
assert.deepEqual([cases.length, cases.filter((item) => !item.ok).length], [6, 5], "PWA rename case counts");
console.log(`PWA rename: ${cases.length} raw native/WASM parity calls passed (1 recorded rename, 5 strict rejections)`);
