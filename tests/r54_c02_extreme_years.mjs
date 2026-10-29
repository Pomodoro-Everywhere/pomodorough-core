import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { invoke } from "./aggregate_artifact/abi_host.mjs";
import { nativeOutput, validateResponses, assertParity } from "./aggregate_artifact/native_oracle.mjs";

const artifact = process.argv[2];
assert.ok(artifact, "usage: node tests/r54_c02_extreme_years.mjs <exact-release.wasm>");
const { instance } = await WebAssembly.instantiate(readFileSync(artifact));
const fixture = (name) => JSON.parse(readFileSync(new URL(`../fixtures/${name}`, import.meta.url)));
const projectionFixture = fixture("workspace-projection-v1.json");
const readModelFixture = fixture("read-model-v1.json");
const intentFixture = fixture("workspace-intent-v1.json");
const REOPENS = 4;
const TIMER_OUT_OF_RANGE = "invalid shared-core input: timer timestamp out of range";
const LAST_INSTANT = "9999-12-31T23:59:59.999999999Z";
const LAST_LEAP = "9999-12-31T23:59:60.999999999Z";
const LAST_DAY = [{ start: "9999-12-31T00:00:00Z", end: "9999-12-31T23:59:59.999Z" }];
// One nanosecond outside each bound; each offset keeps the local year in range.
const PAST_LOWER = "0000-01-01T00:59:59.999999999+01:00";
const PAST_UPPER = "9999-12-31T23:00:00-01:00";
const calls = { "timer.reduce.v1": 0, "workspace.project.v1": 0, "workspace.readModel.v1": 0,
  "workspace.intent.v1": 0, "timer.replay.page.v1": 0, rejected: 0 };
const failures = [];

// Records a failed check and continues, so a red run reports every case.
function check(label, assertion) {
  try {
    assertion();
  } catch (error) {
    failures.push(`FAIL ${label}: ${error.message}`);
  }
}

// Runs one phase as a single native oracle batch, then validates every native
// envelope and compares every raw WASM envelope with its native twin.
function run(items) {
  const native = nativeOutput(items).trimEnd().split("\n");
  check("native oracle response count", () => assert.equal(native.length, items.length));
  return items.map((item, index) => {
    const actual = invoke(instance.exports, item);
    check(`${item.operation}/${item.name}`, () => {
      validateResponses([item], native[index]);
      assertParity(item, native[index], actual);
    });
    calls[item.ok ? item.operation : "rejected"] += 1;
    return JSON.parse(actual);
  });
}

const item = (name, operation, input, ok) => ({ name, operation, input: JSON.stringify(input), ok });

function timer(anchorAt, status) {
  return { id: "year", phase: "focus", status, plannedDurationMs: 60000, elapsedAtAnchorMs: 0, anchorAt };
}

function completedPair(stamp) {
  const completed = { ...timer(stamp, "completed"), elapsedAtAnchorMs: 60000,
    lastIntent: { type: "finish", commandId: "finish", occurredAt: stamp } };
  const history = { id: "year", timerId: "year", commandId: "finish", phase: "focus",
    status: "completed", plannedDurationMs: 60000, completedAt: stamp };
  return [completed, history];
}

function command(type, occurredAt) {
  return { id: type, deviceId: "device-a", deviceSequence: 1, timerId: "year", type, phase: "focus",
    plannedDurationMs: 60000, occurredAt, hlcWallMs: 1, hlcCounter: 0, observedElapsedMs: 0 };
}

// A later start for another timer, so the reducer completes or supersedes the
// earlier session at this start's time.
function supersedingStart(occurredAt) {
  return { ...command("start", occurredAt), id: "second", timerId: "next", deviceSequence: 2, hlcWallMs: 2 };
}

function firstPage(startAt, now) {
  return { commands: [command("start", startAt)], sessions: [], currentTimerId: null, after: null, now };
}

