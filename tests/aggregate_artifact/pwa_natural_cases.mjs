import { fixture, vector } from "./cases.mjs";
import { branch } from "./semantics.mjs";
import { emptyQueues, uuid, fields } from "./pwa_display_cases.mjs";

const operation = "workspace.completionMutation.v1";
export const naturalRequest = (index = 0) => fixture("pwa-natural-completion-v1").receipts[index].request.input;

export function naturalHttpRequest() {
  const http = fixture("pwa-natural-completion-v1").http, input = naturalRequest(), raw = JSON.parse(http.naturalResponseRaw);
  input.workspace.base = Object.fromEntries(fields.map((field) => [field, raw[field]]));
  input.workspace.canonicalHead = { wallMs: raw.serverHlcWallMs, counter: raw.serverHlcCounter };
  input.requestedTimer = raw.canonicalTimer;
  input.allocation = { deviceId: http.startRequest.deviceId, deviceSequence: 1,
    hlc: input.workspace.canonicalHead, lastUuid: null };
  input.observation = { canonicalAnchorAt: raw.canonicalTimer.anchorAt, commandTimes: {} };
  input.clock = Object.fromEntries(["occurredAt", "physicalNow", "observedAt"].map((field) => [field, raw.serverTime]));
  input.ownership = { ...input.ownership, timerId: raw.canonicalTimer.id, deviceId: http.startRequest.deviceId };
  input.leaseNowMs = Date.parse(raw.serverTime); input.ownership.leaseExpiresAtMs = input.leaseNowMs + 60000;
  input.identities.commandUuids = [uuid(raw.serverTime, 1), uuid(raw.serverTime, 2)];
  input.calendarIntervals = [{ start: "2026-10-03T00:00:00Z", end: "2026-10-04T00:00:00Z" }];
  return input;
}

export function naturalRead(input) {
  return { profile: "pwaStorage", source: { kind: "workspace", value: input.workspace },
    selectedPhase: input.selection.phase, selection: input.selection, lifecycle: input.lifecycle || {},
    observedAt: input.clock.observedAt, calendarIntervals: input.calendarIntervals };
}

export function naturalInstall(input) {
  return { kind: "install", compatibility: "pwaRejectedFinish", beforeHistory: [],
    afterHistory: input.workspace.base.history, canonicalTimer: input.workspace.base.canonicalTimer,
    selection: input.selection, lifecycle: input.lifecycle || { consumedCompletions: [], pendingBreaks: [] },
    pending: { commandIds: [], sendableCommandIds: [], otherOperationIds: [] }, advances: [],
    acknowledgements: [], discardedCommandIds: [], referenceTime: input.clock.observedAt,
    calendarIntervals: input.calendarIntervals, sentContext: { kind: "pwa", commands: [], rollbackHistory: [] } };
}

function planned(name, input, count = 1, phase = "short_break") {
  return branch(operation, name, input, "pwaNatural", { equals: { outcome: "planned",
    "commands.0.type": "finish", "commands.0.timerId": input.requestedTimer.id,
    "commands.0.plannedDurationMs": input.requestedTimer.plannedDurationMs,
    "selection.phase": phase, "projection.history.0.id": input.workspace.base.history[0].id },
    lengths: { commands: count, "projection.history": input.workspace.base.history.length },
    same: { "workspace.base": "workspace.base" } });
}

