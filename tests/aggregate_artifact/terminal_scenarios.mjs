import assert from "node:assert/strict";
import { vector } from "./cases.mjs";
import { generatedRequest } from "./generated_scenarios.mjs";
import { terminalRequest } from "./terminal_cases.mjs";
import { branch } from "./semantics.mjs";

function canonicalResponse(projection, command, outcome) {
  const response = terminalRequest().response;
  for (const field of ["canonicalTimer", "history", "tasks", "durationsMs", "autoStartBreaks", "selectedTaskId"]) {
    response[field] = projection[field];
  }
  Object.assign(response, { acknowledgements: [{ commandId: command.id, outcome, reason: "" }],
    serverTime: command.occurredAt, serverHlcWallMs: command.hlcWallMs, serverHlcCounter: command.hlcCounter });
  return response;
}

function generatedInput(call) {
  const input = generatedRequest("pwaStorage", 3);
  const task = call(vector("task.identity.v1", "terminal-task-identity", { title: "Actual attributed focus" }));
  input.workspace.base.tasks = [task];
  input.workspace.base.canonicalTimer.taskId = task.id;
  input.requestedTimer = input.workspace.base.canonicalTimer;
  const plan = call(vector("workspace.completionMutation.v1", "terminal-generated-plan", input));
  assert.equal(plan.commands.length, 2);
  assert.equal(plan.commands[1].phase, "long_break");
  assert.equal(Object.hasOwn(plan.commands[0], "taskId"), false);
  const sent = structuredClone(input.workspace.local);
  sent.commands = [plan.commands[0]];
  const projection = call(vector("projection.apply.v2", "terminal-http-like-focus", {
    base: input.workspace.base, pending: sent, now: plan.commands[0].occurredAt }));
  const local = structuredClone(plan.workspace.local);
  local.commands.forEach((command) => { command.extension = { null: null, empty: "", nested: [false, {}] }; });
  const start = local.commands[1];
  local.commands.push({ ...start, id: "terminal-child", type: "pause",
    deviceSequence: start.deviceSequence + 1, hlcCounter: start.hlcCounter + 1 });
  return { local, sent, response: canonicalResponse(projection, plan.commands[0], "applied"),
    timerDependencies: [...plan.workspace.timerDependencies, { operationId: "terminal-child", dependsOnOperationId: start.id }],
    neverSent: { commands: [start.id, "terminal-child"] } };
}

function nextInput(call, input, first, outcome) {
  const local = { ...input.local, commands: first.pending };
  const sent = { ...input.sent, commands: [first.pending[0]] };
  let projection = input.response;
  if (outcome === "applied") {
    const base = Object.fromEntries(["canonicalTimer", "history", "tasks", "durationsMs", "autoStartBreaks", "selectedTaskId"]
      .map((field) => [field, input.response[field]]));
    projection = call(vector("workspace.project.v1", "terminal-http-like-start", {
      base, local: sent, neverSent: { commands: [first.pending[0].id] }, timerDependencies: [],
      canonicalHead: { wallMs: input.response.serverHlcWallMs, counter: input.response.serverHlcCounter },
      now: input.response.serverTime })).workspace;
  }
  return { local, sent, response: canonicalResponse(projection, first.pending[0], outcome),
    timerDependencies: first.pendingTimerDependencies, neverSent: { commands: ["terminal-child"] } };
}

function composeCompletion(call, output, input) {
  const canonical = output.canonicalResponse;
  const state = { kind: "install", compatibility: "desktopD03", beforeHistory: [], afterHistory: canonical.history,
    canonicalTimer: canonical.canonicalTimer, selection: { phase: "focus", generation: "0", explicit: false },
    pending: { commandIds: [], sendableCommandIds: [], otherOperationIds: [] }, advances: [],
    acknowledgements: canonical.acknowledgements.map(({ commandId, outcome }) => ({ commandId, outcome })),
    discardedCommandIds: [], referenceTime: canonical.serverTime,
    calendarIntervals: [{ start: "2026-10-02T00:00:00Z", end: "2026-10-03T00:00:00Z" }] };
  call(branch("timer.completionState.v1", "terminal-raw-desktop-install", state, "terminalComposition", {
    equals: { reason: "completionSelected", "selection.phase": "short_break", "source.commandId": input.sent.commands[0].id } }));
  Object.assign(state, { compatibility: "pwaRejectedFinish", selection: { ...state.selection, phase: "short_break" },
    sentContext: { kind: "pwa", commands: input.sent.commands, rollbackHistory: canonical.history } });
  call(branch("timer.completionState.v1", "terminal-raw-pwa-install", state, "terminalComposition", {
    equals: { reason: "sentFinishesReconciled", "selection.phase": "short_break" } }));
}

function normalizeGenerated(call, input) {
  const normalized = structuredClone(input);
  for (const command of normalized.local.commands.slice(1)) {
    command.phase = "short_break";
    command.plannedDurationMs = 120000;
  }
  const pending = normalized.local.commands.slice(1).map((command) => ({ ...command,
    phase: "long_break", plannedDurationMs: 180000 }));
  call(branch("reconcile.rebase.v3", "terminal-generated-normalization", normalized, "terminalNormalization", {
    equals: { pending, "projectionPending.commands": pending }, same: { canonicalResponse: "response" } }));
  call(vector("reconcile.rebase.v3", "terminal-frozen-normalization-denied",
    { ...normalized, neverSent: {} }, false, "possibly delivered operation"));
}

export function terminalScenarios(call) {
  const input = generatedInput(call);
  normalizeGenerated(call, input);
  const first = call(branch("reconcile.rebase.v3", "terminal-focus-ack", input, "terminalBarrier", {
    equals: { promotedTimerOperationIds: [input.local.commands[1].id], "timer.status": "paused" },
    lengths: { pending: 2, pendingTimerDependencies: 1 },
    same: { canonicalResponse: "response", baseTimer: "response.canonicalTimer" } }));
  const rejected = nextInput(call, input, first, "rejected");
  call(branch("reconcile.rebase.v3", "terminal-start-rejection", rejected, "terminalRejection", {
    equals: { pending: [], droppedTimerOperationIds: ["terminal-child"], pendingTimerDependencies: [], "timer.status": "completed" } }));
  const frozen = { ...rejected, neverSent: {} };
  call(vector("reconcile.rebase.v3", "terminal-frozen-drop-denied", frozen, false, "possibly delivered dependent"));
  const applied = nextInput(call, input, first, "applied");
  call(branch("reconcile.rebase.v3", "terminal-start-applied", applied, "terminalPromotion", {
    equals: { promotedTimerOperationIds: ["terminal-child"], pendingTimerDependencies: [], "timer.status": "paused",
      pending: [first.pending[1]] }, same: { canonicalResponse: "response" } }));
  const actual = terminalRequest();
  const installed = call(vector("reconcile.rebase.v3", "terminal-composition-source", actual));
  composeCompletion(call, installed, actual);
}