function workspace(canonicalTimer, history, now) {
  const input = structuredClone(projectionFixture.request);
  for (const key of Object.keys(input.local)) input.local[key] = [];
  input.timerDependencies = [];
  input.neverSent = {};
  input.now = now;
  input.base.canonicalTimer = canonicalTimer;
  input.base.history = history;
  return input;
}

function readModel(anchor, observedAt, monotonic) {
  const input = structuredClone(readModelFixture.request);
  input.source.value.base.canonicalTimer = timer(anchor, "running");
  input.observedAt = observedAt;
  input.calendarIntervals = LAST_DAY;
  if (monotonic) Object.assign(input, { profile: "pwaStorage", monotonic });
  return input;
}

function monotonic(anchorAt, sampledTrustedNowMs, nowMs) {
  return { nowMs, continuityId: "session", anchor: { timerId: "year", anchorAt, elapsedAtAnchorMs: 0,
    sampledTrustedNowMs, sampledMonotonicMs: 100, continuityId: "session" } };
}

function pwaObservation(anchor, now, wallMs) {
  const input = structuredClone(intentFixture.request);
  input.compatibility = "pwaStorage";
  input.intent = { kind: "selectPhase", phase: "focus" };
  input.workspace.base.canonicalTimer = { ...intentFixture.timer, anchorAt: anchor };
  input.workspace.canonicalHead = { wallMs, counter: 0 };
  input.allocation.hlc = { wallMs, counter: 0 };
  Object.assign(input.clock, { occurredAt: now, physicalNow: now, observedAt: now,
    monotonicNowMs: 100, continuityId: "browser-session-1" });
  input.calendarIntervals = LAST_DAY;
  return input;
}

// Apple adapter request whose retained start was physically observed 30
// minutes after its canonical time, so the overlay shifts the paused anchor
// by 30 minutes.
function appleOverlay(replicationMode, anchorAt) {
  const intentAt = "9999-12-31T23:00:00Z";
  const physicalAt = "9999-12-31T23:30:00Z";
  const wallMs = 253402297200000;
  const input = structuredClone(intentFixture.request);
  input.replicationMode = replicationMode;
  input.intent = { kind: "selectPhase", phase: "focus" };
  input.workspace.base.canonicalTimer = { ...intentFixture.timer, status: "paused", anchorAt,
    lastIntent: { type: "start", commandId: "retained-start", occurredAt: intentAt } };
  input.workspace.local.commands = [{ id: "retained-start", deviceId: "device-local", deviceSequence: 6,
    timerId: "existing-timer", type: "start", phase: "focus", plannedDurationMs: 60000, occurredAt: intentAt,
    hlcWallMs: wallMs, hlcCounter: 0, observedElapsedMs: 0 }];
  input.workspace.canonicalHead = { wallMs, counter: 0 };
  input.allocation.hlc = { wallMs, counter: 0 };
  input.observation = { canonicalAnchorAt: null, commandTimes: { "retained-start": physicalAt } };
  Object.assign(input.clock, { occurredAt: physicalAt, physicalNow: physicalAt, observedAt: physicalAt });
  input.calendarIntervals = LAST_DAY;
  return input;
}