export function naturalCases() {
  const cases = [0, 1, 2, 3].map((index) => planned(`natural-raw-public-${index}`, naturalRequest(index)));
  cases.push(planned("natural-raw-go-http", naturalHttpRequest()));
  for (const automatic of [false, true]) for (const offset of [-1, 0, 1]) {
    const input = naturalRequest();
    input.stage = automatic ? "automaticFinishCommit" : "finishCommit";
    input.ownership.tabId = "peer";
    input.ownership.leaseExpiresAtMs = input.leaseNowMs + offset;
    const name = `natural-lease-${automatic}-${offset}`;
    cases.push(offset <= 0 ? planned(name, input) : branch(operation, name, input, "pwaNatural", {
      equals: { outcome: "noop", reason: "not_owner", retryAtMs: input.leaseNowMs + offset, commands: [] },
      same: { workspace: "workspace", allocation: "allocation", selection: "selection" } }));
  }
  for (const owner of [null, "foreign"]) {
    const input = naturalRequest();
    input.ownership = owner === null ? null : { ...input.ownership, deviceId: owner };
    cases.push(owner === null ? planned("natural-missing-owner", input) : branch(operation, "natural-foreign-owner", input, "pwaNatural", {
      equals: { outcome: "noop", reason: "not_owner", commands: [] }, same: { allocation: "allocation", workspace: "workspace" } }));
  }
  const explicit = naturalRequest();
  explicit.selection = { phase: "long_break", generation: "7", explicit: true };
  explicit.workspace.base.autoStartBreaks = true;
  cases.push(planned("natural-explicit-choice", explicit, 1, "long_break"));
  const read = naturalRead(naturalRequest());
  cases.push(branch("workspace.readModel.v1", "natural-display-finish", read, "pwaNatural", {
    equals: { "canonical.status": "completed", "display.phase": "short_break", "cadence.completedFocusTotal": 1,
      "cadence.completedFocusToday": 1, availableIntents: ["start", "selectPhase", "finish"] } }));
  cases.push(branch("timer.completionState.v1", "natural-install-selection", naturalInstall(naturalRequest()), "pwaNatural", {
    equals: { "selection.phase": "short_break", reason: "completionSelected", "source.commandId": null },
    lengths: { "lifecycle.consumedCompletions": 1 } }));
  for (const outcome of ["applied", "ignored", "rejected"]) {
    const source = naturalRequest(), input = naturalInstall(source);
    input.selection.phase = "short_break"; input.beforeHistory = structuredClone(input.afterHistory);
    input.sentContext.rollbackHistory = structuredClone(input.afterHistory);
    input.sentContext.commands = [{ id: "pending-finish", timerId: source.requestedTimer.id,
      type: "finish", phase: "focus", deviceSequence: 9, occurredAt: source.clock.occurredAt }];
    input.acknowledgements = [{ commandId: "pending-finish", outcome }];
    cases.push(branch("timer.completionState.v1", `natural-ack-${outcome}`, input, "pwaNatural", {
      same: { selection: "selection", lifecycle: "lifecycle" } }));
  }
  const stale = naturalRequest(); stale.requestedTimer.id = "wrong-target";
  cases.push(branch(operation, "natural-wrong-target", stale, "pwaNatural", {
    equals: { outcome: "noop", reason: "staleTimer", commands: [] }, same: { allocation: "allocation", workspace: "workspace" } }));
  const replaced = naturalRequest();
  replaced.workspace.local.commands = [{ id: "claimed-replacement", deviceId: replaced.allocation.deviceId,
    deviceSequence: 9, timerId: "replacement", type: "start", phase: "focus", plannedDurationMs: 1500000,
    occurredAt: replaced.clock.occurredAt, hlcWallMs: replaced.leaseNowMs, hlcCounter: 0, observedElapsedMs: 0 }];
  replaced.allocation.deviceSequence = 9; replaced.allocation.hlc = { wallMs: replaced.leaseNowMs, counter: 0 };
  cases.push(branch(operation, "natural-frozen-replacement", replaced, "pwaNatural", {
    equals: { outcome: "noop", reason: "staleTimer", commands: [] }, same: { allocation: "allocation", workspace: "workspace" } }));
  for (const field of ["taskId", "phase", "plannedDurationMs", "completedAt", "commandId"]) {
    const input = naturalRequest();
    input.workspace.base.history[0][field] = { phase: "short_break", plannedDurationMs: 60000,
      completedAt: "2026-08-31T12:25:01Z", commandId: "forged", taskId: "forged" }[field];
    cases.push({ ...vector(operation, `natural-invalid-pair-${field}`, input, false, "conflicting workspace terminal"), rejectionHit: "pwaNatural" });
  }
  for (const field of ["sourceAccepted", "ownerGranted", "automatic", "generateAutoBreak"]) {
    const input = { ...naturalRequest(), [field]: true };
    cases.push({ ...vector(operation, `natural-caller-policy-${field}`, input, false), rejectionHit: "pwaNatural" });
  }
  const foreignRead = naturalRead(naturalRequest()); foreignRead.profile = "androidCoordinator";
  delete foreignRead.source.value.displayContext;
  cases.push({ ...vector("workspace.readModel.v1", "natural-foreign-read-context", foreignRead, false, "requires PWA profile"), rejectionHit: "pwaNatural" });
  const invalidState = naturalRequest();
  invalidState.lifecycle = { consumedCompletions: [{ timerId: "", commandId: null, phase: "focus" }], pendingBreaks: [] };
  cases.push({ ...vector(operation, "natural-invalid-consumption", invalidState, false, "invalid consumed completion"), rejectionHit: "pwaNatural" });
  for (const [index, lifecycle] of [null, [[], []], { consumedCompletions: [["timer", null, "focus"]] },
    { pendingBreaks: [["finish", "timer", 1]] }].entries()) {
    for (const name of [operation, "workspace.readModel.v1", "timer.completionState.v1"]) {
      const input = name === operation ? naturalRequest() : name === "workspace.readModel.v1"
        ? naturalRead(naturalRequest()) : naturalInstall(naturalRequest());
      input.lifecycle = lifecycle;
      cases.push({ ...vector(name, `natural-lifecycle-shape-${index}`, input, false, "JSON"), rejectionHit: "pwaNatural" });
    }
  }
  const tupleRead = naturalRead(naturalRequest()); tupleRead.selection = ["focus", "0", false];
  cases.push({ ...vector("workspace.readModel.v1", "natural-selection-tuple", tupleRead, false, "JSON object"), rejectionHit: "pwaNatural" });
  return cases;
}

