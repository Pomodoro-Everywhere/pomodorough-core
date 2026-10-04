import assert from "node:assert/strict";
import { fixture, vector } from "./cases.mjs";
import { branch } from "./semantics.mjs";
import { terminalRequest } from "./terminal_cases.mjs";

export const dependencyOperation = "workspace.legacyDependencyPlan.v1";
const receipts = () => fixture("legacy-dependencies-v1").receipts;
const request = (name = "complete") => structuredClone(receipts().find((row) => row.name === name).input);
const command = (input, id = "legacy-break-start") => input.workspace.local.commands.find((row) => row.id === id);
const preserve = { "workspace.local": "workspace.local", "workspace.base": "workspace.base",
  "workspace.neverSent": "workspace.neverSent", "workspace.canonicalHead": "workspace.canonicalHead",
  "workspace.displayContext": "workspace.displayContext", outgoing: "outgoing", ownership: "ownership" };

export function dependencyCases() {
  const cases = receipts().map((row) => ({ ...branch(dependencyOperation, `dependency-raw-${row.name}`,
    row.input, "legacyDependencies", { equals: row.completeNativeReturn }), input: row.inputRaw }));
  for (const [name, mutate, reason] of [
    ["calendar-missing", (x) => { x.calendarIntervals = []; }, "missingCalendarEvidence"],
    ["saved-body-missing", (x) => { delete x.outgoing.body; }, "savedRequestBodyMissing"],
    ["ack-missing", (x) => { x.sourceAcknowledgements = []; }, "sourceAcknowledgementRequired"],
    ["payload-decision", (x) => { x.workspace.base.durationsMs.short_break = 600000; }, "canonicalPayloadDecisionRequired"],
  ]) {
    const input = request(name === "calendar-missing" ? "complete" : "canonical-source-applied"); mutate(input);
    cases.push(branch(dependencyOperation, `dependency-blocked-${name}`, input, "legacyDependencies", {
      same: { ...preserve, workspace: "workspace" }, equals: { outcome: "blocked", timerDependencies: null,
        metadataWrites: [], "recovery.blocksSync": true, "recovery.blocksMutations": true,
        "recovery.unresolved.0.reason": reason } }));
  }
  const direct = request("sibling-proven"); command(direct, "legacy-break-pause").dependsOnCommandId = "legacy-break-start";
  cases.push(branch(dependencyOperation, "dependency-direct-future", direct, "legacyDependencies", {
    same: preserve, equals: { "classifications.0.dependency.dependsOnOperationId": "legacy-break-start", wireAction: "preserve" } }));
  const explicit = request("missing-parent"); explicit.workspace.timerDependencies = [];
  cases.push(branch(dependencyOperation, "dependency-local-empty-authoritative", explicit, "legacyDependencies", {
    same: { workspace: "workspace" }, equals: { outcome: "noop", metadataWrites: [], "recovery.status": "ready" } }));
  const native = request("sibling-proven"); native.profile = "androidCentralized";
  for (const row of native.workspace.local.commands) {
    if (row.dependsOnCommandId) row.generatedByFinishCommandId = row.dependsOnCommandId;
    delete row.dependsOnCommandId; delete row.generatedBreak; delete row.deviceId; row.neverSent = true;
  }
  cases.push(branch(dependencyOperation, "dependency-native-raw-entity", native, "legacyDependencies", {
    same: preserve, equals: { "classifications.0.dependency.dependsOnOperationId": "legacy-break-start", outcome: "planned" } }));
  cases.push(...additionalEvidenceCases());
  return [...cases, ...dependencyRejections(), ...dependencyCheckerCases(), ...dependencyResidualCases()];
}

function dependencyResidualCases() {
  return fixture("legacy-dependency-residual-v1").cases.map((item) => ({
    ...branch(dependencyOperation, `dependency-residual-${item.name}`, JSON.parse(item.inputRaw), "legacyDependencyResidual", {
      same: { workspace: "workspace", outgoing: "outgoing", ownership: "ownership" },
      equals: { outcome: "blocked", metadataWrites: [], timerDependencies: null,
        "recovery.blocksSync": true, "recovery.blocksMutations": true, "recovery.automaticRepair": false,
        "recovery.unresolved.0.reason": item.rebaseInput ? "possiblyDeliveredPayloadDecisionRequired" : "savedRequestBodyIncomplete" } }),
    input: item.inputRaw }));
}