const [terminal, terminalHistory] = completedPair("9999-12-31T23:30:00-01:00");
const queued = workspace(null, [], "9999-12-31T23:59:59Z");
queued.local.commands = [command("start", "9999-12-31T23:30:00-01:00")];
// Chrono counts the leap second as elapsed, so a one-minute timer anchored at
// 23:59:00 completes at 10000-01-01T00:00:00Z.
const rejections = [
  ["legacy/review-now-overflow", "timer.reduce.v1", { commands: [], now: "9999-12-31T23:59:59-00:01",
    canonicalTimer: timer("9999-12-31T23:59:30Z", "running") },
  "invalid RFC 3339 timestamp: 9999-12-31T23:59:59-00:01"],
  ["legacy/now-underflow", "timer.reduce.v1", { commands: [], now: "0000-01-01T00:30:00+01:00",
    canonicalTimer: timer("0000-01-01T00:00:00Z", "paused") },
  "invalid RFC 3339 timestamp: 0000-01-01T00:30:00+01:00"],
  ["legacy/command-underflow", "timer.reduce.v1", { commands: [command("start", "0000-01-01T00:30:00+01:00")],
    now: "0000-01-01T01:00:00+01:00" }, "invalid RFC 3339 timestamp: 0000-01-01T00:30:00+01:00"],
  ["legacy/command-overflow", "timer.reduce.v1", { commands: [command("start", "9999-12-31T23:30:00-01:00")],
    now: "9999-12-31T23:59:59Z" }, "invalid RFC 3339 timestamp: 9999-12-31T23:30:00-01:00"],
  ["legacy/anchor-overflow", "timer.reduce.v1", { commands: [], now: "9999-12-31T23:59:59Z",
    canonicalTimer: timer("9999-12-31T23:30:00-01:00", "paused") },
  "invalid shared-core input: invalid canonical timer"],
  ["legacy/history-overflow", "timer.reduce.v1", { commands: [], now: "9999-12-31T23:59:59Z",
    history: [terminalHistory] }, "invalid shared-core input: invalid timer history"],
  ["legacy/now-past-lower", "timer.reduce.v1", { commands: [], now: PAST_LOWER },
    "invalid RFC 3339 timestamp: 0000-01-01T00:59:59.999999999+01:00"],
  ["legacy/paused-anchor-past-lower", "timer.reduce.v1", { commands: [], now: LAST_INSTANT,
    canonicalTimer: timer(PAST_LOWER, "paused") }, "invalid shared-core input: invalid canonical timer"],
  ["legacy/command-past-lower", "timer.reduce.v1", { commands: [command("start", PAST_LOWER)], now: LAST_INSTANT },
    "invalid RFC 3339 timestamp: 0000-01-01T00:59:59.999999999+01:00"],
  ["legacy/history-past-lower", "timer.reduce.v1", { commands: [], now: LAST_INSTANT,
    history: [completedPair(PAST_LOWER)[1]] }, "invalid shared-core input: invalid timer history"],
  ["legacy/now-past-upper", "timer.reduce.v1", { commands: [], now: PAST_UPPER },
    "invalid RFC 3339 timestamp: 9999-12-31T23:00:00-01:00"],
  ["legacy/paused-anchor-past-upper", "timer.reduce.v1", { commands: [], now: LAST_INSTANT,
    canonicalTimer: timer(PAST_UPPER, "paused") }, "invalid shared-core input: invalid canonical timer"],
  ["legacy/command-past-upper", "timer.reduce.v1", { commands: [command("start", PAST_UPPER)], now: LAST_INSTANT },
    "invalid RFC 3339 timestamp: 9999-12-31T23:00:00-01:00"],
  ["legacy/history-past-upper", "timer.reduce.v1", { commands: [], now: LAST_INSTANT,
    history: [completedPair(PAST_UPPER)[1]] }, "invalid shared-core input: invalid timer history"],
  ["legacy/canonical-completion-overflow", "timer.reduce.v1", { commands: [], now: "9999-12-31T23:59:60.5Z",
    canonicalTimer: timer("9999-12-31T23:59:00Z", "running") }, TIMER_OUT_OF_RANGE],
  ["legacy/command-completion-overflow", "timer.reduce.v1", { commands: [command("start", "9999-12-31T23:59:00Z")],
    now: "9999-12-31T23:59:60.5Z" }, TIMER_OUT_OF_RANGE],
  // The leap-second start completes the earlier timer, which is then no longer current.
  ["legacy/superseding-start-completion-overflow", "timer.reduce.v1", { now: LAST_LEAP,
    commands: [command("start", "9999-12-31T23:59:00Z"), supersedingStart("9999-12-31T23:59:60.5Z")] },
  TIMER_OUT_OF_RANGE],
  ["workspace/review-now-overflow", "workspace.project.v1",
    workspace(timer("9999-12-31T23:59:30Z", "running"), [], "9999-12-31T23:59:59-00:01"),
    "invalid RFC 3339 timestamp: 9999-12-31T23:59:59-00:01"],
  ["workspace/paused-now-overflow", "workspace.project.v1",
    workspace(timer("9999-12-31T23:00:00Z", "paused"), [], "9999-12-31T23:30:00-01:00"),
    "invalid RFC 3339 timestamp: 9999-12-31T23:30:00-01:00"],
  ["workspace/anchor-underflow", "workspace.project.v1",
    workspace(timer("0000-01-01T00:30:00+01:00", "paused"), [], "2017-01-01T00:00:00Z"),
    "invalid shared-core input: invalid canonical timer"],
  ["workspace/terminal-overflow", "workspace.project.v1",
    workspace(terminal, [terminalHistory], "9999-12-31T23:59:59Z"),
    "invalid shared-core input: invalid canonical timer"],
  ["workspace/queued-command-overflow", "workspace.project.v1", queued,
    "invalid RFC 3339 timestamp: 9999-12-31T23:30:00-01:00"],
  ["workspace/completion-overflow", "workspace.project.v1",
    workspace(timer("9999-12-31T23:59:00Z", "running"), [], "9999-12-31T23:59:60.5Z"), TIMER_OUT_OF_RANGE],
  // Paged replay completes the current timer and projects without the reducer's finish step.
  ["replayPage/completion-overflow", "timer.replay.page.v1",
    firstPage("9999-12-31T23:59:00Z", "9999-12-31T23:59:60.5Z"), TIMER_OUT_OF_RANGE],
  // 23:59:30 + 60 s is 10000-01-01T00:00:30Z.
  ["readModel/deadline-overflow", "workspace.readModel.v1",
    readModel("9999-12-31T23:59:30Z", "9999-12-31T23:59:40Z"),
    "invalid shared-core input: read model deadline out of range"],
  // 23:59:00 + 60 s is exactly 10000-01-01T00:00:00Z.
  ["readModel/deadline-at-10000", "workspace.readModel.v1",
    readModel("9999-12-31T23:59:00Z", "9999-12-31T23:59:00Z"),
    "invalid shared-core input: read model deadline out of range"],
  // Trusted sample 23:59:40 plus 35 s of monotonic time replays at 10000-01-01T00:00:15Z.
  ["readModel/replay-overflow", "workspace.readModel.v1",
    readModel("9999-12-31T23:59:30Z", "9999-12-31T23:59:40Z",
      monotonic("9999-12-31T23:59:30Z", 253402300780000, 35100)),
    "invalid shared-core input: read model replay time out of range"],
  // 20 s more replays at exactly 10000-01-01T00:00:00Z; one millisecond less
  // replays in range and fails only on the deadline.
  ["readModel/replay-at-10000", "workspace.readModel.v1",
    readModel("9999-12-31T23:59:30Z", "9999-12-31T23:59:40Z",
      monotonic("9999-12-31T23:59:30Z", 253402300780000, 20100)),
    "invalid shared-core input: read model replay time out of range"],
  ["readModel/replay-last-ms", "workspace.readModel.v1",
    readModel("9999-12-31T23:59:30Z", "9999-12-31T23:59:40Z",
      monotonic("9999-12-31T23:59:30Z", 253402300780000, 20099)),
    "invalid shared-core input: read model deadline out of range"],
  // The fixture timer has 55 s left: 23:59:30 + 55 s is 10000-01-01T00:00:25Z.
  ["intent/observation-overflow", "workspace.intent.v1",
    pwaObservation("9999-12-31T23:59:30Z", "9999-12-31T23:59:40Z", 253402300780000),
    "invalid shared-core input: monotonic timer observation out of range"],
  // 23:59:05 + 55 s is exactly 10000-01-01T00:00:00Z.
  ["intent/observation-at-10000", "workspace.intent.v1",
    pwaObservation("9999-12-31T23:59:05Z", "9999-12-31T23:59:15Z", 253402300755000),
    "invalid shared-core input: monotonic timer observation out of range"],
  // 23:30:00 shifts to exactly 10000-01-01T00:00:00Z. Centralized Apple
  // replays at the shifted anchor, so parsing that replay time rejects it.
  // Iroh replays at physical now, so only the session bound rejects it.
  ["intent/apple-overlay-centralized-at-10000", "workspace.intent.v1",
    appleOverlay("centralized", "9999-12-31T23:30:00Z"), "invalid RFC 3339 timestamp: +10000-01-01T00:00:00Z"],
  ["intent/apple-overlay-iroh-at-10000", "workspace.intent.v1",
    appleOverlay("iroh", "9999-12-31T23:30:00Z"), TIMER_OUT_OF_RANGE],
];

