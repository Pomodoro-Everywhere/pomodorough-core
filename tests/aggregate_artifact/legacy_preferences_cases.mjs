import assert from "node:assert/strict";
import { fixture, vector } from "./cases.mjs";
import { branch } from "./semantics.mjs";

export const legacyOperation = "workspace.legacyPreferences.v1";
const domains = ["commands", "taskOperations", "durationOperations", "autoStartOperations", "selectedTaskOperations"];
const epoch = "1970-01-01T00:00:00.000Z";
const request = () => structuredClone(fixture("legacy-preferences-v1").request);
const checks = { same: { "workspace.base": "workspace.base", outgoing: "outgoing" },
  equals: { "projection.durationsMs.focus": 2100000, "outgoingAction": "preserve",
    "operations.durationOperations.0.hlcWallMs": 0, "operations.durationOperations.0.hlcCounter": 0,
    "operations.durationOperations.0.occurredAt": epoch, "operations.durationOperations.0.ownerId": "bootstrap",
    "operations.autoStartOperations.0.enabled": false, "operations.selectedTaskOperations.0.taskId": null } };

export function legacyCases() {
  const cases = [branch(legacyOperation, "legacy-remote-35", request(), "legacyPreferences", {
    ...checks, lengths: { "operations.durationOperations": 3 },
    equals: { ...checks.equals, consumedIdentityCount: 5, "operations.durationOperations.1.durationMs": 480000,
      "operations.durationOperations.2.durationMs": 1200000,
      "workspace.displayContext.projectionPending.durationOperations": [] } })];
  for (const [name, minutes, expected] of [["default", 25, null], ["half", 24.5, null], ["invalid", "bad", 60000],
    ["low", -4, 60000], ["high", 181, 10800000], ["null", null, null], ["array", [30.5], 1860000],
    ["hex", "0x1e", 1800000], ["nonfinite", "Infinity", 60000], ["object", {}, 60000]]) {
    const input = request();
    input.settings = { durations: { focus: minutes }, peerOnlySetting: "keep" };
    cases.push(branch(legacyOperation, `legacy-number-${name}`, input, "legacyPreferences", {
      same: checks.same, lengths: { "operations.durationOperations": expected === null ? 0 : 1 },
      equals: { "settings.peerOnlySetting": "keep", "settings.durationSyncBootstrapped": true,
        ...(expected === null ? {} : { "operations.durationOperations.0.durationMs": expected }) } }));
  }
  const saved = request();
  const old = { id: "old-duration", phase: "focus", durationMs: 1500000,
    occurredAt: "1970-01-01T00:00:01Z", hlcWallMs: 0, hlcCounter: 0, extension: { empty: "", omitted: null } };
  saved.workspace.local.durationOperations = [old];
  saved.workspace.displayContext.projectionPending = Object.fromEntries(domains.map((domain) => [domain, []]));
  saved.outgoing = { ownerId: saved.ownership.ownerId, sent: { durationOperations: [old] },
    body: ' { "durationOperations": [ ' + JSON.stringify(old) + ' ] } ', extension: [null, false] };
  cases.push(branch(legacyOperation, "legacy-saved-exact", saved, "legacyPreferences", {
    ...checks, prefixes: { "workspace.local.durationOperations": "workspace.local.durationOperations" } }));
  const missing = structuredClone(saved);
  delete missing.outgoing.body;
  cases.push(branch(legacyOperation, "legacy-saved-missing-body", missing, "legacyPreferences", {
    ...checks, prefixes: { "workspace.local.durationOperations": "workspace.local.durationOperations" } }));
  const implicit = request();
  implicit.settings = { autoStartBreaks: false, autoStartBreaksExplicit: false, selectedTaskId: null };
  implicit.identities.operationUuids = [];
  cases.push(branch(legacyOperation, "legacy-implicit-flags", implicit, "legacyPreferences", {
    equals: { consumedIdentityCount: 0, "settings.autoStartSyncBootstrapped": true,
      "settings.selectedTaskSyncBootstrapped": true, effectsAfterCommit: [] } }));
  cases.push(...legacyRejections(saved));
  return cases;
}

