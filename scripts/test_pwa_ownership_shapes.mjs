import assert from "node:assert/strict";
import test from "node:test";
import { requiredShapeRejections, requireShapeFixtures, shapeRejectionCases } from "../tests/aggregate_artifact/pwa_ownership_shapes.mjs";
import { nativeResponses } from "../tests/aggregate_artifact/native_oracle.mjs";
import { fixture } from "../tests/aggregate_artifact/cases.mjs";

for (const name of ["owner-full-tuple", "clock-tuple", "profile-object", "install-controls"]) {
  test(`required artifact shape rejection ${name}`, () => {
    const item = shapeRejectionCases().find((item) => item.name === `pwa-owner-shape-${name}`);
    assert.ok(item, "required fixture omitted");
    nativeResponses([item]);
  });
}

test("required fixture guard rejects dropping any checker reproduction", () => {
  for (const name of requiredShapeRejections) {
    const raw = fixture("pwa-ownership-shapes-v1");
    raw.rejections = raw.rejections.filter((item) => item.name !== name);
    assert.throws(() => requireShapeFixtures(raw), /required raw shape fixtures/);
  }
});