const upperRunning = timer("9999-12-31T23:58:59.999999999Z", "running");
const leapRunning = { ...timer("9999-12-31T23:59:00Z", "running"), plannedDurationMs: 120000 };
const accepted = {
  legacyLower: ["timer.reduce.v1", { now: "0000-01-01T00:00:00Z",
    commands: [command("start", "0000-01-01T01:00:00+01:00")] }],
  legacyUpper: ["timer.reduce.v1", { commands: [], now: LAST_INSTANT, canonicalTimer: upperRunning }],
  legacyLeap: ["timer.reduce.v1", { commands: [command("pause", LAST_LEAP)], now: LAST_LEAP,
    canonicalTimer: leapRunning }],
  legacySuperseding: ["timer.reduce.v1", { now: LAST_LEAP, commands: [
    command("start", "9999-12-31T23:58:59.999999999Z"), supersedingStart("9999-12-31T23:59:60.5Z")] }],
  replayPageUpper: ["timer.replay.page.v1", firstPage("9999-12-31T23:58:59.999999999Z", LAST_INSTANT)],
  workspaceLower: ["workspace.project.v1",
    workspace(timer("0000-01-01T00:00:00Z", "paused"), [], "0000-01-01T01:00:00+01:00")],
  workspaceLeap: ["workspace.project.v1", workspace(timer(LAST_LEAP, "paused"), [], LAST_LEAP)],
  workspaceUpper: ["workspace.project.v1", workspace(upperRunning, [], LAST_INSTANT)],
  readModelDeadline: ["workspace.readModel.v1", readModel("9999-12-31T23:58:59.999Z", "9999-12-31T23:59:30Z")],
  // Trusted sample 23:59:30.000 plus 29.999 s completes exactly at 23:59:59.999.
  readModelReplay: ["workspace.readModel.v1", readModel("9999-12-31T23:58:59.999Z", "9999-12-31T23:59:30Z",
    monotonic("9999-12-31T23:58:59.999Z", 253402300770000, 30099))],
  intentObservation: ["workspace.intent.v1",
    pwaObservation("9999-12-31T23:59:04.999Z", "9999-12-31T23:59:14.999Z", 253402300754999)],
  appleOverlayCentralized: ["workspace.intent.v1", appleOverlay("centralized", "9999-12-31T23:29:59.999999999Z")],
  appleOverlayIroh: ["workspace.intent.v1", appleOverlay("iroh", "9999-12-31T23:29:59.999999999Z")],
};