function dependencyCheckerCases() {
  return fixture("legacy-dependency-checker-v1").cases.map((item) => {
    const name = `dependency-checker-${item.name}`;
    if (item.expected === "denied") return { ...vector(dependencyOperation, name, item.inputRaw, false),
      rejectionHit: "legacyDependencyChecker" };
    return { ...branch(dependencyOperation, name, JSON.parse(item.inputRaw), "legacyDependencyChecker", {
      same: { workspace: "workspace", outgoing: "outgoing", ownership: "ownership" },
      equals: { outcome: "blocked", metadataWrites: [], timerDependencies: null,
        "recovery.blocksSync": true, "recovery.blocksMutations": true, "recovery.automaticRepair": false } }), input: item.inputRaw };
  });
}

function additionalEvidenceCases() {
  const physical = request();
  physical.workspace.local.commands.forEach((row) => { row.physicalOccurredAt = "2026-08-30T14:00:00Z"; });
  physical.calendarIntervals = [{ start: "2026-08-30T00:00:00Z", end: "2026-08-31T00:00:00Z" }];
  const causal = request();
  causal.workspace.base.history = [0, 1, 2].map((index) => ({ id: `later-${index}`, timerId: `later-${index}`,
    commandId: `later-finish-${index}`, phase: "focus", status: "completed", plannedDurationMs: 1500000,
    completedAt: "2026-08-31T13:00:00Z", endedAt: "2026-08-31T13:00:00Z" }));
  const canonical = request("canonical-source-applied"), source = canonical.workspace.base.history[0];
  canonical.workspace.base.history = [];
  canonical.workspace.base.canonicalTimer = { id: source.timerId, phase: "focus", status: "completed",
    plannedDurationMs: source.plannedDurationMs, elapsedAtAnchorMs: source.plannedDurationMs, anchorAt: source.completedAt,
    startedByDeviceId: canonical.deviceId, lastIntent: { type: "finish", commandId: "legacy-finish", occurredAt: source.completedAt } };
  const frozen = request(); command(frozen).phase = "long_break"; frozen.workspace.neverSent.commands = ["legacy-finish"];
  const sibling = request("canonical-source-applied");
  sibling.workspace.local.commands.push({ ...command(sibling), id: "legacy-break-pause", type: "pause", generatedBreak: false,
    deviceSequence: command(sibling).deviceSequence + 1, hlcCounter: command(sibling).hlcCounter + 1 });
  sibling.workspace.neverSent.commands.push("legacy-break-pause");
  return [
    branch(dependencyOperation, "dependency-physical-source", physical, "legacyDependencies", { same: preserve,
      equals: { "classifications.0.sourceCompletedAt": "2026-08-30T14:00:00Z", "timerDependencies.0.sourceDayStart": "2026-08-30T00:00:00Z" } }),
    branch(dependencyOperation, "dependency-causal-count", causal, "legacyDependencies", { same: preserve,
      equals: { "classifications.0.sourcePhaseAfter": "short_break" } }),
    branch(dependencyOperation, "dependency-canonical-only", canonical, "legacyDependencies", { same: preserve,
      equals: { outcome: "planned", timerDependencies: [] } }),
    branch(dependencyOperation, "dependency-frozen-payload", frozen, "legacyDependencies", { same: { workspace: "workspace" },
      equals: { outcome: "blocked", "recovery.unresolved.0.reason": "possiblyDeliveredPayloadDecisionRequired", metadataWrites: [] } }),
    branch(dependencyOperation, "dependency-sibling-saved-ack", sibling, "legacyDependencies", { same: preserve,
      equals: { timerDependencies: [{ operationId: "legacy-break-pause", dependsOnOperationId: "legacy-break-start" }] } }),
  ];
}

function set(input, path, value) {
  const keys = path.split(".");
  keys.slice(0, -1).reduce((object, key) => object[key], input)[keys.at(-1)] = value;
}

function representations(value, path = "", result = []) {
  if (value && typeof value === "object") {
    if (!Array.isArray(value)) result.push([path, Object.values(value)]);
    for (const [key, child] of Object.entries(value)) {
      if (["extension", "metadata", "originalReceiptProvenance"].includes(key)) continue;
      representations(child, path ? `${path}.${key}` : key, result);
    }
  }
  return result;
}

