import assert from "node:assert/strict";
import { fixture, vector } from "./cases.mjs";
import { branch } from "./semantics.mjs";
import { context, domains, emptyQueues, readRequest, uuid } from "./pwa_display_cases.mjs";
import { terminalRequest } from "./terminal_cases.mjs";

export function admissionCases() {
  const cases = [];
  for (const domain of domains.filter((name) => name !== "commands")) {
    for (const stored of [false, true]) {
      for (const barrier of ["none", "claim", "stale"]) {
        const request = fixture("workspace-projection-v1").request;
        const old = request.local[domain][0];
        const fresh = { ...old, id: "fresh", hlcCounter: 12, extension: { null: null, omitted: [] } };
        request.local[domain] = [old, fresh];
        request.canonicalHead = barrier === "stale" ? request.canonicalHead : null;
        if (barrier === "stale") request.canonicalHead.counter = 11;
        request.neverSent[domain] = barrier === "claim" ? [fresh.id] : [old.id, fresh.id];
        request.displayContext = context(emptyQueues());
        if (stored) request.displayContext.projectionPending[domain] = [old];
        const admitted = stored || barrier === "none";
        cases.push(branch("workspace.project.v1", `pwa-admit-${domain}-${stored}-${barrier}`, request, "pwaAdmission", {
          equals: { [`displayContext.projectionPending.${domain}`]: admitted ? [old, fresh] : [], [`projectionPending.${domain}`]: [] } }));
      }
    }
  }
  cases.push(...preferenceNegatives());
  cases.push(...newRowBarriers());
  cases.push(...preferenceAcknowledgements());
  return cases;
}

function newRowBarriers() {
  return domains.slice(1).flatMap((domain) => ["claim", "stale"].map((barrier) => {
    const request = fixture("workspace-projection-v1").request;
    const old = request.local[domain][0];
    const fresh = { ...old, id: "fresh", hlcCounter: 12 };
    request.local[domain] = [old, fresh];
    request.displayContext = context(emptyQueues());
    request.displayContext.projectionPending[domain] = [old];
    request.neverSent[domain] = barrier === "claim" ? [] : [fresh.id];
    request.canonicalHead = barrier === "claim" ? null : { wallMs: old.hlcWallMs, counter: 12 };
    return branch("workspace.project.v1", `pwa-admit-new-barrier-${domain}-${barrier}`, request, "pwaAdmission", {
      equals: { [`displayContext.projectionPending.${domain}`]: [old], [`projectionPending.${domain}`]: [] } });
  }));
}

function preferenceAcknowledgements() {
  return domains.slice(1).flatMap((domain, index) => {
    const workspace = fixture("workspace-projection-v1").request;
    const old = workspace.local[domain][0];
    const fresh = { ...old, id: "fresh", hlcCounter: 12, extension: { null: null, omitted: [] } };
    const request = terminalRequest();
    Object.assign(request, { local: { ...emptyQueues(), [domain]: [old, fresh] },
      sent: { ...emptyQueues(), [domain]: [old] }, neverSent: { [domain]: [fresh.id] }, timerDependencies: [],
      displayContext: context({ ...emptyQueues(), [domain]: [old, fresh] }) });
    Object.assign(request.response, workspace.base, { acknowledgements: [], serverTime: workspace.now,
      serverHlcWallMs: old.hlcWallMs, serverHlcCounter: 13 });
    const fields = ["taskAcknowledgements", "durationAcknowledgements", "autoStartAcknowledgements", "selectedTaskAcknowledgements"];
    for (const field of fields) request.response[field] = [];
    request.response[fields[index]] = [{ operationId: old.id, outcome: "applied", reason: "" }];
    const outputFields = ["pendingTaskOperations", "pendingDurationOperations", "pendingAutoStartOperations", "pendingSelectedTaskOperations"];
    const accepted = branch("reconcile.rebase.v3", `pwa-admit-ack-trim-${domain}`, request, "pwaAdmission", {
      equals: { [`displayContext.projectionPending.${domain}`]: [fresh], [outputFields[index]]: [fresh], [`projectionPending.${domain}`]: [] },
      same: { canonicalResponse: "response" } });
    const stale = structuredClone(request);
    stale.local[domain] = [fresh];
    stale.sent[domain] = [];
    stale.response[fields[index]] = [];
    return [accepted, vector("reconcile.rebase.v3", `pwa-admit-stale-context-${domain}`, stale, false, "retained payloads")];
  });
}

