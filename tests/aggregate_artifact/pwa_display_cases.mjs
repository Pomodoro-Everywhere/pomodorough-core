import { changed, fixture, vector } from "./cases.mjs";
import { branch } from "./semantics.mjs";

export const domains = ["commands", "taskOperations", "durationOperations", "autoStartOperations", "selectedTaskOperations"];
const stores = ["pending", "pendingTasks", "pendingDurations", "pendingAutoStarts", "pendingSelectedTasks"];
export const fields = ["canonicalTimer", "history", "tasks", "durationsMs", "autoStartBreaks", "selectedTaskId"];
export const emptyQueues = () => Object.fromEntries(domains.map((name) => [name, []]));
export const context = (projectionPending) => ({ profile: "pwaStorage", projectionPending });
export const meta = (records, key) => records.meta.find((record) => record.key === key)?.value;

export function rawWorkspace(index = 0) {
  const records = fixture("pwa-display-context-v1").observations[index].persisted;
  const snapshot = meta(records, "snapshot");
  return { base: Object.fromEntries(fields.map((name) => [name, snapshot[name]])),
    local: Object.fromEntries(domains.map((name, i) => [name, records[stores[i]]])),
    neverSent: meta(records, "deliveryProof"), canonicalHead: meta(records, "canonicalHead") ?? null,
    timerDependencies: meta(records, "timerDependencies"), displayContext: context(meta(records, "projectionPending")) };
}

export function readRequest(workspace, at = "2026-08-31T12:00:01Z") {
  return { profile: "pwaStorage", source: { kind: "workspace", value: workspace }, selectedPhase: "focus",
    observedAt: at, calendarIntervals: [{ start: "2026-08-31T00:00:00Z", end: "2026-09-01T00:00:00Z" }] };
}

export function uuid(at, index = 1) {
  const hex = Date.parse(at).toString(16).padStart(12, "0");
  return `${hex.slice(0, 8)}-${hex.slice(8)}-7000-8000-${String(index).padStart(12, "0")}`;
}

export function intentRequest(workspace = rawWorkspace(), at = "2026-08-31T12:00:01Z", kind = "pause") {
  const records = fixture("pwa-display-context-v1").observations[0].persisted;
  return { ...fixture("workspace-intent-v1").request, compatibility: "pwaStorage", workspace,
    intent: { kind }, requestedTimer: null, selection: { phase: "focus", generation: "0", explicit: false },
    allocation: { deviceId: meta(records, "deviceId"), deviceSequence: meta(records, "deviceSequence"),
      hlc: meta(records, "hlc"), lastUuid: meta(records, "uuidV7") }, observation: meta(records, "workspaceObservation"),
    clock: { occurredAt: at, physicalNow: at, observedAt: at },
    identities: { commandUuids: [uuid(at)], timerUuid: "12345678-1234-4234-8234-123456789012" },
    calendarIntervals: readRequest(workspace).calendarIntervals };
}

export function finishRequest(workspace = rawWorkspace(), at = "2026-08-31T12:00:03Z") {
  const input = intentRequest(workspace, at);
  delete input.intent;
  return { ...input, stage: "finishCommit", ownership: null };
}

export function displayCases() {
  const cases = [];
  for (const index of [0, 1]) {
    const workspace = rawWorkspace(index);
    cases.push(branch("workspace.project.v1", `pwa-raw-project-${index}`, { ...workspace, now: "2026-08-31T12:00:01Z" }, "pwaDisplay", {
      equals: { "workspace.canonicalTimer.status": "running", "projectionPending.commands": [],
        "displayContext.projectionPending.commands": workspace.local.commands } }));
    cases.push(branch("workspace.readModel.v1", `pwa-raw-read-${index}`, readRequest(workspace), "pwaDisplay", {
      equals: { "canonical.status": "running", "canonical.timerId": workspace.local.commands[0].timerId,
        availableIntents: ["pause", "finish", "cancel", "cancelAndClear"] } }));
  }
  cases.push(...invalidDisplayCases());
  return cases;
}

function corruptContexts() {
  const stored = rawWorkspace().displayContext.projectionPending;
  return [
    ["null-wrapper", null], ["boolean", true], ["wrong-profile", { ...context(stored), profile: "appleWorkspace" }],
    ["eligibility", { ...context(stored), eligible: true }], ["missing-records", { profile: "pwaStorage" }],
    ["incomplete", context({ commands: [] })], ["unknown-domain", context({ ...stored, sendingAllowed: true })],
    ["malformed-array", context({ ...stored, commands: "corrupt" })],
    ["unknown-id", context(changed(stored, "commands.0.id", "foreign"))],
    ["rewritten-clock", context(changed(stored, "commands.0.hlcCounter", 4))],
    ["added-extension", context(changed(stored, "commands.0.extension", { allowed: true }))],
    ["duplicate-id", context({ ...stored, commands: [...stored.commands, ...stored.commands] })],
  ];
}

function invalidDisplayCases() {
  const cases = [];
  for (const [name, displayContext] of corruptContexts()) {
    const workspace = { ...rawWorkspace(), displayContext };
    for (const [operation, request] of [
      ["workspace.project.v1", { ...workspace, now: "2026-08-31T12:00:01Z" }],
      ["workspace.readModel.v1", readRequest(workspace)], ["workspace.intent.v1", intentRequest(workspace)],
      ["workspace.completionMutation.v1", { ...finishRequest(workspace), requestedTimer: fixture("workspace-intent-v1").timer }],
    ]) cases.push(vector(operation, `pwa-display-${name}`, request, false));
  }
  for (const profile of ["appleWorkspace", "androidCoordinator", "desktopStorage", "desktopTerminal"]) {
    cases.push(vector("workspace.intent.v1", `pwa-context-${profile}`, { ...intentRequest(), compatibility: profile }, false, "PWA profile"));
    cases.push(vector("workspace.readModel.v1", `pwa-context-${profile}`, { ...readRequest(rawWorkspace()), profile }, false, "PWA profile"));
  }
  const duplicate = JSON.stringify({ ...rawWorkspace(), now: "2026-08-31T12:00:01Z" })
    .replace('"profile":"pwaStorage"', '"profile":"pwaStorage","profile":"pwaStorage"');
  cases.push(vector("workspace.project.v1", "pwa-duplicate-context-key", duplicate, false, "duplicate field"));
  const bootstrap = fixture("bootstrap-workspace-v1").request;
  bootstrap.profile = "pwaStorage";
  bootstrap.local.workspace = { ...rawWorkspace(), now: "2026-08-31T12:00:01Z" };
  delete bootstrap.local.workspace.displayContext;
  bootstrap.local.workspace.local.commands[0].id = null;
  bootstrap.local.projectionPending = emptyQueues();
  cases.push(vector("bootstrap.workspacePlan.v1", "pwa-hidden-malformed-command-id", bootstrap, false));
  return cases;
}