export function naturalScenarios(call) {
  for (const phase of ["focus", "short_break", "long_break"]) {
    const input = naturalRequest();
    input.workspace.base.canonicalTimer.phase = phase;
    input.workspace.base.history[0].phase = phase;
    input.requestedTimer.phase = phase;
    input.selection.phase = phase;
    input.workspace.base.autoStartBreaks = true;
    const count = phase === "focus" ? 2 : 1, destination = phase === "focus" ? "short_break" : "focus";
    const output = call(branch(operation, `natural-auto-${phase}`, input, "pwaNaturalFlow", {
      equals: { "commands.*.type": count === 2 ? ["finish", "start"] : ["finish"], "selection.phase": destination },
      lengths: { "projection.history": 1 }, same: { "workspace.base": "workspace.base" } }));
    const retry = { ...input, workspace: output.workspace, allocation: output.allocation,
      observation: output.observation, selection: output.selection, lifecycle: output.lifecycle,
      identities: { commandUuids: [nextUuid(output.allocation.lastUuid)], timerUuid: null } };
    retry.workspace.neverSent.commands = [];
    retry.workspace.displayContext.projectionPending = emptyQueues();
    call(branch(operation, `natural-frozen-retry-${phase}`, retry, "pwaNaturalFlow", {
      equals: { outcome: "noop", commands: [] }, same: { workspace: "workspace", allocation: "allocation", lifecycle: "lifecycle" } }));
    const consumed = naturalInstall(input);
    consumed.beforeHistory = input.workspace.base.history;
    consumed.lifecycle = output.lifecycle;
    call(branch("timer.completionState.v1", `natural-reinstall-${phase}`, consumed, "pwaNaturalFlow", {
      same: { selection: "selection", lifecycle: "lifecycle" } }));
  }
  const fourth = naturalRequest();
  fourth.workspace.base.autoStartBreaks = true;
  const previous = [1, 2, 3].map((index) => ({ ...fourth.workspace.base.history[0], id: `prior-${index}`,
    timerId: `prior-${index}`, completedAt: `2026-08-31T0${index}:00:00Z`, endedAt: `2026-08-31T0${index}:00:00Z` }));
  fourth.workspace.base.history.push(...previous);
  fourth.workspace.base.durationsMs.long_break = 1800000;
  const completed = call(plannedFlow("natural-fourth-long", fourth));
  const model = naturalRead({ ...fourth, workspace: completed.workspace, selection: completed.selection, lifecycle: completed.lifecycle });
  call(branch("workspace.readModel.v1", "natural-fourth-counts", model, "pwaNaturalFlow", {
    equals: { "cadence.completedFocusTotal": 4, "cadence.completedFocusToday": 4,
      "cadence.completedFocusTodayPlannedDurationMs": 6000000, "canonical.phase": "long_break" } }));
}

function plannedFlow(name, input) {
  return branch(operation, name, input, "pwaNaturalFlow", { equals: { "selection.phase": "long_break",
    "commands.*.type": ["finish", "start"], "commands.1.phase": "long_break", "commands.1.plannedDurationMs": 1800000 },
    lengths: { "projection.history": 4 }, same: { "workspace.base": "workspace.base" } });
}

function nextUuid(value) {
  const hex = (BigInt(`0x${value.replaceAll("-", "")}`) + 1n).toString(16).padStart(32, "0");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}
