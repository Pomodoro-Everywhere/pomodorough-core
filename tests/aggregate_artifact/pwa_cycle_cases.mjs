import { fixture, vector } from "./cases.mjs";
import { branch } from "./semantics.mjs";
import { selectionRequest } from "./pwa_selection_cases.mjs";
import { naturalRequest, naturalRead, naturalInstall } from "./pwa_natural_cases.mjs";
import { uuid } from "./pwa_display_cases.mjs";

export const cycleHit = "pwaCycleRepair", evidenceHit = "pwaDischargeRepair";
const operation = "workspace.intent.v1";

function rawError(operation, name, input, error) {
  return { ...vector(operation, name, input, false, error), rejectionHit: "pwaDischargeShape" };
}

export function repairCases() {
  const cases = publicCycleCases();
  for (const owner of ["local", "foreign", "peer", "missing"]) for (const record of ["only", "with-presentation", "empty-evidence"]) {
    const input = naturalRequest(), marker = { timerId: input.requestedTimer.id, phase: "focus", commandId: "fabricated-finish" };
    input.lifecycle = { consumedCompletions: [marker], pendingBreaks: [] };
    if (record === "with-presentation") input.lifecycle.consumedCompletions.unshift({ ...marker, commandId: null });
    if (record === "empty-evidence") input.lifecycle.finishEvidence = [];
    if (owner === "foreign") input.ownership.deviceId = "foreign-owner";
    if (owner === "peer") input.ownership.tabId = "peer";
    if (owner === "missing") input.ownership = null;
    cases.push(rawError("workspace.completionMutation.v1", `discharge-fabricated-${owner}-${record}`, input, "consumed Finish lacks"));
    cases.push(rawError("workspace.readModel.v1", `discharge-read-${owner}-${record}`, naturalRead(input), "consumed Finish lacks"));
    const intent = selectionRequest(); intent.lifecycle = input.lifecycle;
    cases.push(rawError(operation, `discharge-intent-${owner}-${record}`, intent, "consumed Finish lacks"));
  }
  return cases;
}

function publicCycleCases() {
  return fixture("pwa-cycle-public-v1").receipts.flatMap((receipt) => {
    if (receipt.calls) return receipt.calls.map((call, index) => rawError(call.operation,
      `cycle-public-${receipt.case}-${index}`, call.inputRaw, "consumed Finish lacks"));
    const cycles = receipt.first ? [receipt.first, receipt.second] : [receipt];
    return cycles.flatMap((cycle, index) => ["choice", "start", "read", "finish"].map((field) => {
      const call = cycle[field];
      return branch(call.operation, `cycle-public-${receipt.case}-${index}-${field}`, call.inputRaw, "pwaCycleSource", { equals: call.completeReturn });
    }));
  });
}

function cycleCall(call, operation, name, input, checks, hit = cycleHit) {
  return call(branch(operation, name, input, hit, checks));
}

function successor(input, output, kind, at) {
  return { ...input, intent: { kind }, workspace: output.workspace, allocation: output.allocation,
    observation: output.observation, selection: output.selection, lifecycle: output.lifecycle,
    clock: { occurredAt: at, physicalNow: at, observedAt: at },
    identities: { commandUuids: [uuid(at, 100), uuid(at, 101)], timerUuid: "33333333-3333-4333-8333-333333333333" } };
}

