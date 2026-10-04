import { fixture, vector } from "./cases.mjs";
import { branch } from "./semantics.mjs";

const operation = "workspace.legacyPreferences.v1";
const request = () => structuredClone(fixture("legacy-preferences-v1").request);

export function legacyNumericCases() {
  const cases = [];
  for (const [name, number, duration] of [["below", 90.49999999999999, 5400000],
    ["exact", 90.5, 5460000], ["above", 90.50000000000001, 5460000]]) {
    for (const representation of ["number", "string"]) {
      const input = request(), value = representation === "string" ? String(number) : number;
      input.settings = { durations: { focus: value, short_break: value, long_break: value }, peerOnlySetting: { number } };
      cases.push(branch(operation, `legacy-json-${name}-${representation}`, input, "legacyNumeric", {
        lengths: { "operations.durationOperations": 3 }, equals: {
          "operations.durationOperations.0.durationMs": duration, "operations.durationOperations.1.durationMs": duration,
          "operations.durationOperations.2.durationMs": duration, "settings.peerOnlySetting.number": number },
        same: { "workspace.base": "workspace.base" } }));
    }
  }
  for (const complete of [false, true]) {
    const input = request();
    input.settings = { peerOnlySetting: { number: 90.49999999999999, array: [90.50000000000001, null] } };
    if (complete) Object.assign(input.settings, { durationSyncBootstrapped: true, autoStartSyncBootstrapped: true,
      selectedTaskSyncBootstrapped: true });
    input.identities.operationUuids = [];
    cases.push(branch(operation, `legacy-json-markers-${complete}`, input, "legacyNumeric", {
      equals: { consumedIdentityCount: 0, writeSettings: !complete },
      same: { "settings.peerOnlySetting": "settings.peerOnlySetting", workspace: "workspace" } }));
  }
  for (const complete of [false, true]) {
    const input = request();
    const retained = { id: "numeric-claimed", phase: "focus", durationMs: 1800000,
      occurredAt: "1970-01-01T00:00:01Z", hlcWallMs: 0, hlcCounter: 0,
      extension: { number: 90.49999999999999, array: [90.50000000000001, null] } };
    input.workspace.local.durationOperations = [retained];
    input.workspace.displayContext.projectionPending = structuredClone(input.workspace.local);
    input.outgoing = { ownerId: input.ownership.ownerId, sent: { durationOperations: [retained] },
      body: ' { "durationOperations": [' + JSON.stringify(retained) + '] } ', extension: { number: 90.49999999999999 } };
    if (complete) {
      input.settings = { durationSyncBootstrapped: true, autoStartSyncBootstrapped: true, selectedTaskSyncBootstrapped: true };
      input.identities.operationUuids = [];
    }
    cases.push(branch(operation, `legacy-json-claimed-${complete}`, input, "legacyNumeric", {
      same: { outgoing: "outgoing" }, prefixes: { "workspace.local.durationOperations": "workspace.local.durationOperations" } }));
  }
  return cases.concat(legacyNumericRejections());
}

function legacyNumericRejections() {
  const cases = ["1.0", "1e0", "1.00000000000000001", "9007199254740992", "18446744073709551616", "1e400"].map((token) => {
    const input = JSON.stringify(request()).replace('"counter":0', `"counter":${token}`);
    return vector(operation, `legacy-json-reject-${token}`, input, false);
  });
  const input = JSON.stringify(request());
  for (const raw of [input.replace('"peerOnlySetting":{', '"peerOnlySetting":{"number":90.49999999999999,"number":90.5,'),
    input.replace('"peerOnlySetting":{', '"peerOnlySetting":{"nested":[{"number":90.49999999999999,"number":90.5}],'),
    input + " trailing"]) cases.push(vector(operation, `legacy-json-invalid-${cases.length}`, raw, false));
  return cases.map((item) => ({ ...item, rejectionHit: "legacyNumeric" }));
}
