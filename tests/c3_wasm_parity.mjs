import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const wasm = await readFile(process.argv[2]);
const { instance } = await WebAssembly.instantiate(wasm);
const encoder = new TextEncoder();
const decoder = new TextDecoder("utf-8", { fatal: true });

function allocate(bytes) {
  const pointer = Number(instance.exports.pomodorough_alloc(bytes.length));
  new Uint8Array(instance.exports.memory.buffer, pointer, bytes.length).set(bytes);
  return pointer;
}

function invoke(operation, input) {
  const operationBytes = encoder.encode(operation);
  const inputBytes = encoder.encode(input);
  const operationPointer = allocate(operationBytes);
  const inputPointer = allocate(inputBytes);
  const packed = instance.exports.pomodorough_dispatch(
    operationPointer,
    operationBytes.length,
    inputPointer,
    inputBytes.length,
  );
  const outputPointer = Number(packed & 0xffff_ffffn);
  const outputLength = Number((packed >> 32n) & 0xffff_ffffn);
  const output = new Uint8Array(
    instance.exports.memory.buffer,
    outputPointer,
    outputLength,
  ).slice();
  instance.exports.pomodorough_free_v2(operationPointer, operationBytes.length);
  instance.exports.pomodorough_free_v2(inputPointer, inputBytes.length);
  instance.exports.pomodorough_free_v2(outputPointer, outputLength);
  return JSON.parse(decoder.decode(output));
}

function task(title) {
  const result = invoke("task.identity.v1", JSON.stringify({ title }));
  assert.equal(result.ok, true);
  return result.value;
}

function queues() {
  return {
    commands: [],
    taskOperations: [],
    durationOperations: [],
    autoStartOperations: [],
    selectedTaskOperations: [],
  };
}

function durations() {
  return { focus: 1_200_000, short_break: 300_000, long_break: 900_000 };
}

function clock(id, wallOffset) {
  return {
    id,
    deviceId: "device-c3",
    occurredAt: "2026-07-20T12:00:11Z",
    hlcWallMs: 1_784_548_810_000 + wallOffset,
    hlcCounter: 0,
  };
}

function response(tasks = []) {
  return {
    acknowledgements: [],
    taskAcknowledgements: [],
    durationAcknowledgements: [],
    autoStartAcknowledgements: [],
    selectedTaskAcknowledgements: [],
    revision: 7,
    canonicalTimer: null,
    history: [],
    tasks,
    durationsMs: durations(),
    autoStartBreaks: false,
    selectedTaskId: null,
    serverTime: "2026-07-20T12:00:10Z",
    serverHlcWallMs: 1_784_548_810_000,
    serverHlcCounter: 0,
  };
}

function rebase(local, sent, canonical) {
  return invoke(
    "reconcile.rebase.v1",
    JSON.stringify({ local, sent, response: canonical, timerDependencies: [] }),
  );
}

function queueValue(field, id) {
  const operation = clock(id, 1_000);
  if (field === "commands") {
    return {
      ...operation,
      deviceSequence: 1,
      timerId: "timer-c3",
      type: "start",
      phase: "focus",
      plannedDurationMs: 1_200_000,
      observedElapsedMs: 0,
    };
  }
  if (field === "taskOperations") {
    return { ...operation, taskId: "task-c3", type: "delete", title: "" };
  }
  if (field === "durationOperations") {
    return { ...operation, phase: "focus", durationMs: 1_200_000 };
  }
  if (field === "autoStartOperations") {
    return { ...operation, enabled: true };
  }
  return { ...operation, taskId: null };
}

function acknowledgementFields(field) {
  if (field === "commands") return ["acknowledgements", "commandId"];
  if (field === "taskOperations") return ["taskAcknowledgements", "operationId"];
  if (field === "durationOperations") return ["durationAcknowledgements", "operationId"];
  if (field === "autoStartOperations") return ["autoStartAcknowledgements", "operationId"];
  return ["selectedTaskAcknowledgements", "operationId"];
}

function acknowledgedInput(field, operation, timerDependencies = []) {
  const [responseField, idField] = acknowledgementFields(field);
  const local = queues();
  local[field].push(structuredClone(operation));
  const sent = queues();
  sent[field].push({ id: operation.id });
  const canonical = response();
  canonical[responseField].push({
    [idField]: operation.id,
    outcome: "applied",
    reason: "",
  });
  return { local, sent, response: canonical, timerDependencies };
}

