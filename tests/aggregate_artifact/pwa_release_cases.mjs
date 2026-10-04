import assert from "node:assert/strict";
import { changed, fixture, vector } from "./cases.mjs";
import { branch } from "./semantics.mjs";
import { releaseShapeCases } from "./pwa_ownership_shapes.mjs";

const operation = "workspace.ownershipPlan.v1";

export function releaseRequest(item = {}) {
  const seed = fixture("pwa-ownership-plan-v1");
  let raw = seed.request; raw.action = { kind: "release" }; delete raw.clock.leaseDurationMs;
  raw.ownership = Object.hasOwn(item, "owner") ? item.owner : seed.owner;
  for (const [path, value] of Object.entries(item.set ?? {})) raw = changed(raw, path, value);
  for (const path of item.omit ?? []) {
    const keys = path.split("."); const parent = keys.slice(0, -1).reduce((value, key) => value[key], raw);
    delete parent[keys.at(-1)];
  }
  return raw;
}

function success(item, input = releaseRequest(item), hit = "pwaRelease") {
  const ownership = structuredClone(input.ownership);
  if (item.writes) ownership.leaseExpiresAtMs = input.clock.nowMs;
  return branch(operation, `pwa-release-${item.name}`, input, hit, { equals: {
    schemaVersion: 1, ownership, renewed: false, reason: item.reason,
    ownershipWrites: item.writes ? [{ ...ownership, kind: "recordTimerOwner" }] : [], effectsAfterCommit: [] },
    same: { workspace: "workspace" } });
}

export function releaseCases() {
  const matrix = fixture("pwa-ownership-release-v1");
  assert.equal(matrix.successes.length, 21, "required release successes");
  assert.equal(matrix.rejections.length, 31, "required release rejections");
  const negatives = matrix.rejections.map((item) => {
    const raw = releaseRequest(item); let input = JSON.stringify(raw);
    if (item.duplicate) input = `{"${item.duplicate}":${JSON.stringify(raw[item.duplicate])},${input.slice(1)}`;
    return { ...vector(operation, `pwa-release-invalid-${item.name}`, input, false), rejectionHit: "pwaRelease" };
  });
  return [...matrix.successes.map((item) => success(item)), ...negatives, ...releaseShapeCases()];
}

export function releaseScenarios(call) {
  const seed = fixture("pwa-ownership-plan-v1");
  const installed = call(vector(operation, "pwa-release-install-chain", seed.request));
  for (const offset of [-1, 0, 1]) {
    for (const peer of [false, true]) {
      const input = releaseRequest({ set: { ownership: installed.ownership,
        localTabId: peer ? "peer" : installed.ownership.tabId,
        "clock.nowMs": installed.ownership.leaseExpiresAtMs + offset } });
      const released = call(success({ name: `boundary-${peer}-${offset}`, writes: peer ? 0 : 1,
        reason: peer ? "notOwner" : "" }, input, "pwaReleaseBoundary"));
      if (!peer) {
        const renewed = { ...seed.request, ownership: released.ownership, localTabId: "reopened",
          clock: { nowMs: input.clock.nowMs, leaseDurationMs: seed.request.clock.leaseDurationMs } };
        call(branch(operation, `pwa-release-takeover-${offset}`, renewed, "pwaReleaseTakeover", { equals: {
          renewed: true, "ownership.tabId": "reopened", effectsAfterCommit: [] }, same: { workspace: "workspace" } }));
      }
    }
  }
  for (const kind of ["pause", "finish", "cancel", "clear", "replacement"]) {
    const input = releaseRequest(); const start = input.workspace.local.commands[0];
    input.workspace.local.commands.push({ ...start, id: `release-${kind}`, deviceSequence: 2, hlcCounter: 1,
      type: kind === "replacement" ? "start" : kind, timerId: kind === "replacement" ? "new-timer" : start.timerId });
    call(success({ name: `terminal-${kind}`, writes: 1, reason: "" }, input, "pwaReleaseTerminal"));
  }
}