function preferenceNegatives() {
  const cases = [];
  for (const domain of domains.filter((name) => name !== "commands")) {
    const request = fixture("workspace-projection-v1").request;
    request.displayContext = context(structuredClone(request.local));
    request.local[domain][0].extension = { preserved: null };
    cases.push(vector("workspace.project.v1", `pwa-admit-extension-mismatch-${domain}`, request, false, "retained payloads"));
    request.displayContext.projectionPending[domain][0] = structuredClone(request.local[domain][0]);
    cases.push(branch("workspace.project.v1", `pwa-admit-exact-extension-${domain}`, request, "pwaAdmission", {
      same: { "displayContext.projectionPending": "displayContext.projectionPending" } }));
    request.neverSent[domain] = [request.local[domain][0].id, request.local[domain][0].id];
    cases.push(vector("workspace.project.v1", `pwa-admit-duplicate-proof-${domain}`, request, false, "delivery claim"));
  }
  return cases;
}

function advance(output, kind, index) {
  const input = fixture("pwa-display-admission-v1").request;
  input.workspace = JSON.parse(JSON.stringify(output.workspace));
  for (const field of ["selection", "allocation", "observation"]) input[field] = output[field];
  input.intent = { kind };
  input.requestedTimer = output.projection.canonicalTimer;
  input.identities.commandUuids = [0, 1, 2].map((offset) => uuid(input.clock.occurredAt, index + offset));
  if (["start", "pause", "resume"].includes(kind)) {
    input.identities.commandUuids = input.identities.commandUuids.slice(0, 2);
    delete input.ownership;
    delete input.durability;
  }
  return input;
}

function finish(output, index) {
  const input = advance(output, "pause", index);
  delete input.intent;
  Object.assign(input, { stage: "finishCommit", ownership: null, localTabId: "p222-tab-0",
    leaseNowMs: Date.parse(input.clock.physicalNow), leaseDurationMs: 60000 });
  input.identities.timerUuid = "22345678-1234-4234-8234-123456789012";
  return input;
}

function offlineBreak(call, phase, minutes, history) {
  const input = fixture("pwa-display-admission-v1").request;
  input.workspace.base.history = history;
  input.intent = { kind: "setDuration", phase, minutes };
  const edited = call(branch("workspace.intent.v1", `pwa-admit-duration-${phase}`, input, "pwaAdmission", {
    equals: { [`projection.durationsMs.${phase}`]: minutes * 60000, "groupOutcomes.durationOperations.0.outcome": "applied" },
    lengths: { "workspace.displayContext.projectionPending.durationOperations": 1 }, same: { "workspace.base": "workspace.base" } }));
  call(branch("workspace.readModel.v1", `pwa-admit-cadence-${phase}`, readRequest(edited.workspace), "pwaAdmission", {
    equals: { "cadence.completedFocusToday": history.length, "canonical.status": "idle" } }));
  const started = call(vector("workspace.intent.v1", `pwa-admit-start-${phase}`, advance(edited, "start", 23)));
  const paused = call(vector("workspace.intent.v1", `pwa-admit-pause-${phase}`, advance(started, "pause", 26)));
  const resumed = call(vector("workspace.intent.v1", `pwa-admit-resume-${phase}`, advance(paused, "resume", 29)));
  const completed = call(branch("workspace.completionMutation.v1", `pwa-admit-finish-${phase}`, finish(resumed, 32), "pwaAdmission", {
    equals: { "commands.*.type": ["finish", "start"], "commands.1.plannedDurationMs": minutes * 60000,
      "selection.phase": phase, "projection.canonicalTimer.status": "running", "projection.canonicalTimer.plannedDurationMs": minutes * 60000 } }));
  call(branch("workspace.project.v1", `pwa-admit-reopen-${phase}`, { ...JSON.parse(JSON.stringify(completed.workspace)), now: input.clock.physicalNow }, "pwaAdmission", {
    equals: { workspace: completed.projection, projectionPending: emptyQueues() } }));
}