const acceptedNames = Object.keys(accepted);
const firstPhase = run([
  ...rejections.map(([name, operation, input]) => item(name, operation, input, false)),
  ...acceptedNames.map((name) => item(name, accepted[name][0], accepted[name][1], true)),
]);
rejections.forEach(([name, , , error], index) =>
  check(name, () => assert.equal(firstPhase[index].error, error)));
const first = Object.fromEntries(acceptedNames.map((name, index) =>
  [name, firstPhase[rejections.length + index].value]));

const { legacyLower, legacyUpper, legacyLeap, legacySuperseding, replayPageUpper } = first;
check("legacyLower", () => {
  assert.equal(legacyLower.canonicalTimer.anchorAt, "0000-01-01T00:00:00Z", "legacy lower anchorAt");
  assert.equal(legacyLower.sessions[0].startedAt, "0000-01-01T00:00:00Z", "legacy lower startedAt");
});
check("legacyUpper", () => {
  assert.equal(legacyUpper.canonicalTimer.status, "completed", "legacy upper status");
  assert.equal(legacyUpper.canonicalTimer.anchorAt, LAST_INSTANT, "legacy upper anchorAt");
  assert.equal(legacyUpper.history[0].completedAt, LAST_INSTANT, "legacy upper completedAt");
  assert.equal(legacyUpper.sessions[0].endedAt, LAST_INSTANT, "legacy upper endedAt");
});
check("legacyLeap", () => {
  assert.equal(legacyLeap.canonicalTimer.status, "paused", "legacy leap status");
  assert.equal(legacyLeap.canonicalTimer.anchorAt, LAST_LEAP, "legacy leap anchorAt");
});
check("legacySuperseding", () => {
  assert.equal(legacySuperseding.canonicalTimer.id, "next", "legacy superseding current id");
  assert.equal(legacySuperseding.canonicalTimer.status, "running", "legacy superseding status");
  assert.equal(legacySuperseding.canonicalTimer.anchorAt, "9999-12-31T23:59:60.5Z", "legacy superseding anchorAt");
  assert.equal(legacySuperseding.history[0].timerId, "year", "legacy superseding history timerId");
  assert.equal(legacySuperseding.history[0].completedAt, LAST_INSTANT, "legacy superseding completedAt");
  assert.equal(legacySuperseding.sessions[1].timerId, "year", "legacy superseding session timerId");
  assert.equal(legacySuperseding.sessions[1].status, "completed", "legacy superseding session status");
  assert.equal(legacySuperseding.sessions[1].endedAt, LAST_INSTANT, "legacy superseding endedAt");
});
check("replayPageUpper", () => {
  assert.equal(replayPageUpper.canonicalTimer.status, "completed", "replay page status");
  assert.equal(replayPageUpper.canonicalTimer.anchorAt, LAST_INSTANT, "replay page anchorAt");
  assert.equal(replayPageUpper.history[0].completedAt, LAST_INSTANT, "replay page completedAt");
  assert.equal(replayPageUpper.sessions[0].endedAt, LAST_INSTANT, "replay page endedAt");
  assert.equal(replayPageUpper.currentTimerId, "year", "replay page currentTimerId");
  assert.deepEqual(replayPageUpper.after, { hlcWallMs: 1, hlcCounter: 0, deviceId: "device-a", id: "start" },
    "replay page after");
  assert.deepEqual(replayPageUpper.outcomes, { start: { outcome: "applied", reason: "" } }, "replay page outcomes");
});
check("workspaceLower", () =>
  assert.equal(first.workspaceLower.workspace.canonicalTimer.anchorAt, "0000-01-01T00:00:00Z", "workspace lower"));