export function dependencyRejections() {
  const cases = [];
  for (const name of ["all-five-raw", "canonical-source-applied", "sibling-proven"]) {
    const original = request(name);
    for (const [path, array] of representations(original)) {
      const input = structuredClone(original);
      if (path) set(input, path, array);
      cases.push({ ...vector(dependencyOperation, `dependency-shape-${name}-${path || "root"}`, path ? input : array, false), rejectionHit: "legacyDependencies" });
    }
  }
  for (const path of ["profile", "workspace.local.commands.0.type", "workspace.local.commands.0.phase",
    "workspace.displayContext.profile", "sourceAcknowledgements.0.outcome"]) {
    const input = request("canonical-source-applied");
    set(input, path, { [path.endsWith("outcome") ? "applied" : "start"]: null });
    cases.push({ ...vector(dependencyOperation, `dependency-enum-${path}`, input, false), rejectionHit: "legacyDependencies" });
  }
  for (const [name, mutate] of [
    ["unknown-control", (x) => { x.action = { kind: "repair", requestId: "phantom" }; }],
    ["unknown-proof", (x) => { x.workspace.neverSent.fake = []; }],
    ["duplicate-proof", (x) => { x.workspace.neverSent.commands.push(x.workspace.neverSent.commands[0]); }],
    ["unsafe-counter", (x) => { command(x).hlcCounter = 9007199254740992; }],
    ["fraction-counter", (x) => { command(x).hlcCounter = 1.5; }],
    ["parent-self", (x) => { command(x).dependsOnCommandId = command(x).id; }],
    ["generated-no-id", (x) => { delete command(x).dependsOnCommandId; }],
    ["bad-physical", (x) => { command(x).physicalOccurredAt = "invalid"; }],
    ["bad-day", (x) => { command(x).sourceDayStart = "invalid"; command(x).sourceDayEnd = "invalid"; }],
    ["partial-day", (x) => { command(x).sourceDayStart = "2026-08-31T00:00:00Z"; }],
    ["excluding-day", (x) => { command(x).sourceDayStart = "2026-09-01T00:00:00Z"; command(x).sourceDayEnd = "2026-09-02T00:00:00Z"; }],
    ["overlap-calendar", (x) => { x.calendarIntervals.push(x.calendarIntervals[0]); }],
    ["source-after-child", (x) => { command(x).physicalOccurredAt = "2026-08-30T00:00:00Z"; }],
    ["wrong-owner", (x) => { x.ownership.expectedOwnerId = "other"; }],
    ["missing-parent-field", (x) => { x.workspace.timerDependencies = [{ operationId: command(x).id }]; }],
    ["unknown-child", (x) => { x.workspace.timerDependencies = [{ operationId: "phantom", dependsOnOperationId: "legacy-finish" }]; }],
    ["duplicate-edges", (x) => { x.workspace.timerDependencies = [1, 2].map(() => ({ operationId: command(x).id, dependsOnOperationId: "legacy-finish" })); }],
    ["reverse-edge", (x) => { x.workspace.timerDependencies = [{ operationId: "legacy-finish", dependsOnOperationId: "legacy-break-start" }]; }],
  ]) {
    const input = request(); mutate(input);
    cases.push({ ...vector(dependencyOperation, `dependency-reject-${name}`, input, false), rejectionHit: "legacyDependencies" });
  }
  for (const [name, mutate] of [
    ["wrong-outgoing-owner", (x) => { x.outgoing.ownerId = "other"; }],
    ["proof-claim", (x) => { x.workspace.neverSent.commands.push("legacy-finish"); }],
    ["ack-phantom", (x) => { x.sourceAcknowledgements[0].commandId = "phantom"; }],
    ["ack-duplicate", (x) => { x.sourceAcknowledgements.push(x.sourceAcknowledgements[0]); }],
    ["body-malformed", (x) => { x.outgoing.body = "{"; }],
    ["body-duplicate", (x) => { x.outgoing.body = '{"commands":[],"commands":[]}'; }],
    ["body-enum-object", (x) => { const body = JSON.parse(x.outgoing.body); body.commands[0].type = { finish: null }; x.outgoing.body = JSON.stringify(body); }],
    ["body-record-array", (x) => { const body = JSON.parse(x.outgoing.body); body.commands[0] = Object.values(body.commands[0]); x.outgoing.body = JSON.stringify(body); }],
    ["body-wrong-device", (x) => { const body = JSON.parse(x.outgoing.body); body.deviceId = "foreign"; x.outgoing.body = JSON.stringify(body); }],
  ]) {
    const input = request("canonical-source-applied"); mutate(input);
    cases.push({ ...vector(dependencyOperation, `dependency-reject-${name}`, input, false), rejectionHit: "legacyDependencies" });
  }
  const duplicate = JSON.stringify(request()).replace('"profile":"pwaStorage"', '"profile":"pwaStorage","profile":"pwaStorage"');
  cases.push({ ...vector(dependencyOperation, "dependency-duplicate-root", duplicate, false), rejectionHit: "legacyDependencies" });
  return cases;
}

