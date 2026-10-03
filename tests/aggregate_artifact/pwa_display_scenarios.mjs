import assert from "node:assert/strict";
import { fixture, vector } from "./cases.mjs";
import { branch } from "./semantics.mjs";
import { context, domains, emptyQueues, fields, finishRequest, intentRequest, rawWorkspace, readRequest, uuid } from "./pwa_display_cases.mjs";
import { terminalRequest } from "./terminal_cases.mjs";

function next(input, output, at, kind) {
  const request = intentRequest(output.workspace, at, kind);
  for (const field of ["allocation", "observation", "selection"]) request[field] = output[field];
  request.requestedTimer = output.projection.canonicalTimer;
  return request;
}

function finish(input, output, at, auto) {
  const request = { ...finishRequest(output.workspace, at), ...next(input, output, at, "pause") };
  delete request.intent;
  if (auto) {
    request.identities.commandUuids.push(uuid(at, 2));
    request.identities.timerUuid = "22345678-1234-4234-8234-123456789012";
    Object.assign(request, { localTabId: "p222-tab-0", leaseNowMs: Date.parse(at), leaseDurationMs: 60000 });
  }
  return request;
}

function response(workspace, command, outcome) {
  const result = terminalRequest().response;
  for (const name of fields) result[name] = workspace[name];
  Object.assign(result, { acknowledgements: [{ commandId: command.id, outcome, reason: "" }],
    serverTime: command.occurredAt, serverHlcWallMs: command.hlcWallMs, serverHlcCounter: command.hlcCounter });
  return result;
}

function claim(call, output, name) {
  const batch = call(vector("sync.batchPlan.v1", `${name}-claim`, { kind: "new", mode: "sync", nextDomain: "commands",
    queues: Object.fromEntries(domains.map((name) => [name, output.workspace.local[name].map((operation) =>
      Object.fromEntries(["id", "deviceId", "hlcWallMs", "hlcCounter", ...(name === "commands" ? ["deviceSequence"] : [])]
        .map((field) => [field, operation[field]])))])), timerDependencies: output.workspace.timerDependencies
          .map(({ operationId, dependsOnOperationId }) => ({ operationId, dependsOnOperationId })),
    limits: { perDomain: 256, total: 512 } }));
  const workspace = structuredClone(output.workspace);
  for (const domain of domains) workspace.neverSent[domain] = (workspace.neverSent[domain] || [])
    .filter((id) => !batch.selected[domain].includes(id));
  return JSON.parse(JSON.stringify({ ...output, workspace }));
}

function controls(call, request, name, status) {
  const output = call(branch("workspace.readModel.v1", name, readRequest(request.workspace, request.clock.physicalNow), "pwaLifecycle", {
    equals: { "canonical.status": status, availableIntents: status === "paused"
      ? ["resume", "finish", "cancel", "cancelAndClear"] : ["pause", "finish", "cancel", "cancelAndClear"] } }));
  assert.equal(output.canonical.timerId, request.requestedTimer.id);
}

