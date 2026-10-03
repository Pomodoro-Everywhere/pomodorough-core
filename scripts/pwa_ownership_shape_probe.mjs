import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { requiredShapeRejections, shapeRejectionCases } from "../tests/aggregate_artifact/pwa_ownership_shapes.mjs";

const [baselineOracle, outputPath] = process.argv.slice(2);
assert.ok(baselineOracle && outputPath, "usage: node scripts/pwa_ownership_shape_probe.mjs BASELINE_ORACLE OUTPUT_JSON");
const oracle = process.env.CORE_PWA09_ORACLE || new URL("../target/debug/examples/artifact_parity_oracle", import.meta.url).pathname;
const primary = shapeRejectionCases().filter((item) => requiredShapeRejections.some((name) => item.name === `pwa-owner-shape-${name}`));
assert.equal(primary.length, 7);
const receipts = primary.map((item) => {
  const baselineEnvelopeRaw = dispatch(baselineOracle, item);
  const nativeEnvelopeRaw = dispatch(oracle, item);
  const baseline = JSON.parse(baselineEnvelopeRaw), current = JSON.parse(nativeEnvelopeRaw);
  assert.equal(baseline.ok, true, `${item.name}: reproduction lost baseline-red behavior`);
  assert.equal(current.ok, false, `${item.name}: still accepted`);
  assert.deepEqual(Object.keys(current).sort(), ["error", "ok"]);
  return { ...item, baselineEnvelopeRaw, nativeEnvelopeRaw };
});
writeFileSync(outputPath, JSON.stringify({ baselineOracle, receipts, redCount: 7, greenCount: 7 }, null, 2));
console.log(JSON.stringify({ redCount: 7, greenCount: 7, outputPath }));

function dispatch(binary, item) {
  const returned = spawnSync(binary, [], { input: JSON.stringify({ operation: item.operation, input: item.input }) + "\n",
    encoding: "utf8", maxBuffer: 16 * 1024 * 1024 });
  assert.equal(returned.status, 0, returned.stderr);
  return returned.stdout.trimEnd();
}
