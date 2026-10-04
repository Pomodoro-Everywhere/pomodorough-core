import { fixture, vector } from "./cases.mjs";
import { branch } from "./semantics.mjs";
import { naturalRequest, naturalRead, naturalInstall } from "./pwa_natural_cases.mjs";

const operation = "workspace.intent.v1";
export function selectionRequest(phase = "focus", status = "completed") {
  const input = naturalRequest();
  for (const key of ["stage", "ownership", "localTabId", "leaseNowMs", "leaseDurationMs"]) delete input[key];
  input.intent = { kind: "selectPhase", phase };
  input.selection.generation = "7";
  input.lifecycle = { consumedCompletions: [], pendingBreaks: [] };
  if (status === "idle") {
    input.workspace.base.canonicalTimer = null; input.workspace.base.history = [];
    input.requestedTimer = null; input.observation.canonicalAnchorAt = null;
  } else if (status !== "completed") {
    const timer = input.workspace.base.canonicalTimer;
    timer.status = status; timer.elapsedAtAnchorMs = 0;
    timer.anchorAt = "2026-08-31T12:00:00Z";
    if (["cancelled", "superseded"].includes(status)) delete timer.lastIntent;
    input.workspace.base.history = []; input.requestedTimer = structuredClone(timer);
    input.observation.canonicalAnchorAt = timer.anchorAt;
    input.clock = { occurredAt: "2026-08-31T12:01:00Z", physicalNow: "2026-08-31T12:01:00Z", observedAt: "2026-08-31T12:01:00Z" };
  }
  return input;
}

function choice(name, input, phase, generation = "8", hit = "pwaChoice") {
  return branch(operation, name, input, hit, { equals: {
    outcome: "planned", selection: { phase, generation, explicit: true }, commands: [], atomicCommandIds: [],
    ownershipWrites: [], effectsAfterCommit: [] }, same: { workspace: "workspace", allocation: "allocation" } });
}

export function selectionCases() {
  const cases = [];
  for (const receipt of fixture("pwa-selection-public-v1").receipts) {
    const publicCase = branch(operation, `choice-public-${receipt.phase}`, receipt.selected.inputRaw, "pwaChoice", {
      equals: receipt.selected.completeReturn });
    cases.push(publicCase);
    cases.push(branch(operation, `choice-legacy-public-${receipt.phase}`, receipt.legacyCompletePlanner.inputRaw, "pwaChoice", {
      equals: receipt.legacyCompletePlanner.completeReturn }));
  }
  for (const status of ["idle", "running", "paused", "completed", "cancelled", "superseded"]) {
    for (const phase of ["focus", "short_break", "long_break"]) {
      const input = selectionRequest(phase, status);
      input.identities = { commandUuids: [], timerUuid: null };
      cases.push(choice(`choice-${status}-${phase}`, input, phase));
    }
  }
  for (const [generation, next] of [["0", "1"], ["9007199254740991", "9007199254740992"], ["9223372036854775806", "9223372036854775807"]]) {
    const input = selectionRequest(); input.selection.generation = generation;
    cases.push(choice(`choice-generation-${generation}`, input, "focus", next));
  }
  const noop = selectionRequest(); noop.intent = { kind: "restart" }; noop.selection.generation = "9223372036854775807";
  cases.push(branch(operation, "choice-noop-exhausted", noop, "pwaChoice", {
    equals: { outcome: "noop", commands: [], effectsAfterCommit: [] },
    same: { workspace: "workspace", allocation: "allocation", selection: "selection", lifecycle: "lifecycle" } }));
  return cases;
}