function pendingLifecycle(call, auto) {
  let workspace = rawWorkspace(1);
  workspace.local.commands = [];
  workspace.neverSent = emptyQueues();
  workspace.base.autoStartBreaks = auto;
  const input = intentRequest(workspace, "2026-08-31T12:00:00Z", "start");
  input.identities.commandUuids = [uuid(input.clock.occurredAt, 22)];
  input.observation.commandTimes = {};
  const started = call(branch("workspace.intent.v1", `pwa-null-start-${auto}`, input, "pwaLifecycle", {
    equals: { "projection.canonicalTimer.status": "running", "commandOutcomes.0.outcome": "applied", "workspace.base.canonicalTimer": null },
    lengths: { "workspace.displayContext.projectionPending.commands": 1 } }));
  const claimed = claim(call, started, `pwa-start-${auto}`);
  claimed.workspace.canonicalHead = { wallMs: claimed.commands[0].hlcWallMs, counter: claimed.commands[0].hlcCounter };
  const pause = next(input, claimed, "2026-08-31T12:00:01Z", "pause");
  controls(call, pause, `pwa-claimed-reload-${auto}`, "running");
  const paused = call(branch("workspace.intent.v1", `pwa-covered-pause-${auto}`, pause, "pwaLifecycle", {
    equals: { "projection.canonicalTimer.status": "paused", "commandOutcomes.0.outcome": "applied" },
    prefixes: { "workspace.local.commands": "workspace.local.commands" }, lengths: { "workspace.displayContext.projectionPending.commands": 2 } }));
  const resume = next(input, paused, "2026-08-31T12:00:02Z", "resume");
  controls(call, resume, `pwa-paused-reload-${auto}`, "paused");
  const resumed = call(branch("workspace.intent.v1", `pwa-covered-resume-${auto}`, resume, "pwaLifecycle", {
    equals: { "projection.canonicalTimer.status": "running", "commandOutcomes.0.outcome": "applied" } }));
  const request = finish(input, resumed, "2026-08-31T12:00:03Z", auto);
  const completed = call(branch("workspace.completionMutation.v1", `pwa-covered-finish-${auto}`, request, "pwaLifecycle", {
    equals: { "commands.*.type": auto ? ["finish", "start"] : ["finish"], "selection.phase": "short_break",
      "projection.canonicalTimer.status": auto ? "running" : "completed", "commandOutcomes.*.outcome": auto ? ["applied", "applied"] : ["applied"] } }));
  assert.deepEqual(completed.workspace.local.commands.slice(0, 3), resumed.workspace.local.commands);
  if (auto) generatedAcknowledgements(call, completed);
}

function generatedAcknowledgements(call, completed) {
  const breakCanonical = completed.projection;
  const childRequest = next(null, completed, "2026-08-31T12:00:04Z", "pause");
  const child = call(branch("workspace.intent.v1", "pwa-display-generated-child", childRequest, "pwaLifecycle", {
    equals: { "projection.canonicalTimer.status": "paused", "commandOutcomes.0.outcome": "applied" },
    lengths: { "workspace.timerDependencies": 2 } }));
  completed = { ...child, commands: completed.commands };
  const claimed = claim(call, completed, "pwa-generated");
  const source = completed.commands[0];
  const sourceQueues = { ...emptyQueues(), commands: completed.workspace.local.commands.filter((command) => command.timerId === source.timerId) };
  const terminal = call(vector("workspace.project.v1", "pwa-server-focus", { base: completed.workspace.base, local: sourceQueues,
    canonicalHead: null, neverSent: {}, timerDependencies: [], displayContext: context(null), now: source.occurredAt })).workspace;
  const sent = structuredClone(sourceQueues);
  const canonical = response(terminal, source, "applied");
  canonical.acknowledgements = sourceQueues.commands.map((command) => ({ commandId: command.id, outcome: "applied", reason: "" }));
  const request = { local: claimed.workspace.local, sent, response: canonical, neverSent: completed.workspace.neverSent,
    timerDependencies: completed.workspace.timerDependencies, displayContext: completed.workspace.displayContext };
  request.neverSent.commands = [completed.commands[1].id, child.commands[0].id];
  const first = call(branch("reconcile.rebase.v3", "pwa-display-focus-ack", request, "pwaRebase", {
    equals: { promotedTimerOperationIds: [completed.commands[1].id], "timer.status": "paused" },
    lengths: { pending: 2, "displayContext.projectionPending.commands": 2, "projectionPending.commands": 2 }, same: { canonicalResponse: "response" } }));
  normalizeContext(call, request, first);
  const start = first.pending[0];
  const rejected = { local: { ...emptyQueues(), commands: first.pending }, sent: { ...emptyQueues(), commands: [start] },
    neverSent: { commands: [first.pending[1].id] }, timerDependencies: first.pendingTimerDependencies, displayContext: first.displayContext,
    response: response(terminal, start, "rejected") };
  call(branch("reconcile.rebase.v3", "pwa-display-start-rejected", rejected, "pwaRebase", {
    equals: { pending: [], "displayContext.projectionPending.commands": [], "timer.status": "completed", droppedTimerOperationIds: [first.pending[1].id] }, same: { canonicalResponse: "response" } }));
  const accepted = structuredClone(rejected);
  accepted.response = response(breakCanonical, start, "applied");
  call(branch("reconcile.rebase.v3", "pwa-display-start-accepted", accepted, "pwaRebase", {
    equals: { pending: [first.pending[1]], "displayContext.projectionPending.commands": [first.pending[1]], "timer.status": "paused", promotedTimerOperationIds: [first.pending[1].id] }, same: { canonicalResponse: "response" } }));
  call(vector("reconcile.rebase.v3", "pwa-display-claimed-child-drop-denied", { ...rejected, neverSent: {} }, false, "possibly delivered dependent"));
}

