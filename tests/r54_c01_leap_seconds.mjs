import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { invoke } from "./aggregate_artifact/abi_host.mjs";
import { nativeResponses, assertParity } from "./aggregate_artifact/native_oracle.mjs";

const artifact = process.argv[2];
assert.ok(artifact, "usage: node tests/r54_c01_leap_seconds.mjs <exact-release.wasm>");
const bytes = readFileSync(artifact);
const fixture = JSON.parse(readFileSync(new URL("../fixtures/workspace-projection-v1.json", import.meta.url)));
const stamps = JSON.parse(readFileSync(new URL("../fixtures/r54-c01-timestamps.json", import.meta.url)));
const { instance } = await WebAssembly.instantiate(bytes);
const NOW = "2017-01-01T00:01:00Z";
const REOPENS = 4;
const STATUSES = ["paused", "completed", "cancelled"];
const calls = { "workspace.project.v1": 0, "projection.apply.v2": 0, "timer.reduce.v1": 0, rejected: 0 };

const isLeap = (stamp) => stamp.slice(17, 19) === "60";
const leapCount = stamps.filter(([stamp]) => isLeap(stamp)).length;
assert.equal(stamps.length, 24, "timestamp fixture length");
assert.equal(leapCount, 15, "leap timestamp count");
assert.equal(stamps.filter(([stamp]) => !stamp.endsWith("Z")).length, 8, "offset and lowercase-z count");
assert.equal(stamps.filter(([, expected]) => isLeap(expected) && !expected.includes("T23:59:60")).length, 2,
  "non-midnight leap count");

function request(stamp, status) {
  const input = structuredClone(fixture.request);
  for (const key of Object.keys(input.local)) input.local[key] = [];
  input.timerDependencies = [];
  input.neverSent = {};
  input.now = NOW;
  input.base.canonicalTimer = { id: "leap", phase: "focus", status,
    plannedDurationMs: 60000, elapsedAtAnchorMs: 0, anchorAt: stamp };
  input.base.history = [];
  if (status !== "paused") {
    const kind = status === "completed" ? "finish" : "cancel";
    input.base.canonicalTimer.elapsedAtAnchorMs = status === "completed" ? 60000 : 1234;
    input.base.canonicalTimer.lastIntent = { type: kind, commandId: kind, occurredAt: stamp };
    input.base.history = [{ id: "leap", timerId: "leap", commandId: kind,
      phase: "focus", status, plannedDurationMs: 60000 }];
    if (status === "completed") input.base.history[0].completedAt = stamp;
    else input.base.history[0].endedAt = stamp;
  }
  return input;
}

function call(name, operation, input, ok = true) {
  const item = { name, operation, input: JSON.stringify(input), ok };
  const [expected] = nativeResponses([item]);
  const actual = invoke(instance.exports, item);
  assertParity(item, expected, actual);
  calls[ok ? operation : "rejected"] += 1;
  return JSON.parse(actual);
}

function reopen(label, input, first) {
  for (let round = 0; round < REOPENS; round += 1) {
    input.base.canonicalTimer = first.value.workspace.canonicalTimer;
    input.base.history = first.value.workspace.history;
    const next = call(`${label}/reopen-${round}`, "workspace.project.v1", input);
    assert.deepEqual(next, first, `${label} reopen ${round} changed the complete envelope`);
  }
}

for (const status of STATUSES) {
  for (const [stamp, expected] of stamps) {
    const label = `${status}/${stamp}`;
    const input = request(stamp, status);
    const first = call(label, "workspace.project.v1", input);
    const workspace = first.value.workspace;
    assert.equal(workspace.canonicalTimer.anchorAt, expected, `${label} anchorAt`);
    if (status !== "paused") {
      assert.equal(workspace.canonicalTimer.lastIntent.occurredAt, stamp, `${label} lastIntent.occurredAt`);
    }
    if (status === "completed") assert.equal(workspace.history[0].completedAt, expected, `${label} completedAt`);
    if (status === "cancelled") assert.equal(workspace.history[0].endedAt, expected, `${label} endedAt`);
    reopen(label, input, first);
  }
}

function command(type, occurredAt, deviceSequence, hlcWallMs) {
  return { id: type, deviceId: "device-a", deviceSequence, timerId: "leap", type, phase: "focus",
    plannedDurationMs: 3600000, occurredAt, hlcWallMs, hlcCounter: 0, observedElapsedMs: 0 };
}

// Applies commands to an empty base, then reopens the applied timer and history.
function appliedThenReopened(label, commands) {
  const input = request("2016-12-31T23:59:00Z", "paused");
  input.base.canonicalTimer = null;
  const applied = call(`${label}/apply`, "projection.apply.v2",
    { base: input.base, pending: { ...input.local, commands }, now: NOW }).value;
  input.base.canonicalTimer = applied.canonicalTimer;
  input.base.history = applied.history;
  const first = call(`${label}/open`, "workspace.project.v1", input);
  assert.deepEqual(first.value.workspace.canonicalTimer, applied.canonicalTimer, `${label} open changed the timer`);
  assert.deepEqual(first.value.workspace.history, applied.history, `${label} open changed history`);
  reopen(label, input, first);
  return first.value.workspace;
}