export function cycleScenarios(call) {
  for (const phase of ["focus", "short_break", "long_break"]) {
    const input = selectionRequest(phase); input.lifecycle.finishEvidence = [];
    const chosen = cycleCall(call, operation, `cycle-choice-${phase}`, input, { equals: { "selection.explicit": true, "selection.generation": "8" } });
    const start = successor(input, chosen, "start", input.clock.occurredAt);
    const started = cycleCall(call, operation, `cycle-start-${phase}`, start, {
      equals: { "commands.*.type": ["start"], "selection.explicit": false, "selection.generation": "8" }, same: { "workspace.base": "workspace.base" } });
    const deadline = new Date(Date.parse(started.commands[0].occurredAt) + started.commands[0].plannedDurationMs).toISOString();
    const next = successor(start, started, "restart", deadline), destination = phase === "focus" ? "short_break" : "focus";
    const readInput = naturalRead(next), count = phase === "focus" ? 2 : 1;
    cycleCall(call, "workspace.readModel.v1", `cycle-deadline-${phase}`, readInput, {
      equals: { "display.phase": destination, "canonical.status": "completed", "cadence.completedFocusTotal": count,
        availableIntents: ["start", "selectPhase", "finish"] } });
    const observed = cycleCall(call, operation, `cycle-observe-restart-${phase}`, next, {
      equals: { outcome: "noop", commands: [] }, same: { selection: "selection", allocation: "allocation", "workspace.base": "workspace.base" } });
    const actual = cycleCall(call, "workspace.project.v1", `cycle-current-projection-${phase}`, { ...started.workspace, now: deadline }, {
      equals: { "workspace.canonicalTimer.id": started.commands[0].timerId }, lengths: { "workspace.history": 2 } });
    const finish = { ...naturalRequest(), ...next, stage: "finishCommit", requestedTimer: started.projection.canonicalTimer,
      ownership: { timerId: started.commands[0].timerId, deviceId: started.allocation.deviceId, tabId: "p222-tab-0", leaseExpiresAtMs: Date.parse(deadline) + 60000 } };
    delete finish.intent;
    const completed = cycleCall(call, "workspace.completionMutation.v1", `cycle-finish-${phase}`, finish, {
      equals: { outcome: "planned", "selection.phase": destination, "selection.explicit": false, "selection.generation": "8" },
      lengths: { commands: 1, "lifecycle.finishEvidence": 1, "projection.history": 2 }, same: { "workspace.base": "workspace.base" } });
    const explicit = { ...next, intent: { kind: "selectPhase", phase }, workspace: started.workspace };
    const guarded = cycleCall(call, operation, `cycle-current-choice-${phase}`, explicit, {
      equals: { "selection.explicit": true, "selection.generation": "9" }, same: { "workspace.base": "workspace.base" } });
    const protectedRead = naturalRead({ ...explicit, selection: guarded.selection, lifecycle: guarded.lifecycle });
    cycleCall(call, "workspace.readModel.v1", `cycle-current-protected-${phase}`, protectedRead, { equals: { "display.phase": phase, "cadence.completedFocusTotal": count } });
    const protectedFinish = structuredClone({ ...finish, selection: guarded.selection, lifecycle: guarded.lifecycle });
    protectedFinish.workspace.base.autoStartBreaks = true;
    cycleCall(call, "workspace.completionMutation.v1", `cycle-current-protected-finish-${phase}`,
      protectedFinish, {
        equals: { outcome: "planned" }, lengths: { commands: 1 }, same: { selection: "selection", "workspace.base": "workspace.base" } });
    const consumed = naturalRead({ ...next, lifecycle: guarded.lifecycle });
    cycleCall(call, "workspace.readModel.v1", `cycle-already-consumed-${phase}`, consumed, { equals: { "display.phase": phase } });
    cycleCall(call, "workspace.readModel.v1", `cycle-completed-reopen-${phase}`, naturalRead({ ...next, ...completed }), {
      equals: { "display.phase": destination, "cadence.completedFocusTotal": count } });
    evidenceScenarios(call, finish, completed, actual.workspace.canonicalTimer, phase);
    if (phase === "focus") evidenceShapeScenarios(call, finish, completed);
    if (observed.selection.explicit) throw new Error("Restart observation cannot restore a previous cycle choice.");
  }
}