export function dependencyScenarios(call) {
  for (const name of ["complete", "missing-parent", "sibling-proven", "canonical-source-applied"]) {
    const input = request(name);
    const first = call(branch(dependencyOperation, `dependency-restart-${name}-first`, input, "legacyDependencyRestart", {
      same: preserve, equals: { "outgoingAction": "preserve", "wireAction": "preserve" } }));
    input.workspace = first.workspace;
    const next = call(branch(dependencyOperation, `dependency-restart-${name}-second`, input, "legacyDependencyRestart", {
      same: { workspace: "workspace", outgoing: "outgoing" }, equals: {
        outcome: name === "missing-parent" ? "blocked" : "noop", metadataWrites: [] } }));
    assert.deepEqual(next.workspace, first.workspace);
  }
  acknowledgementScenarios(call);
  residualCompositionScenarios(call);
}

function residualCompositionScenarios(call) {
  for (const item of fixture("legacy-dependency-residual-v1").cases.filter((item) => item.rebaseInput)) {
    const input = JSON.parse(item.inputRaw);
    const blocked = call(branch(dependencyOperation, `dependency-residual-flow-${item.name}-blocked`, input, "legacyDependencyResidualFlow", {
      same: { workspace: "workspace", outgoing: "outgoing" }, equals: { outcome: "blocked", metadataWrites: [], timerDependencies: null } }));
    call(branch(dependencyOperation, `dependency-residual-flow-${item.name}-reopen`, { ...input, workspace: blocked.workspace }, "legacyDependencyResidualFlow", {
      equals: blocked }));
    call(vector("reconcile.rebase.v3", `dependency-residual-flow-${item.name}-wire-denied`, item.rebaseInput, false, "possibly delivered operation"));
    input.workspace.neverSent.commands.push("legacy-break-pause");
    const allowed = call(branch(dependencyOperation, `dependency-residual-flow-${item.name}-proven`, input, "legacyDependencyResidualFlow", {
      same: preserve, equals: { outcome: "planned", wireAction: "preserve" } }));
    const rebase = structuredClone(item.rebaseInput);
    rebase.timerDependencies = allowed.timerDependencies;
    rebase.neverSent.commands.push("legacy-break-pause");
    const expected = input.workspace.local.commands.filter((row) => row.id !== "legacy-finish").map((row) => ({ ...row,
      phase: "short_break", plannedDurationMs: 300000, observedElapsedMs: Math.min(300000, Math.max(0, row.observedElapsedMs)) }));
    call(branch("reconcile.rebase.v3", `dependency-residual-flow-${item.name}-normalized`, rebase, "legacyDependencyResidualFlow", {
      equals: { ...Object.fromEntries(expected.flatMap((row, index) => ["id", "phase", "plannedDurationMs", "observedElapsedMs", "dependsOnCommandId"]
        .map((field) => [`pending.${index}.${field}`, row[field]]))), promotedTimerOperationIds: ["legacy-break-start"] },
      lengths: { pending: expected.length }, same: { canonicalResponse: "response" } }));
  }
}

function acknowledgementScenarios(call) {
  const input = request("sibling-proven");
  const migrated = call(vector(dependencyOperation, "dependency-ack-migration", input));
  const source = command(input, "legacy-finish"), start = command(input), pause = command(input, "legacy-break-pause");
  const local = input.workspace.local, sent = { ...Object.fromEntries(Object.keys(local).map((domain) => [domain, []])), commands: [source] };
  const projected = call(vector("workspace.project.v1", "dependency-ack-source-projection", {
    ...input.workspace, local: sent, timerDependencies: [], neverSent: {}, displayContext: { profile: "pwaStorage", projectionPending: null } })).workspace;
  const response = { ...terminalRequest().response, ...Object.fromEntries(["canonicalTimer", "history", "tasks", "durationsMs", "autoStartBreaks", "selectedTaskId"]
    .map((field) => [field, projected[field]])), serverTime: source.occurredAt,
    serverHlcWallMs: source.hlcWallMs, serverHlcCounter: source.hlcCounter };
  for (const outcome of ["applied", "ignored", "rejected"]) {
    const rebase = { local, sent, response: { ...response, acknowledgements: [{ commandId: source.id, outcome, reason: "" }] },
      timerDependencies: migrated.timerDependencies, neverSent: { commands: [start.id, pause.id] } };
    const accepted = outcome !== "rejected";
    call(branch("reconcile.rebase.v3", `dependency-rebase-${outcome}`, rebase, "legacyDependencyAcknowledgement", {
      equals: { pendingTimerDependencies: accepted ? [{ operationId: pause.id, dependsOnOperationId: start.id }] : [],
        promotedTimerOperationIds: accepted ? [start.id] : [],
        droppedTimerOperationIds: accepted ? [] : [pause.id, start.id].sort() },
      lengths: { pending: accepted ? 2 : 0 } }));
    if (!accepted) call(vector("reconcile.rebase.v3", "dependency-rebase-frozen-drop-denied", { ...rebase, neverSent: {} }, false, "possibly delivered dependent"));
  }
}
