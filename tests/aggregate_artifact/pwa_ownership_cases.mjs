import { changed, fixture, vector } from "./cases.mjs";
import { branch } from "./semantics.mjs";
import { shapeLegacyControls, shapeRejectionCases } from "./pwa_ownership_shapes.mjs";

const operation = "workspace.ownershipPlan.v1";
const request = () => fixture("pwa-ownership-plan-v1").request;
const owner = () => fixture("pwa-ownership-plan-v1").owner;

function checked(name, input, equals) {
  return branch(operation, `pwa-owner-${name}`, input, "pwaOwnership", {
    equals: { ...equals, effectsAfterCommit: [] }, same: { workspace: "workspace" },
  });
}

export function ownershipCases() {
  const cases = [];
  const write = { ...owner(), kind: "recordTimerOwner" };
  cases.push(checked("missing-retained", request(), { ownership: owner(), renewed: true, ownershipWrites: [write, write] }));
  const install = changed(request(), "action", { kind: "install" });
  cases.push(checked("install-retained", install, { ownership: owner(), renewed: false, ownershipWrites: [write] }));
  const existing = changed(request(), "ownership", owner());
  cases.push(checked("same-tab", existing, { renewed: true, ownership: owner(), ownershipWrites: [write] }));
  cases.push(checked("install-valid", { ...existing, action: { kind: "install" } }, { ownership: owner(), ownershipWrites: [], renewed: false }));
  cases.push(checked("foreign-expired", changed(changed(existing, "ownership.deviceId", "foreign"), "ownership.leaseExpiresAtMs", 0), {
    reason: "notOwner", renewed: false, ownershipWrites: [] }));
  cases.push(checked("stale-timer", changed(request(), "action.timerId", "stale"), {
    reason: "staleTimer", ownership: owner(), renewed: false, ownershipWrites: [write] }));
  const removed = changed(existing, "workspace.local.commands", []);
  cases.push(checked("removed", removed, { reason: "staleOwner", ownership: null, ownershipWrites: [{ kind: "removeTimerOwner" }] }));
  const expired = changed(request(), "clock.nowMs", 1784550400000);
  cases.push(checked("deadline-missing", expired, { reason: "notClaimable", ownership: null, ownershipWrites: [] }));
  cases.push(checked("deadline-existing", { ...expired, ownership: owner() }, { renewed: true,
    "ownership.leaseExpiresAtMs": 1784550430000 }));
  for (const value of [null, 0]) {
    cases.push(checked(`legacy-lease-${value}`, changed(changed(existing, "localTabId", "renamed"), "ownership.leaseExpiresAtMs", value), {
      renewed: true, "ownership.tabId": "renamed" }));
  }
  for (const kind of ["finish", "cancel", "clear", "replacement"]) {
    const input = structuredClone(existing);
    const start = input.workspace.local.commands[0];
    input.workspace.local.commands.push({ ...start, id: `owner-${kind}`, deviceSequence: 2, hlcCounter: 1,
      type: kind === "replacement" ? "start" : kind, timerId: kind === "replacement" ? "replacement-timer" : start.timerId });
    cases.push(checked(`terminal-${kind}`, input, { reason: "staleOwner", renewed: false,
      ownership: null, ownershipWrites: [{ kind: "removeTimerOwner" }] }));
  }
  cases.push(...ownershipNegatives());
  cases.push(...shapeRejectionCases(), ...shapeLegacyControls());
  return cases;
}

function ownershipNegatives() {
  const cases = [];
  for (const [name, value] of [["false", false], ["array", []], ["empty", {}],
    ["unknown", { ...owner(), owns: true }], ["string-time", { ...owner(), leaseExpiresAtMs: "0" }],
    ["negative-time", { ...owner(), leaseExpiresAtMs: -1 }], ["unsafe-time", { ...owner(), leaseExpiresAtMs: 9007199254740992 }]]) {
    cases.push(vector(operation, `pwa-owner-invalid-${name}`, { ...request(), ownership: value }, false));
  }
  for (const profile of ["appleWorkspace", "androidCoordinator", "desktopStorage", "desktopTerminal"]) {
    cases.push(vector(operation, `pwa-owner-wrong-${profile}`, { ...request(), profile }, false));
  }
  for (const key of ["owns", "claimable", "manual", "droppedTimerIds"]) {
    cases.push(vector(operation, `pwa-owner-computed-${key}`, { ...request(), [key]: true }, false));
  }
  for (const key of ["ownership", "localTabId", "clock"]) {
    const input = request(); delete input[key];
    cases.push(vector(operation, `pwa-owner-missing-${key}`, input, false));
  }
  cases.push(vector(operation, "pwa-owner-expiry-overflow", changed(request(), "clock.leaseDurationMs", 9007199254740991), false, "lease expiry overflow"));
  cases.push(vector(operation, "pwa-owner-fractional-clock", changed(request(), "clock.nowMs", 1.5), false));
  cases.push(vector(operation, "pwa-owner-invalid-context", changed(request(), "workspace.displayContext", false), false));
  return cases;
}

export function ownershipScenarios(call) {
  const initial = call(checked("boundary-install", request(), { ownership: owner(), renewed: true }));
  for (const [offset, renewed, reason] of [[-1, false, "notOwner"], [0, true, ""], [1, true, ""]]) {
    const input = { ...request(), ownership: initial.ownership, localTabId: "tab-renamed",
      clock: { ...request().clock, nowMs: initial.ownership.leaseExpiresAtMs + offset } };
    const expectedOwner = renewed ? { ...initial.ownership, tabId: "tab-renamed",
      leaseExpiresAtMs: input.clock.nowMs + input.clock.leaseDurationMs } : initial.ownership;
    call(branch(operation, `pwa-owner-boundary-${offset}`, input, "pwaLeaseBoundary", {
      equals: { renewed, reason, ownership: expectedOwner,
        ...(!renewed ? { retryAtMs: initial.ownership.leaseExpiresAtMs } : {}),
        ownershipWrites: renewed ? [{ ...expectedOwner, kind: "recordTimerOwner" }] : [], effectsAfterCommit: [] },
      same: { workspace: "workspace" } }));
  }
  canonicalScenarios(call);
}

function canonicalScenarios(call) {
  const input = request();
  const projected = call(vector("workspace.project.v1", "pwa-owner-raw-canonical", { ...input.workspace, now: "1970-01-01T00:00:00Z" }));
  for (const device of ["device-local", "device-foreign", null]) {
    const raw = structuredClone(input);
    raw.action = { kind: "install" };
    raw.workspace.base.canonicalTimer = projected.workspace.canonicalTimer;
    raw.workspace.base.canonicalTimer = structuredClone(raw.workspace.base.canonicalTimer);
    if (device === null) delete raw.workspace.base.canonicalTimer.startedByDeviceId;
    else raw.workspace.base.canonicalTimer.startedByDeviceId = device;
    const claimed = device !== "device-foreign";
    call(branch(operation, `pwa-owner-origin-${device}`, raw, "pwaOwnerOrigin", {
      equals: { ownership: claimed ? owner() : null, renewed: false,
        ownershipWrites: claimed ? [{ ...owner(), kind: "recordTimerOwner" }] : [], effectsAfterCommit: [] },
      same: { workspace: "workspace" } }));
  }
}
