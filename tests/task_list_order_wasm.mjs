// Native/WASM parity for the task list order. The order folds case with the
// Rust toolchain's Unicode tables and compares digit runs by value; the
// release artifact must list the same titles in the same order as native Core.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { invoke } from "./aggregate_artifact/abi_host.mjs";
import { nativeResponses, assertParity } from "./aggregate_artifact/native_oracle.mjs";

const artifact = process.argv[2];
assert.ok(artifact, "usage: node tests/task_list_order_wasm.mjs <exact-release.wasm>");
const { instance } = await WebAssembly.instantiate(readFileSync(artifact));

// Each list is in the expected order.
const lists = {
  bulk: Array.from({ length: 12 }, (_, index) => `Bulk task ${index + 1}`),
  "leading-zeros": ["Task 0", "Task 00", "Task 1", "Task 007", "Task 7", "Task 08"],
  "long-runs": [`Run ${"0".repeat(60)}2`, "Run 18446744073709551615", "Run 184467440737095516150",
    `Run ${"9".repeat(40)}`, `Run 1${"0".repeat(40)}`],
  "mixed-case": ["Apple", "apple", "Banana", "banana", "cherry", "Zebra"],
  cyrillic: ["Арбуз", "Яблоко", "яблоко", "яблоко 2", "ЯБЛОКО 10"],
  "multi-char-fold": ["STRASSE 2", "Straße 3", "strasse 10", "ΟΔΟΣ 2", "οδος 3", "οδοσ 10"],
  "digits-among-text": ["Task", "Task 1", "Task A", "Task-1", "Task1", "Taska", "École 2", "école 10",
    "日本語 2", "日本語 10"],
  "ascii-punctuation": ["Task/", "Task1", "Task10", "Task:", "Task@", "Task[", "Task_1", "Task`", "TaskA", "Taska",
    "Task{"],
  "sharp-s": ["SS", "ss", "ß", "ẞ", "STRASSE 2", "STRAẞE 3", "Straße 4", "strasse 10"],
  "dotted-i": ["I 2", "ı 3", "i 10", "iz", "i\u0307 2", "İ 3", "i\u0307 10", "j"],
};

function parsed(raw, label) {
  const envelope = JSON.parse(raw);
  assert.equal(envelope.ok, true, `${label}: ${envelope.error}`);
  return envelope.value;
}

function reduceCase(name, tasks) {
  const operations = tasks.map((task, index) => ({
    id: `upsert-${index}`, deviceId: "device-a", occurredAt: "2026-08-22T12:00:00Z",
    hlcWallMs: 2000 + index, hlcCounter: 0, taskId: task.id, type: "upsert", title: task.title }));
  return { name, operation: "task.reduce.v1", input: JSON.stringify({ operations }), ok: true };
}

const identityCases = Object.entries(lists).flatMap(([list, titles]) => titles.map((title, index) =>
  ({ name: `${list}-identity-${index}`, operation: "task.identity.v1", input: JSON.stringify({ title }), ok: true })));
const identities = nativeResponses(identityCases).map((raw, index) => parsed(raw, identityCases[index].name));
const reduceCases = [];
let cursor = 0;
for (const [list, titles] of Object.entries(lists)) {
  const tasks = identities.slice(cursor, cursor += titles.length);
  assert.deepEqual(tasks.map((task) => task.title), titles, `${list}: titles are already normalized`);
  reduceCases.push({ ...reduceCase(`${list}-reversed`, [...tasks].reverse()), titles });
  const middle = Math.floor(tasks.length / 2);
  reduceCases.push({ ...reduceCase(`${list}-rotated`, [...tasks.slice(middle), ...tasks.slice(0, middle)]), titles });
}

const cases = [...identityCases, ...reduceCases];
const expected = nativeResponses(cases);
for (const [index, item] of cases.entries()) {
  const actual = invoke(instance.exports, item);
  assertParity(item, expected[index], actual);
  if (item.titles) {
    assert.deepEqual(parsed(actual, item.name).tasks.map((task) => task.title), item.titles, item.name);
  }
}
assert.deepEqual([identityCases.length, reduceCases.length], [77, 20], "task list order case counts");
console.log(`Task list order: ${cases.length} raw native/WASM parity calls passed (${reduceCases.length} ordered lists)`);