check("workspaceLeap", () =>
  assert.equal(first.workspaceLeap.workspace.canonicalTimer.anchorAt, LAST_LEAP, "workspace leap"));
check("workspaceUpper", () => {
  const upper = first.workspaceUpper.workspace;
  assert.equal(upper.canonicalTimer.status, "completed", "workspace upper status");
  assert.equal(upper.canonicalTimer.anchorAt, LAST_INSTANT, "workspace upper anchorAt");
  assert.equal(upper.history[0].completedAt, LAST_INSTANT, "workspace upper completedAt");
  assert.equal(upper.history[0].endedAt, LAST_INSTANT, "workspace upper endedAt");
});
check("readModelDeadline", () => {
  assert.equal(first.readModelDeadline.canonical.status, "running", "read model deadline status");
  assert.equal(first.readModelDeadline.canonical.deadlineAt, "9999-12-31T23:59:59.999Z", "read model deadlineAt");
});
check("readModelReplay", () => {
  assert.equal(first.readModelReplay.canonical.status, "completed", "read model replay status");
  assert.equal(first.readModelReplay.canonical.elapsedMs, 60000, "read model replay elapsedMs");
});
check("intentObservation", () =>
  assert.deepEqual(first.intentObservation.timerObservation, { timerId: "existing-timer", elapsedMs: 15000,
    remainingMs: 45000, deadlineAt: "9999-12-31T23:59:59.999Z" }, "intent observation"));