export function selectionScenarios(call) {
  for (const status of ["running", "completed"]) for (const phase of ["focus", "short_break", "long_break"]) {
    const input = selectionRequest(phase, status);
    input.lifecycle.finishEvidence = [];
    const output = call(choice(`choice-flow-${status}-${phase}`, input, phase, "8", "pwaChoiceFlow"));
    const restored = JSON.parse(JSON.stringify({ ...input, selection: output.selection, lifecycle: output.lifecycle }));
    const read = naturalRead(restored); read.observedAt = "2026-08-31T12:25:00Z";
    call(branch("workspace.readModel.v1", `choice-expired-display-${status}-${phase}`, read, "pwaChoiceFlow", {
      equals: { "display.phase": phase, "canonical.status": "completed", "cadence.completedFocusTotal": 1 } }));
    const finish = naturalRequest(); finish.selection = output.selection; finish.lifecycle = output.lifecycle;
    finish.workspace.base.autoStartBreaks = true;
    const completed = call(branch("workspace.completionMutation.v1", `choice-finish-${status}-${phase}`, finish, "pwaChoiceFlow", {
      equals: { "commands.*.type": ["finish"] }, same: { selection: "selection", "workspace.base": "workspace.base" },
      lengths: { "projection.history": 1 } }));
    for (const outcome of ["applied", "ignored", "rejected"]) {
      const install = naturalInstall(finish);
      install.lifecycle = completed.lifecycle; install.beforeHistory = finish.workspace.base.history;
      install.sentContext.commands = completed.commands; install.sentContext.rollbackHistory = finish.workspace.base.history;
      install.acknowledgements = [{ commandId: completed.commands[0].id, outcome }];
      install.canonicalTimer = completed.projection.canonicalTimer; install.afterHistory = completed.projection.history;
      const chosen = call(choice(`choice-late-${status}-${phase}-${outcome}`, { ...restored, lifecycle: completed.lifecycle }, phase, "9", "pwaChoiceFlow"));
      install.selection = chosen.selection; install.lifecycle = chosen.lifecycle;
      call(branch("timer.completionState.v1", `choice-ack-${status}-${phase}-${outcome}`, install, "pwaChoiceFlow", {
        same: { selection: "selection", lifecycle: "lifecycle" } }));
      for (const timer of ["missing", "replacement"]) {
        const later = structuredClone(install);
        later.canonicalTimer = timer === "missing" ? null : { ...later.canonicalTimer,
          id: "newer-session", status: "paused", elapsedAtAnchorMs: 0, lastIntent: null };
        call(branch("timer.completionState.v1", `choice-ack-${status}-${phase}-${outcome}-${timer}`, later, "pwaChoiceFlow", {
          same: { selection: "selection", lifecycle: "lifecycle" } }));
      }
    }
  }
  let input = selectionRequest("focus", "idle"); input.intent = { kind: "skip" };
  for (const [index, phase] of ["short_break", "focus", "short_break"].entries()) {
    const output = call(choice(`choice-skip-${index}`, input, phase, String(8 + index), "pwaChoiceFlow"));
    input = { ...input, selection: output.selection, lifecycle: output.lifecycle };
  }
}

function rejected(name, input) {
  return { ...vector(operation, `choice-invalid-${name}`, input, false), rejectionHit: "pwaChoiceShape" };
}

function replace(input, path, value, omit = false) {
  const keys = path.split("."), last = keys.pop();
  const parent = keys.reduce((current, key) => current[key], input);
  if (omit) delete parent[last]; else parent[last] = value;
  return input;
}

export function selectionShapeCases() {
  const contract = fixture("pwa-selection-contract-v1"), cases = [];
  for (const [prefix, fields] of [["", contract.fields], ["selection.", contract.selectionFields]]) {
    for (const [name, field] of Object.entries(fields)) {
      const path = prefix + name;
      for (const [index, value] of [[], { pwaStorage: null }, null].entries()) {
        if (field.shape.endsWith("object") && index === 1 || field.shape.startsWith("nullable:") && value === null || field.shape.startsWith("array:") && index === 0) continue;
        cases.push(rejected(`${path}-${index}`, replace(selectionRequest(), path, value)));
      }
      if (field.required && path !== "lifecycle") cases.push(rejected(`${path}-omitted`, replace(selectionRequest(), path, null, true)));
    }
  }
  for (const [index, value] of [{ selectPhase: { phase: "focus" } }, ["selectPhase"], "selectPhase", {}, { kind: { selectPhase: null } }, { kind: "selectPhase" }, { kind: "skip", explicit: true }].entries()) {
    cases.push(rejected(`action-encoding-${index}`, replace(selectionRequest(), "intent", value)));
  }
  for (const phase of [null, [], { focus: null }, "invalid"]) cases.push(rejected(`phase-${JSON.stringify(phase)}`, replace(selectionRequest(), "intent.phase", phase)));
  for (const generation of ["-1", "01", "+1", "", "9223372036854775807", "9223372036854775808"]) {
    cases.push(rejected(`generation-${generation}`, replace(selectionRequest(), "selection.generation", generation)));
  }
  for (const status of ["idle", "invalid"]) cases.push(rejected(`timer-${status}`, replace(selectionRequest(), "workspace.base.canonicalTimer.status", status)));
  for (const flag of ["explicitChoice", "advanceGeneration", "sourceAccepted"]) cases.push(rejected(flag, { ...selectionRequest(), [flag]: true }));
  for (const state of [null, [], { consumedCompletions: null }, { consumedCompletions: [["timer", null, "focus"]] }, { pendingBreaks: null }, { pendingBreaks: [["finish", "timer", 1]] }, { automatic: true }]) {
    cases.push(rejected(`lifecycle-${JSON.stringify(state)}`, replace(selectionRequest(), "lifecycle", state)));
  }
  for (const [field, value] of [["phase", "long_break"], ["plannedDurationMs", 60000]]) {
    const input = naturalInstall(selectionRequest("short_break"));
    input.selection.explicit = true;
    input.sentContext.commands = [{ id: "late-finish", timerId: input.canonicalTimer.id,
      type: "finish", phase: "focus", deviceSequence: 9, occurredAt: input.referenceTime }];
    input.acknowledgements = [{ commandId: "late-finish", outcome: "rejected" }];
    input.afterHistory[0][field] = value;
    cases.push({ ...vector("timer.completionState.v1", `choice-invalid-ack-pair-${field}`, input, false,
      "conflicting workspace terminal"), rejectionHit: "pwaChoiceShape" });
  }
  return cases;
}