function projectionInput(pending) {
  return {
    base: {
      canonicalTimer: null,
      history: [],
      tasks: [],
      durationsMs: durations(),
      autoStartBreaks: false,
      selectedTaskId: null,
    },
    pending,
    now: "2026-07-20T12:00:10Z",
  };
}

function projectionFromRebase(value) {
  return {
    base: {
      canonicalTimer: value.baseTimer,
      history: value.baseHistory,
      tasks: value.baseTasks,
      durationsMs: value.baseDurationsMs,
      autoStartBreaks: value.baseAutoStartBreaks,
      selectedTaskId: value.baseSelectedTaskId,
    },
    pending: {
      commands: value.pending,
      taskOperations: value.pendingTaskOperations,
      durationOperations: value.pendingDurationOperations,
      autoStartOperations: value.pendingAutoStartOperations,
      selectedTaskOperations: value.pendingSelectedTaskOperations,
    },
    now: "2026-07-20T12:00:10Z",
  };
}

function assertProjectionMatchesRebase(value) {
  const projection = invoke(
    "projection.apply.v2",
    JSON.stringify(projectionFromRebase(value)),
  );
  assert.equal(projection.ok, true);
  for (const [rebaseField, projectionField] of [
    ["timer", "canonicalTimer"],
    ["history", "history"],
    ["tasks", "tasks"],
    ["durationsMs", "durationsMs"],
    ["autoStartBreaks", "autoStartBreaks"],
    ["selectedTaskId", "selectedTaskId"],
  ]) {
    assert.deepEqual(value[rebaseField], projection.value[projectionField]);
  }
}

function assertRebaseProjectionReject(field, operation) {
  const pending = queues();
  pending[field].push(structuredClone(operation));
  const projected = invoke(
    "projection.apply.v2",
    JSON.stringify(projectionInput(pending)),
  );
  assert.equal(projected.ok, false, `projection accepted ${field}`);

  const local = queues();
  local[field].push(structuredClone(operation));
  const retained = rebase(local, queues(), response());
  assert.equal(retained.ok, false, `reconciliation retained ${field}`);

  const acknowledged = invoke(
    "reconcile.rebase.v1",
    JSON.stringify(acknowledgedInput(field, operation)),
  );
  assert.equal(acknowledged.ok, false, `reconciliation acknowledged ${field}`);
}

const alpha = task("Alpha");
const beta = task("Beta");
const local = queues();
local.taskOperations.push({
  ...clock("task-beta", 1_000),
  taskId: beta.id,
  type: "upsert",
  title: beta.title,
});
local.durationOperations.push({
  ...clock("duration-short", 1_001),
  phase: "short_break",
  durationMs: 600_000,
});
local.autoStartOperations.push({ ...clock("auto-start", 1_002), enabled: true });
local.selectedTaskOperations.push({ ...clock("select-beta", 1_003), taskId: beta.id });
local.commands.push({
  ...clock("timer-beta", 1_004),
  deviceSequence: 1,
  timerId: "timer-beta",
  type: "start",
  phase: "focus",
  plannedDurationMs: 1_200_000,
  observedElapsedMs: 0,
  taskId: beta.id,
});

const rebased = rebase(local, queues(), response([alpha]));
assert.equal(rebased.ok, true);
assert.equal(rebased.value.timer.taskId, beta.id);
assert.equal(rebased.value.selectedTaskId, beta.id);

const projected = invoke(
  "projection.apply.v2",
  JSON.stringify({
    base: {
      canonicalTimer: rebased.value.baseTimer,
      history: rebased.value.baseHistory,
      tasks: rebased.value.baseTasks,
      durationsMs: rebased.value.baseDurationsMs,
      autoStartBreaks: rebased.value.baseAutoStartBreaks,
      selectedTaskId: rebased.value.baseSelectedTaskId,
    },
    pending: {
      commands: rebased.value.pending,
      taskOperations: rebased.value.pendingTaskOperations,
      durationOperations: rebased.value.pendingDurationOperations,
      autoStartOperations: rebased.value.pendingAutoStartOperations,
      selectedTaskOperations: rebased.value.pendingSelectedTaskOperations,
    },
    now: "2026-07-20T12:00:10Z",
  }),
);
assert.equal(projected.ok, true);
for (const [rebaseField, projectionField] of [
  ["timer", "canonicalTimer"],
  ["history", "history"],
  ["tasks", "tasks"],
  ["durationsMs", "durationsMs"],
  ["autoStartBreaks", "autoStartBreaks"],
  ["selectedTaskId", "selectedTaskId"],
]) {
  assert.deepEqual(rebased.value[rebaseField], projected.value[projectionField]);
}