function normalizeContext(call, request, first) {
  const normalized = structuredClone(request);
  const ids = new Set(first.pending.map((command) => command.id));
  const extension = { null: null, empty: "", nested: [false, {}] };
  for (const command of normalized.local.commands.filter((command) => ids.has(command.id))) {
    Object.assign(command, { phase: "long_break", plannedDurationMs: 60000, extension });
  }
  normalized.displayContext = context(structuredClone(normalized.local));
  const expected = first.pending.map((command) => ({ ...command, extension }));
  call(branch("reconcile.rebase.v3", "pwa-display-normalized-context", normalized, "pwaRebase", {
    equals: { pending: expected, "displayContext.projectionPending.commands": expected }, same: { canonicalResponse: "response" } }));
  call(vector("reconcile.rebase.v3", "pwa-display-frozen-normalization-denied", { ...normalized, neverSent: {} }, false, "possibly delivered operation"));
}

export function displayScenarios(call) {
  pendingLifecycle(call, false);
  pendingLifecycle(call, true);
  const request = intentRequest(rawWorkspace(), "2026-08-31T12:00:01Z", "selectPhase");
  request.intent.phase = "long_break";
  call(branch("workspace.intent.v1", "pwa-display-explicit-phase", request, "pwaLifecycle", {
    equals: { "selection.phase": "long_break", commands: [] }, same: { "workspace.base": "workspace.base", "workspace.local": "workspace.local" } }));
  automaticFinish(call);
  preferenceParity(call);
  durationContext(call);
}

function durationContext(call) {
  const request = fixture("workspace-intent-v1").request;
  Object.assign(request, { compatibility: "pwaStorage", intent: { kind: "setDuration", phase: "focus", minutes: 30 },
    ownership: { ownerId: null, expectedOwnerId: null }, durability: { outgoingDurationOperationIds: [], localTabId: "p222-tab-0" } });
  request.workspace.displayContext = context(emptyQueues());
  const first = call(vector("workspace.intent.v1", "pwa-duration-context-initial", request));
  const later = next(request, first, "2026-07-20T12:00:11Z", "setDuration");
  later.intent = { kind: "setDuration", phase: "focus", minutes: 45 };
  later.ownership = request.ownership;
  later.durability = request.durability;
  later.calendarIntervals = request.calendarIntervals;
  const replacement = call(branch("workspace.intent.v1", "pwa-duration-context-retired", later, "pwaParity", {
    equals: { retiredDurationOperationIds: [first.operations.durationOperations[0].id], "projection.durationsMs.focus": 2700000 },
    lengths: { "workspace.local.durationOperations": 1, "workspace.displayContext.projectionPending.durationOperations": 1 } }));
  assert.deepEqual(replacement.workspace.displayContext.projectionPending.durationOperations, replacement.workspace.local.durationOperations);
  const claimed = structuredClone(later);
  claimed.workspace.neverSent.durationOperations = [];
  claimed.durability.outgoingDurationOperationIds = [first.operations.durationOperations[0].id];
  call(branch("workspace.intent.v1", "pwa-duration-context-claimed", claimed, "pwaParity", {
    equals: { retiredDurationOperationIds: [], "groupOutcomes.durationOperations.0.outcome": "applied", "projection.durationsMs.focus": 2700000 },
    lengths: { "workspace.displayContext.projectionPending.durationOperations": 2 },
    prefixes: { "workspace.local.durationOperations": "workspace.local.durationOperations" } }));
  claimed.workspace.displayContext.projectionPending.durationOperations = [];
  call(branch("workspace.intent.v1", "pwa-duration-hidden-claimed", claimed, "pwaAdmission", {
    equals: { retiredDurationOperationIds: [], "groupOutcomes.durationOperations.0.outcome": "queued", "projection.durationsMs.focus": request.workspace.base.durationsMs.focus },
    same: { "workspace.displayContext.projectionPending": "workspace.displayContext.projectionPending" },
    prefixes: { "workspace.local.durationOperations": "workspace.local.durationOperations" } }));
}

