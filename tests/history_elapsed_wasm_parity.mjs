// Native/WASM parity for history elapsed time, with pinned values: an early
// Finish, a row without the field, explicit null, and the range rejections of
// `fixtures/workspace-terminal-v1.json` on every composed boundary.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { invoke } from "./aggregate_artifact/abi_host.mjs";
import { nativeResponses, assertParity } from "./aggregate_artifact/native_oracle.mjs";
import { terminalRequest } from "./aggregate_artifact/terminal_cases.mjs";

const artifact = process.argv[2];
assert.ok(artifact, "usage: node tests/history_elapsed_wasm_parity.mjs <exact-release.wasm>");
const { instance } = await WebAssembly.instantiate(readFileSync(artifact));
const terminal = JSON.parse(readFileSync(new URL("../fixtures/workspace-terminal-v1.json", import.meta.url)));
const PLANNED_MS = 1500000;
const PROFILES = ["appleWorkspace", "desktopStorage", "desktopTerminal", "androidCoordinator", "pwaStorage"];
const ABOVE_SIGNED_RANGE = "9223372036854775808";
const cases = [];

function add(name, operation, input, ok, check) {
  cases.push({ name, operation, input: typeof input === "string" ? input : JSON.stringify(input), ok, check });
}

function at(offsetMs) {
  const minutes = String(Math.floor(offsetMs / 60000)).padStart(2, "0");
  const seconds = String(Math.floor(offsetMs % 60000 / 1000)).padStart(2, "0");
  return `2026-07-15T10:${minutes}:${seconds}Z`;
}

function command(id, type, atMs, observedElapsedMs) {
  return { id, deviceId: "device-a", deviceSequence: atMs / 1000 + 1, timerId: "timer-a", type, phase: "focus",
    plannedDurationMs: PLANNED_MS, occurredAt: at(atMs), hlcWallMs: 1000 + atMs, hlcCounter: 0, observedElapsedMs };
}

function workspace(item) {
  const input = structuredClone(terminal.request);
  input.base.canonicalTimer = item.cleared ? null : { ...terminal.timer, ...item.timerOverrides };
  input.base.history = [{ ...terminal.history, ...item.historyOverrides }];
  return input;
}

function readModel(input, profile) {
  const { now, ...value } = input;
  return { profile, source: { kind: "workspace", value }, selectedPhase: "focus", observedAt: now,
    calendarIntervals: [{ start: "2026-07-20T00:00:00Z", end: "2026-07-21T00:00:00Z" }] };
}

function rebase(item) {
  const input = terminalRequest();
  input.local = structuredClone(terminal.request.local);
  input.sent = structuredClone(input.local);
  input.response.acknowledgements = [];
  input.response.serverTime = "2026-07-20T12:00:00Z";
  input.response.serverHlcWallMs = 1784548800000;
  input.response.canonicalTimer = item.cleared ? null : { ...terminal.timer, ...item.timerOverrides };
  input.response.history = [{ ...terminal.history, ...item.historyOverrides }];
  return input;
}

function reducerCases() {
  const finished = (elapsedMs) => (value) => {
    assert.equal(value.history.length, 1);
    assert.equal(value.history[0].status, "completed");
    assert.equal(value.history[0].plannedDurationMs, PLANNED_MS);
    assert.equal(value.history[0].elapsedMs, elapsedMs);
    assert.equal(value.canonicalTimer.elapsedAtAnchorMs, elapsedMs);
  };
  add("early-finish", "timer.reduce.v1", { commands: [command("start", "start", 0, 0),
    command("finish", "finish", 600000, 0)], now: at(700000) }, true, finished(600000));
  add("early-finish-observed-elapsed-lies", "timer.reduce.v1", { commands: [command("start", "start", 0, 0),
    command("finish", "finish", 600000, PLANNED_MS)], now: at(700000) }, true, finished(600000));
  add("finish-while-paused", "timer.reduce.v1", { commands: [command("start", "start", 0, 0),
    command("pause", "pause", 300000, 300000), command("finish", "finish", 900000, 0)], now: at(960000) },
  true, finished(300000));
}

function missingFieldCases() {
  const early = { timerOverrides: { elapsedAtAnchorMs: 59999 }, historyOverrides: {} };
  const explicitNull = { ...early, historyOverrides: { elapsedMs: null } };
  for (const [label, item] of [["missing", early], ["null", explicitNull]]) {
    add(`project-${label}`, "workspace.project.v1", workspace(item), true,
      (value) => assert.equal(value.workspace.history[0].elapsedMs, 59999));
    for (const profile of PROFILES) {
      add(`read-${label}-${profile}`, "workspace.readModel.v1", readModel(workspace(item), profile), true, (value) => {
        assert.equal(value.schemaVersion, 2);
        assert.equal(value.cadence.completedFocusToday, 1);
        assert.equal(value.cadence.completedFocusTodayElapsedMs, 59999);
      });
    }
    add(`rebase-${label}`, "reconcile.rebase.v3", rebase(item), true, (value) => {
      assert.equal(value.workspace.history[0].elapsedMs, 59999);
      assert.deepEqual(value.baseHistory, rebase(item).response.history);
    });
  }
}

function rejectionCases() {
  const rangeError = (error) => assert.equal(error, terminal.elapsedRangeError);
  assert.equal(terminal.elapsedRangeRejections.length, 4, "range rejection fixture count");
  for (const item of terminal.elapsedRangeRejections) {
    add(`project-${item.name}`, "workspace.project.v1", workspace(item), false, rangeError);
    for (const profile of PROFILES) {
      add(`read-${item.name}-${profile}`, "workspace.readModel.v1", readModel(workspace(item), profile), false,
        rangeError);
    }
    add(`rebase-${item.name}`, "reconcile.rebase.v3", rebase(item), false, rangeError);
  }
  // JSON.stringify cannot print 2^63 exactly, so the token is spliced into the raw input.
  const unsigned = { historyOverrides: { elapsedMs: "ABOVE_SIGNED_RANGE" } };
  const decodeError = (error) => assert.match(error, new RegExp(
    `^invalid shared-core JSON: invalid value: integer \`${ABOVE_SIGNED_RANGE}\`, expected i64( at line \\d+ column \\d+)?$`));
  for (const [name, operation, input] of [["project", "workspace.project.v1", workspace(unsigned)],
    ["read", "workspace.readModel.v1", readModel(workspace(unsigned), "pwaStorage")],
    ["rebase", "reconcile.rebase.v3", rebase(unsigned)]]) {
    const raw = JSON.stringify(input).replace('"ABOVE_SIGNED_RANGE"', ABOVE_SIGNED_RANGE);
    assert.ok(raw.includes(`"elapsedMs":${ABOVE_SIGNED_RANGE}`), name);
    add(`${name}-above-signed-range`, operation, raw, false, decodeError);
  }
}

reducerCases();
missingFieldCases();
rejectionCases();
const expected = nativeResponses(cases);
for (const [index, item] of cases.entries()) {
  const actual = invoke(instance.exports, item);
  assertParity(item, expected[index], actual);
  const envelope = JSON.parse(actual);
  assert.equal(envelope.ok, item.ok, `${item.operation}/${item.name}`);
  item.check(item.ok ? envelope.value : envelope.error);
}
const rejected = cases.filter((item) => !item.ok).length;
assert.deepEqual([cases.length, rejected], [48, 31], "history elapsed parity case counts");
console.log(`history elapsed: ${cases.length} raw native/WASM parity calls passed `
  + `(${cases.length - rejected} accepted with pinned elapsed, ${rejected} rejected with pinned errors)`);