function evidenceShapeScenarios(call, input, completed) {
  const contract = fixture("pwa-finish-evidence-schema-v1"), old = fixture("pwa-completion-shapes-v1").fields;
  const fields = { "": "object" };
  for (const [field, record] of Object.entries(contract.fields)) {
    for (const [path, shape] of Object.entries(old)) if (path === record.schemaPath || path.startsWith(record.schemaPath + "/")) {
      fields[field + path.slice(record.schemaPath.length)] = shape;
    }
  }
  const request = { ...input, lifecycle: completed.lifecycle };
  for (const [path, shape] of Object.entries(fields)) for (const operation of ["workspace.completionMutation.v1", "workspace.readModel.v1", "timer.completionState.v1"]) {
    const invalid = structuredClone(request), keys = path.split("/").filter(Boolean);
    let target = invalid.lifecycle.finishEvidence;
    for (const key of ["0", ...keys.slice(0, -1)]) {
      if (target[key] === undefined || target[key] === null) target[key] = {};
      target = target[key];
    }
    if (!keys.length) invalid.lifecycle.finishEvidence[0] = [];
    else target[keys.at(-1)] = shape.endsWith("string") ? { finish: null } : [];
    const raw = operation === "workspace.readModel.v1" ? naturalRead(invalid) : operation === "timer.completionState.v1" ? naturalInstall(invalid) : invalid;
    call(rawError(operation, `discharge-structure-${operation}-${path || "record"}`, raw));
  }
  for (const [name, change] of evidenceMutations()) {
    const invalid = structuredClone(request); change(invalid.lifecycle);
    for (const operation of ["workspace.completionMutation.v1", "workspace.readModel.v1", "timer.completionState.v1"]) {
      const raw = operation === "workspace.readModel.v1" ? naturalRead(invalid) : operation === "timer.completionState.v1" ? naturalInstall(invalid) : invalid;
      call(rawError(operation, `discharge-invalid-${operation}-${name}`, raw));
    }
  }
}

function evidenceMutations() {
  return [
    ["null", (state) => { state.finishEvidence = null; }],
    ["object", (state) => { state.finishEvidence = {}; }],
    ...["command", "sourceTimer", "sourceHistory"].map((field) => [`missing-${field}`, (state) => { delete state.finishEvidence[0][field]; }]),
    ["unknown-policy", (state) => { state.finishEvidence[0].ownerGranted = true; }],
    ["duplicate", (state) => { state.finishEvidence.push(structuredClone(state.finishEvidence[0])); }],
    ["missing-consumption", (state) => { state.consumedCompletions = state.consumedCompletions.filter((row) => row.commandId === null); }],
    ["native-device-shape", (state) => { state.finishEvidence[0].sourceTimer.lastIntent.deviceId = []; }],
    ...Object.entries({ id: "fabricated-finish", timerId: "other", phase: "long_break", type: "start", plannedDurationMs: 60000,
      occurredAt: "2026-08-31T00:00:00Z" }).map(([field, value]) => [`command-${field}`, (state) => { state.finishEvidence[0].command[field] = value; }]),
  ];
}

function evidenceScenarios(call, input, completed, current, phase) {
  const original = { ...input, requestedTimer: current, lifecycle: completed.lifecycle };
  // Keep the original natural raw input. Only the Core-returned durable evidence discharges it.
  cycleCall(call, "workspace.completionMutation.v1", `discharge-original-reopen-${phase}`, original, {
    equals: { outcome: "noop", reason: "alreadyConsumed", commands: [] }, same: { allocation: "allocation", workspace: "workspace" } }, evidenceHit);
  for (const state of ["remote", "cleared", "replaced"]) {
    const raw = structuredClone(completed.workspace); raw.local.commands = []; raw.neverSent.commands = [];
    raw.displayContext.projectionPending.commands = [];
    raw.base.history = structuredClone(completed.projection.history);
    const timer = structuredClone(completed.projection.canonicalTimer);
    timer.lastIntent.commandId = "remote-provenance";
    raw.base.history.find((row) => row.timerId === timer.id).commandId = "remote-provenance";
    raw.base.canonicalTimer = state === "cleared" ? null : state === "replaced"
      ? { ...timer, id: "replacement", status: "paused", elapsedAtAnchorMs: 0, lastIntent: null } : timer;
    const read = naturalRead({ ...input, workspace: raw, selection: completed.selection, lifecycle: completed.lifecycle });
    cycleCall(call, "workspace.readModel.v1", `discharge-preserved-${phase}-${state}`, read, { equals: { "cadence.completedFocusTotal": phase === "focus" ? 2 : 1 } }, evidenceHit);
    const install = naturalInstall({ ...input, workspace: raw, selection: completed.selection, lifecycle: completed.lifecycle });
    install.beforeHistory = input.workspace.base.history;
    install.sentContext.commands = completed.commands;
    install.acknowledgements = [{ commandId: completed.commands[0].id, outcome: "rejected" }];
    cycleCall(call, "timer.completionState.v1", `discharge-install-${phase}-${state}`, install, { same: { selection: "selection", lifecycle: "lifecycle" } }, evidenceHit);
  }
}