const forgedCanonical = rebase(
  queues(),
  queues(),
  response([{ id: "forged-task", title: "Alpha" }]),
);
assert.equal(forgedCanonical.ok, false);
assert.match(forgedCanonical.error, /invalid canonical response tasks/);

const forgedOperation = {
  ...clock("forged-upsert", 1_000),
  taskId: "forged-task",
  type: "upsert",
  title: "Beta",
};
const forgedLocal = queues();
forgedLocal.taskOperations.push(forgedOperation);
const forgedSent = queues();
forgedSent.taskOperations.push({ id: "forged-upsert" });
const acknowledged = response();
acknowledged.taskAcknowledgements.push({
  operationId: "forged-upsert",
  outcome: "applied",
  reason: "",
});
const forgedAcknowledged = rebase(forgedLocal, forgedSent, acknowledged);
assert.equal(forgedAcknowledged.ok, false);
assert.match(forgedAcknowledged.error, /invalid task identity or title/);

for (const invalid of [
  { focus: 1_200_000, short_break: 300_000 },
  { ...durations(), custom: 600_000 },
]) {
  const canonical = response();
  canonical.durationsMs = invalid;
  const invalidRebase = rebase(queues(), queues(), canonical);
  assert.equal(invalidRebase.ok, false);
  assert.match(invalidRebase.error, /invalid canonical response durationsMs/);

  const invalidProjection = invoke(
    "projection.apply.v2",
    JSON.stringify({
      base: {
        canonicalTimer: null,
        history: [],
        tasks: [],
        durationsMs: invalid,
        autoStartBreaks: false,
        selectedTaskId: null,
      },
      pending: queues(),
      now: "2026-07-20T12:00:10Z",
    }),
  );
  assert.equal(invalidProjection.ok, false);
  assert.match(invalidProjection.error, /invalid base durations/);
}

for (const field of [
  "commands",
  "taskOperations",
  "durationOperations",
  "autoStartOperations",
  "selectedTaskOperations",
]) {
  const acknowledged = invoke(
    "reconcile.rebase.v1",
    JSON.stringify(acknowledgedInput(field, queueValue(field, `ack-${field}`))),
  );
  assert.equal(acknowledged.ok, true);
  assertProjectionMatchesRebase(acknowledged.value);

  const invalidClock = queueValue(field, `invalid-clock-${field}`);
  invalidClock.hlcWallMs = field === "commands" ? 0 : -1;
  assertRebaseProjectionReject(field, invalidClock);
}

const invalidDomainValues = [
  ["commands", "phase", "custom"],
  ["taskOperations", "type", "replace"],
  ["durationOperations", "phase", "custom"],
  ["autoStartOperations", "enabled", "yes"],
  ["selectedTaskOperations", "taskId", ""],
];
for (const [field, key, value] of invalidDomainValues) {
  const operation = queueValue(field, `invalid-${key}-${field}`);
  operation[key] = value;
  assertRebaseProjectionReject(field, operation);
}

for (const [field, value] of [
  ["id", ""],
  ["deviceId", ""],
  ["deviceSequence", 0],
  ["timerId", ""],
  ["phase", "custom"],
  ["plannedDurationMs", 1],
  ["occurredAt", "not-a-timestamp"],
  ["hlcWallMs", 0],
  ["hlcCounter", -1],
  ["observedElapsedMs", 9_007_199_254_740_992],
]) {
  const command = queueValue("commands", `invalid-command-${field}`);
  command[field] = value;
  assertRebaseProjectionReject("commands", command);
}

const invalidParent = queueValue("commands", "acknowledged-parent");
invalidParent.phase = "custom";
const dependencyInput = acknowledgedInput("commands", invalidParent, [
  {
    operationId: "retained-child",
    dependsOnOperationId: "acknowledged-parent",
  },
]);
dependencyInput.local.commands.push(
  queueValue("commands", "retained-child"),
);
const dependencyResult = invoke(
  "reconcile.rebase.v1",
  JSON.stringify(dependencyInput),
);
assert.equal(dependencyResult.ok, false);

console.log("C3 WASM parity: passed");