function peerDisable(call) {
  const input = fixture("pwa-display-admission-v1").request;
  input.intent = { kind: "setAutoStart", enabled: false };
  input.workspace.neverSent.autoStartOperations = [];
  const output = call(branch("workspace.intent.v1", "pwa-admit-peer-disable", input, "pwaAdmission", {
    equals: { "projection.autoStartBreaks": false, "groupOutcomes.autoStartOperations.0.outcome": "applied",
      "workspace.neverSent.autoStartOperations": [input.identities.commandUuids[0]] },
    prefixes: { "workspace.local.autoStartOperations": "workspace.local.autoStartOperations" } }));
  const started = call(vector("workspace.intent.v1", "pwa-admit-peer-focus", advance(output, "start", 23)));
  call(branch("workspace.completionMutation.v1", "pwa-admit-peer-no-start", finish(started, 26), "pwaAdmission", {
    equals: { "commands.*.type": ["finish"], "projection.autoStartBreaks": false, "projection.canonicalTimer.status": "completed" } }));
}

function claimedTask(call) {
  const input = fixture("pwa-display-admission-v1").request;
  input.intent = { kind: "addAndSelectTask", title: "Claimed display task" };
  const added = call(vector("workspace.intent.v1", "pwa-admit-task-add", input));
  const started = call(vector("workspace.intent.v1", "pwa-admit-task-start", advance(added, "start", 23)));
  const batch = call(vector("sync.batchPlan.v1", "pwa-admit-task-claim", { kind: "new", mode: "sync", nextDomain: "commands",
    queues: Object.fromEntries(domains.map((name) => [name, started.workspace.local[name].map((row) =>
      Object.fromEntries(["id", "deviceId", "hlcWallMs", "hlcCounter", ...(name === "commands" ? ["deviceSequence"] : [])].map((key) => [key, row[key]])))])),
    timerDependencies: [], limits: { perDomain: 256, total: 512 } }));
  const reload = structuredClone(started);
  for (const name of domains) reload.workspace.neverSent[name] = reload.workspace.neverSent[name].filter((id) => !batch.selected[name].includes(id));
  call(branch("workspace.readModel.v1", "pwa-admit-task-claimed-read", readRequest(reload.workspace), "pwaAdmission", {
    equals: { "tasks.selectedTaskId": started.projection.selectedTaskId, "tasks.total": 1, "canonical.status": "running" } }));
  const selection = advance(reload, "selectTask", 26);
  selection.intent.taskId = null;
  const retargeted = call(branch("workspace.intent.v1", "pwa-admit-task-retarget", selection, "pwaAdmission", {
    equals: { "projection.selectedTaskId": null, "projection.canonicalTimer.taskId": undefined,
      "groupOutcomes.selectedTaskOperations.0.outcome": "applied", "commands.*.type": ["retarget"] },
    prefixes: { "workspace.local.selectedTaskOperations": "workspace.local.selectedTaskOperations", "workspace.local.commands": "workspace.local.commands" } }));
  const deletion = advance(retargeted, "deleteTask", 29);
  deletion.intent.taskId = started.projection.selectedTaskId;
  const deleted = call(branch("workspace.intent.v1", "pwa-admit-task-delete", deletion, "pwaAdmission", {
    equals: { "projection.tasks": [], "projection.selectedTaskId": null, "projection.canonicalTimer.taskId": undefined,
      "groupOutcomes.taskOperations.0.outcome": "applied", "groupOutcomes.selectedTaskOperations": [], commands: [] },
    prefixes: { "workspace.local.taskOperations": "workspace.local.taskOperations", "workspace.local.commands": "workspace.local.commands" } }));
  for (const name of domains) for (const id of batch.selected[name]) assert.ok(!deleted.workspace.neverSent[name].includes(id));
  call(branch("workspace.project.v1", "pwa-admit-task-delete-reopen", { ...deleted.workspace, now: deletion.clock.physicalNow }, "pwaAdmission", {
    equals: { workspace: deleted.projection, projectionPending: emptyQueues() } }));
}

export function admissionScenarios(call) {
  const history = [1, 2, 3].map((index) => ({ id: `p222-history-${index}`, timerId: `p222-timer-${index}`, commandId: `p222-finish-${index}`,
    phase: "focus", status: "completed", plannedDurationMs: 1500000, completedAt: `2026-08-31T${String(12 - index).padStart(2, "0")}:00:00.000Z`,
    endedAt: `2026-08-31T${String(12 - index).padStart(2, "0")}:00:00.000Z` }));
  offlineBreak(call, "short_break", 10, []);
  offlineBreak(call, "long_break", 30, history);
  peerDisable(call);
  claimedTask(call);
}