function legacyRejections(saved) {
  const mutations = [
    ["tuple-root", (x) => Object.values(x)],
    ["tuple-owner", (x) => { x.ownership = Object.values(x.ownership); }],
    ["tuple-identities", (x) => { x.identities = Object.values(x.identities); }],
    ["tuple-head", (x) => { x.workspace.canonicalHead = Object.values(x.workspace.canonicalHead); }],
    ["object-profile", (x) => { x.profile = { pwaStorage: null }; }],
    ["object-display-profile", (x) => { x.workspace.displayContext.profile = { pwaStorage: null }; }],
    ["wrong-owner", (x) => { x.ownership.expectedOwnerId = "foreign"; }],
    ["wrong-outgoing-owner", (x) => { x.outgoing = { ownerId: "foreign" }; }],
    ["missing-proof", (x) => { delete x.workspace.neverSent; }],
    ["unknown-proof", (x) => { x.workspace.neverSent.fake = []; }],
    ["proof-claim", (x) => { Object.assign(x, structuredClone(saved)); x.workspace.neverSent.durationOperations = ["old-duration"]; }],
    ["body-claim", (x) => { Object.assign(x, structuredClone(saved)); delete x.outgoing.sent;
      x.workspace.neverSent.durationOperations = ["old-duration"]; }],
    ["ids-claim", (x) => { Object.assign(x, structuredClone(saved)); x.outgoing = {
      ownerId: x.ownership.ownerId, queueIds: { durationOperations: ["old-duration"] } };
      x.workspace.neverSent.durationOperations = ["old-duration"]; }],
    ["bad-body", (x) => { x.outgoing = { ownerId: x.ownership.ownerId, body: "{" }; }],
    ["missing-identity", (x) => { x.identities.operationUuids = []; }],
    ["duplicate-identity", (x) => { x.identities.operationUuids[1] = x.identities.operationUuids[0]; }],
    ["collision", (x) => { Object.assign(x, structuredClone(saved)); x.workspace.local.durationOperations[0].id = x.identities.operationUuids[0];
      x.outgoing = null; }],
    ["bad-selection", (x) => { x.settings.selectedTaskId = ""; }],
    ["object-conversion", (x) => { x.settings.durations.focus = { toString: "shadow" }; }],
    ["stored-mismatch", (x) => { Object.assign(x, structuredClone(saved)); x.workspace.displayContext.projectionPending = structuredClone(x.workspace.local);
      x.workspace.displayContext.projectionPending.durationOperations[0].deviceId = x.deviceId; }],
    ["no-timestamp-repair", (x) => { Object.assign(x, structuredClone(saved)); delete x.workspace.local.durationOperations[0].occurredAt; }],
  ];
  return mutations.map(([name, mutate]) => {
    const input = request(); const replaced = mutate(input);
    return { ...vector(legacyOperation, `legacy-reject-${name}`, replaced ?? input, false), rejectionHit: "legacyPreferences" };
  });
}

export function legacyScenarios(call) {
  const input = request();
  const first = call(branch(legacyOperation, "legacy-restart-first", input, "legacyRestart", checks));
  input.workspace = first.workspace;
  input.settings = first.settings;
  input.identities.operationUuids = [];
  const restarted = call(branch(legacyOperation, "legacy-restart-noop", input, "legacyRestart", {
    same: { workspace: "workspace", settings: "settings", outgoing: "outgoing" },
    equals: { outcome: "noop", writeSettings: false, consumedIdentityCount: 0, effectsAfterCommit: [] } }));
  assert.deepEqual(restarted.workspace, input.workspace);
  assert.equal(Object.hasOwn(first.operations.autoStartOperations[0], "deviceId"), false);
  for (const domain of domains) assert.deepEqual(restarted.operations[domain], []);
}
