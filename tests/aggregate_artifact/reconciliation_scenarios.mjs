import { fixture, vector } from "./cases.mjs";
import { branch } from "./semantics.mjs";

function response(first, outcome) {
  return { acknowledgements: [{ commandId: first.commands[0].id, outcome, reason: outcome === "rejected" ? "conflict" : "" }],
    taskAcknowledgements: [], durationAcknowledgements: [], autoStartAcknowledgements: [], selectedTaskAcknowledgements: [],
    revision: 1, canonicalTimer: null, history: first.projection.history,
    tasks: [], selectedTaskId: null, durationsMs: first.projection.durationsMs,
    autoStartBreaks: true, serverTime: "2026-07-20T12:00:12Z", serverHlcWallMs: 1784548812000, serverHlcCounter: 0 };
}

function rebaseRequest(first, finished) {
  const sent = fixture("workspace-intent-v1").request.workspace.local;
  sent.commands = [first.commands[0]];
  return { local: finished.workspace.local, sent, response: response(first, "applied"),
    timerDependencies: finished.workspace.timerDependencies,
    neverSent: { commands: finished.workspace.local.commands.slice(1).map((item) => item.id) } };
}

function batchRequest(commands, dependencies) {
  const queues = fixture("workspace-intent-v1").request.workspace.local;
  queues.commands = commands.map(({ id, deviceId, deviceSequence, hlcWallMs, hlcCounter }) =>
    ({ id, deviceId, deviceSequence, hlcWallMs, hlcCounter }));
  return { kind: "new", mode: "sync", queues, limits: { perDomain: 256, total: 512 }, nextDomain: "commands",
    timerDependencies: dependencies.map(({ operationId, dependsOnOperationId }) => ({ operationId, dependsOnOperationId })) };
}

export function reconcileChildren(call, profile, first, finished) {
  const prefix = `${profile}-generated`;
  const request = rebaseRequest(first, finished);
  const held = call(branch("reconcile.rebase.v2", `${prefix}-focus-ack`, request, "barrierRebase", {
    equals: { promotedTimerOperationIds: [first.commands[1].id] },
    lengths: { pending: 3, pendingTimerDependencies: 2, droppedTimerOperationIds: 0 } }));
  call(branch("sync.batchPlan.v1", `${prefix}-child-barrier`, batchRequest(held.pending, held.pendingTimerDependencies),
    "barrier", { equals: { "selected.commands": [first.commands[1].id],
      heldTimerOperationId: finished.workspace.local.commands[2].id }, lengths: { "selected.commands": 1 } }));
  const rejectedRequest = { ...request, response: response(first, "rejected") };
  const descendants = finished.workspace.local.commands.slice(1).map((item) => item.id).sort();
  const rejected = call(branch("reconcile.rebase.v2", `${prefix}-rejection`, rejectedRequest, "rejection", {
    lengths: { pending: 0, pendingTimerDependencies: 0, droppedTimerOperationIds: 3, promotedTimerOperationIds: 0 },
    equals: { droppedTimerOperationIds: descendants } }));
  call(vector("sync.batchPlan.v1", `${prefix}-rejected-drain`, batchRequest(rejected.pending, rejected.pendingTimerDependencies)));
  rejectStart(call, prefix, first, held);
}

function rejectStart(call, prefix, first, held) {
  const local = fixture("workspace-intent-v1").request.workspace.local;
  local.commands = held.pending;
  const sent = structuredClone(local);
  sent.commands = [held.pending[0]];
  const canonical = response(first, "rejected");
  canonical.acknowledgements[0].commandId = held.pending[0].id;
  const input = { local, sent, response: canonical, timerDependencies: held.pendingTimerDependencies,
    neverSent: { commands: held.pending.slice(1).map((item) => item.id) } };
  call(branch("reconcile.rebase.v2", `${prefix}-start-rejection`, input, "rejection", {
    lengths: { pending: 0, pendingTimerDependencies: 0, droppedTimerOperationIds: 2 },
    equals: { promotedTimerOperationIds: [], droppedTimerOperationIds: held.pending.slice(1).map((item) => item.id).sort() } }));
}