for (const name of ["appleOverlayCentralized", "appleOverlayIroh"]) {
  check(name, () => assert.deepEqual(first[name].projection.canonicalTimer, { ...intentFixture.timer,
    status: "paused", anchorAt: LAST_INSTANT,
    lastIntent: { type: "start", commandId: "retained-start", occurredAt: "9999-12-31T23:00:00Z" } }, name));
}

// The legacy reducer rejects a timer that overlaps history, so reopen each
// field alone; workspace reopens feed back the complete projected base, and
// the replay page continues from its own sessions and cursor. A missing first
// value is already reported above, so only existing values reopen.
const reopenPlan = [
  { name: "legacyLower", key: "canonicalTimer", now: "0000-01-01T00:00:00Z" },
  { name: "legacyUpper", key: "canonicalTimer", now: LAST_INSTANT },
  { name: "legacyUpper", key: "history", now: LAST_INSTANT },
  { name: "legacyLeap", key: "canonicalTimer", now: LAST_LEAP },
  { name: "legacySuperseding", key: "canonicalTimer", now: LAST_LEAP },
  { name: "legacySuperseding", key: "history", now: LAST_LEAP },
  { name: "workspaceLower" }, { name: "workspaceLeap" }, { name: "workspaceUpper" },
  { name: "replayPageUpper", page: true },
].flatMap((plan) => Array.from({ length: REOPENS }, (_, round) => ({ ...plan, round })))
  .filter(({ name }) => first[name] !== undefined);

function reopenItem({ name, key, now, page, round }) {
  if (key) {
    return item(`${name}/${key}/reopen-${round}`, "timer.reduce.v1",
      { commands: [], now, [key]: first[name][key] }, true);
  }
  if (page) {
    const { sessions, currentTimerId, after } = first[name];
    return item(`${name}/reopen-${round}`, "timer.replay.page.v1",
      { commands: [], sessions, currentTimerId, after, now: LAST_INSTANT }, true);
  }
  const input = structuredClone(accepted[name][1]);
  input.base.canonicalTimer = first[name].workspace.canonicalTimer;
  input.base.history = first[name].workspace.history;
  return item(`${name}/reopen-${round}`, "workspace.project.v1", input, true);
}

// A replay page reports outcomes only for its own commands.
run(reopenPlan.map(reopenItem)).forEach(({ value }, index) => {
  const { name, key, page, round } = reopenPlan[index];
  if (key) check(`${name} ${key} reopen ${round}`, () => assert.deepEqual(value[key], first[name][key]));
  else if (page) check(`${name} reopen ${round}`, () => assert.deepEqual(value, { ...first[name], outcomes: {} }));
  else check(`${name} reopen ${round} changed the complete value`, () => assert.deepEqual(value, first[name]));
});

check("R54-C02 parity call counts", () => assert.deepEqual(calls, { "timer.reduce.v1": 28,
  "workspace.project.v1": 15, "workspace.readModel.v1": 2, "workspace.intent.v1": 3, "timer.replay.page.v1": 5,
  rejected: 33 }));
const total = Object.values(calls).reduce((sum, count) => sum + count, 0);
if (failures.length > 0) {
  console.error(failures.join("\n"));
  console.error(`R54-C02: ${failures.length} failed checks in ${total} raw native/WASM parity calls`);
  process.exit(1);
}
const perOperation = Object.entries(calls).map(([name, count]) => `${name} ${count}`).join(", ");
console.log(`R54-C02: ${total} complete raw native/WASM parity calls passed (${perOperation}); `
  + `${rejections.length} out-of-range rejections, ${acceptedNames.length} exact bounds, ${REOPENS} reopens`);