function automaticFinish(call) {
  const request = finishRequest(rawWorkspace(), "2026-08-31T12:25:01Z");
  const project = call(vector("workspace.project.v1", "pwa-automatic-unexpired", { ...request.workspace, now: "1970-01-01T00:00:00Z" }));
  request.requestedTimer = project.workspace.canonicalTimer;
  Object.assign(request, { stage: "automaticFinishCommit", localTabId: "p222-tab-0", leaseNowMs: Date.parse(request.clock.physicalNow),
    ownership: { timerId: request.requestedTimer.id, deviceId: "p222-device", tabId: "p222-tab-0", leaseExpiresAtMs: 1788177660000 } });
  call(branch("workspace.completionMutation.v1", "pwa-display-automatic-finish", request, "pwaLifecycle", {
    equals: { "commands.*.type": ["finish"], "commandOutcomes.*.outcome": ["applied"], "projection.canonicalTimer.status": "completed" } }));
  call(branch("workspace.completionMutation.v1", "pwa-display-foreign-owner", { ...request, ownership: { ...request.ownership, deviceId: "other-device" } }, "pwaLifecycle", {
    equals: { outcome: "noop", reason: "not_owner", commands: [], ownershipWrites: [] }, same: { "workspace.local": "workspace.local", "workspace.neverSent": "workspace.neverSent" } }));
}

function preferenceParity(call) {
  const task = fixture("workspace-intent-desktop-known-tasks-v1").knownTasks[0];
  for (const intent of [{ kind: "upsertTask", title: "Display task" }, { kind: "addAndSelectTask", title: "Display task" },
    { kind: "selectTask", taskId: task.id }, { kind: "deleteTask", taskId: task.id },
    { kind: "setDuration", phase: "focus", minutes: 30 }, { kind: "setAutoStart", enabled: true }]) {
    const request = fixture("workspace-intent-v1").request;
    Object.assign(request, { compatibility: "pwaStorage", intent, ownership: { ownerId: null, expectedOwnerId: null },
      durability: { outgoingDurationOperationIds: [], localTabId: "p222-tab-0" } });
    request.workspace.base.tasks = [task];
    const baseline = call(vector("workspace.intent.v1", `pwa-preference-baseline-${intent.kind}`, request));
    request.workspace.displayContext = context(emptyQueues());
    const result = call(branch("workspace.intent.v1", `pwa-preference-parity-${intent.kind}`, request, "pwaParity", {
      equals: { projection: baseline.projection, operations: baseline.operations, groupOutcomes: baseline.groupOutcomes },
      same: { "workspace.base": "workspace.base" } }));
    const compared = structuredClone(result);
    delete compared.workspace.displayContext;
    assert.deepEqual(compared, baseline);
  }
}
