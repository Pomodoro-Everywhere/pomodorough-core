import assert from "node:assert/strict";
import { changed, fixture, vector } from "./cases.mjs";

export const requiredShapeRejections = ["owner-full-tuple", "owner-short-tuple", "clock-tuple", "profile-object",
  "install-controls", "display-profile-object", "missing-never-sent"];

export function shapeRejectionCases() {
  const shapeFixture = fixture("pwa-ownership-shapes-v1");
  requireShapeFixtures(shapeFixture);
  const cases = shapeFixture.rejections;
  const primary = cases.map((item) => {
    let raw = structuredClone(fixture("pwa-ownership-plan-v1").request);
    if (Object.hasOwn(item, "value")) raw = changed(raw, item.path, item.value);
    else {
      const keys = item.path.split(".");
      const parent = keys.slice(0, -1).reduce((value, key) => value[key], raw);
      delete parent[keys.at(-1)];
    }
    return rejected(item.name, raw);
  });
  return [...primary, ...fieldCases(shapeFixture), ...controlCases(shapeFixture)];
}

export function requireShapeFixtures(shapeFixture) {
  assert.deepEqual(shapeFixture.rejections.map((item) => item.name), requiredShapeRejections, "required raw shape fixtures");
  assert.ok(shapeFixture.fieldShapes.length, "required concrete field fixtures");
  assert.equal(new Set(shapeFixture.fieldShapes.map((item) => item.path)).size, shapeFixture.fieldShapes.length,
    "duplicate concrete field fixture");
  assert.ok(shapeFixture.closedObjects.includes(""), "root unknown-control fixtures required");
}

function rejected(name, raw) {
  return { ...vector("workspace.ownershipPlan.v1", `pwa-owner-shape-${name}`, raw, false), rejectionHit: "pwaOwnershipShape" };
}

function populatedRequest() {
  const seed = fixture("pwa-ownership-plan-v1");
  const raw = structuredClone(seed.request);
  raw.ownership = seed.owner;
  raw.workspace.canonicalHead = { wallMs: 1784548800000, counter: 0 };
  raw.workspace.displayContext.projectionPending = structuredClone(raw.workspace.local);
  raw.workspace.neverSent = Object.fromEntries(Object.keys(raw.workspace.local).map((name) => [name, []]));
  return raw;
}

function replace(raw, path, value) {
  return path ? changed(raw, path, value) : value;
}

function fieldCases(shapeFixture, populate = populatedRequest) {
  const cases = [];
  for (const field of shapeFixture.fieldShapes) {
    const name = field.path || "root";
    const tuple = field.shape === "objectArray" ? [["timer", "device"]]
      : field.shape === "stringArray" ? [{ pwaStorage: null }]
      : field.path === "clock" ? [1784548801000, 30000]
      : ["ownership-timer", "device-local", "tab-local", 1784548831000];
    const values = [["populated-array", tuple], ["enum-object", { pwaStorage: null }]];
    if (!field.shape.startsWith("nullable")) values.push(["null", null]);
    for (const [kind, value] of values) cases.push(rejected(`field-${name}-${kind}`, replace(populate(), field.path, value)));
    if (field.required && field.path) {
      const raw = populate(), keys = field.path.split(".");
      const parent = keys.slice(0, -1).reduce((value, key) => value[key], raw);
      delete parent[keys.at(-1)];
      cases.push(rejected(`field-${name}-omitted`, raw));
    }
  }
  return cases;
}

function controlCases(shapeFixture, populate = populatedRequest, action = "install") {
  const cases = [];
  for (const path of shapeFixture.closedObjects) {
    for (const value of [false, true, null]) {
      cases.push(rejected(`control-${path || "root"}-${value}`, changed(populate(),
        path ? `${path}.unknownPolicyControl` : "unknownPolicyControl", value)));
    }
  }
  for (const key of ["claimable", "owns", "manual"]) {
    for (const value of [false, true, null]) {
      cases.push(rejected(`${action}-${key}-${value}`, changed(populate(), "action", { kind: action, [key]: value })));
    }
  }
  return cases;
}

export function releaseShapeCases() {
  const shapeFixture = fixture("pwa-ownership-shapes-v1");
  requireShapeFixtures(shapeFixture);
  shapeFixture.fieldShapes = shapeFixture.fieldShapes.filter(({ path }) => !["action.timerId", "clock.leaseDurationMs"].includes(path));
  const populate = () => {
    const raw = populatedRequest(); raw.action = { kind: "release" }; delete raw.clock.leaseDurationMs;
    return raw;
  };
  return [...fieldCases(shapeFixture, populate), ...controlCases(shapeFixture, populate, "release")].map((item) => ({
    ...item, name: item.name.replace("pwa-owner-shape-", "pwa-release-shape-"), rejectionHit: "pwaReleaseShape" }));
}

export function shapeLegacyControls() {
  return ["tabId", "leaseExpiresAtMs"].flatMap((key) => ["absent", "null"].map((presence) => {
    const raw = populatedRequest(); raw.action = { kind: "install" };
    if (presence === "absent") delete raw.ownership[key]; else raw.ownership[key] = null;
    return { ...vector("workspace.ownershipPlan.v1", `pwa-owner-legacy-${key}-${presence}`, raw), hit: "pwaOwnershipLegacy",
      checks: { equals: { ownershipWrites: [], renewed: false, effectsAfterCommit: [] }, same: { ownership: "ownership", workspace: "workspace" } } };
  }));
}