const start = command("start", "2016-12-31T23:59:00Z", 1, 1483228740000);
for (const [kind, status, field] of [["finish", "completed", "completedAt"], ["cancel", "cancelled", "endedAt"]]) {
  for (const [at, expected] of [["2016-12-31T23:59:60Z", "2016-12-31T23:59:60Z"],
    ["2016-12-31T23:59:60.5Z", "2016-12-31T23:59:60.5Z"],
    ["2017-01-01T04:59:60.75+05:00", "2016-12-31T23:59:60.75Z"]]) {
    const label = `command/${kind}/${at}`;
    const workspace = appliedThenReopened(label, [start, command(kind, at, 2, 1483228800500)]);
    assert.equal(workspace.canonicalTimer.status, status, `${label} status`);
    assert.equal(workspace.canonicalTimer.anchorAt, expected, `${label} anchorAt`);
    assert.equal(workspace.canonicalTimer.lastIntent.occurredAt, expected, `${label} lastIntent.occurredAt`);
    assert.equal(workspace.history[0][field], expected, `${label} ${field}`);
  }
}

const leapStart = command("start", "2016-12-31T23:59:60.25Z", 1, 1483228800250);
const running = appliedThenReopened("command/running", [leapStart]).canonicalTimer;
assert.equal(running.status, "running", "command/running status");
assert.equal(running.anchorAt, "2016-12-31T23:59:60.25Z", "command/running anchorAt");
assert.equal(running.lastIntent.occurredAt, "2016-12-31T23:59:60.25Z", "command/running lastIntent.occurredAt");
const pause = command("pause", "2017-01-01T04:59:60.75+05:00", 2, 1483228800750);
const paused = appliedThenReopened("command/paused", [leapStart, pause]).canonicalTimer;
assert.equal(paused.status, "paused", "command/paused status");
assert.equal(paused.anchorAt, "2016-12-31T23:59:60.75Z", "command/paused anchorAt");
assert.equal(paused.lastIntent.occurredAt, "2016-12-31T23:59:60.75Z", "command/paused lastIntent.occurredAt");

const finish = command("finish", "2016-12-31T23:59:60.75Z", 2, 1483228800750);
const reduced = call("legacy", "timer.reduce.v1", { commands: [leapStart, finish], now: NOW }).value;
assert.equal(reduced.sessions.length, 1, "legacy session count");
assert.equal(reduced.sessions[0].startedAt, "2016-12-31T23:59:60.25Z", "legacy startedAt");
assert.equal(reduced.sessions[0].anchorAt, "2016-12-31T23:59:60.75Z", "legacy session anchorAt");
assert.equal(reduced.sessions[0].endedAt, "2016-12-31T23:59:60.75Z", "legacy endedAt");
assert.equal(reduced.canonicalTimer.anchorAt, "2016-12-31T23:59:60.75Z", "legacy canonical anchorAt");
assert.equal(reduced.history[0].completedAt, "2016-12-31T23:59:60.75Z", "legacy completedAt");
// The legacy reducer rejects a timer that overlaps history, so reopen each alone.
for (const key of ["canonicalTimer", "history"]) {
  for (let round = 0; round < REOPENS; round += 1) {
    const again = call(`legacy/${key}/reopen-${round}`, "timer.reduce.v1",
      { commands: [], now: NOW, [key]: reduced[key] }).value;
    assert.deepEqual(again[key], reduced[key], `legacy ${key} reopen ${round}`);
  }
}

for (const [anchor, intent] of [["2016-12-31T23:59:60.123456789Z", "2016-12-31T23:59:60.123456788Z"],
  ["2016-12-31T23:59:60.123456789Z", "2016-12-31T23:59:60.12345679Z"],
  ["2016-12-31T23:59:60Z", "2016-12-31T23:59:59.999999999Z"],
  ["2016-12-31T23:59:59.999999999Z", "2016-12-31T23:59:60Z"]]) {
  for (const status of ["completed", "cancelled"]) {
    const label = `${status}/${anchor}/intent-${intent}`;
    const input = request(anchor, status);
    input.base.canonicalTimer.lastIntent.occurredAt = intent;
    const result = call(label, "workspace.project.v1", input, false);
    assert.match(result.error, /conflicting workspace terminal timer\/history/, label);
  }
}

assert.deepEqual(calls, { "workspace.project.v1": 400, "projection.apply.v2": 8, "timer.reduce.v1": 9, rejected: 8 },
  "R54-C01 parity call counts");
const total = Object.values(calls).reduce((sum, count) => sum + count, 0);
const perOperation = Object.entries(calls).map(([name, count]) => `${name} ${count}`).join(", ");
console.log(`R54-C01: ${total} complete raw native/WASM parity calls passed (${perOperation}); `
  + `${stamps.length} timestamps (${leapCount} leap), ${STATUSES.length} statuses, ${REOPENS} reopens`);
