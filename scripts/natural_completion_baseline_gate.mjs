import assert from "node:assert/strict";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { invoke } from "../tests/aggregate_artifact/abi_host.mjs";

const temp = process.env.PWA12_EVIDENCE_DIR || "/var/folders/r_/_mr22dqn24d31b7460cz8z5m0000gn/T/opencode";
const baseline = JSON.parse(readFileSync(`${temp}/core-pwa11-preserved-envelopes.json`, "utf8"));
const bytes = readFileSync(`${temp}/pwa-core-045/pomodorough_core.wasm`);
const sha256 = createHash("sha256").update(bytes).digest("hex");
assert.equal(sha256, baseline.artifactSha256);
assert.equal(baseline.cases.length, 1077);
const { instance } = await WebAssembly.instantiate(bytes);
const cases = [...baseline.cases, ...baseline.adversarial];
const expected = [...baseline.expected, ...baseline.adversarialExpected];
const result = spawnSync("rustup", ["run", "1.97.1", "cargo", "run", "--quiet", "--locked", "--example", "artifact_parity_oracle"], {
  cwd: new URL("../", import.meta.url), input: cases.map(({ operation, input }) => JSON.stringify({ operation, input })).join("\n") + "\n",
  encoding: "utf8", timeout: 120000, maxBuffer: 64 * 1024 * 1024 });
assert.equal(result.status, 0, result.stderr);
const actual = result.stdout.trimEnd().split("\n"), changes = [], shapeChanges = [];
assert.equal(actual.length, cases.length);
for (const [index, item] of cases.entries()) {
  assert.equal(invoke(instance.exports, item), expected[index], `${item.name}: frozen official envelope changed`);
  if (actual[index] === expected[index]) continue;
  const input = JSON.parse(item.input), before = JSON.parse(expected[index]), after = JSON.parse(actual[index]);
  if (!before.ok && !after.ok && /must be a JSON (object|array|string|scalar)/.test(after.error)) {
    assert.ok(index < baseline.cases.length, "numeric/error controls must remain byte-identical");
    assert.ok(["workspace.completionMutation.v1", "workspace.readModel.v1", "timer.completionState.v1"].includes(item.operation));
    shapeChanges.push({ case: item.name, operation: item.operation, inputRaw: item.input,
      priorEnvelope: expected[index], correctedEnvelope: actual[index],
      reason: "Concrete representation admission now rejects malformed JSON before Serde can accept positional records or enum objects." });
    continue;
  }
  assert.ok(before.ok && after.ok, `${item.operation}/${item.name}: unexpected error/precedence drift\n${expected[index]}\n${actual[index]}`);
  assert.equal(item.operation, "workspace.readModel.v1", `${item.name}: unexpected contract drift`);
  assert.equal(input.profile, "pwaStorage");
  assert.equal(after.value.canonical.status, "completed");
  assert.ok(after.value.availableIntents.includes("finish"));
  assert.deepEqual(after.value.canonical, before.value.canonical);
  assert.deepEqual(after.value.cadence, before.value.cadence);
  assert.deepEqual(after.value.tasks, before.value.tasks);
  const restored = structuredClone(after);
  restored.value.display = before.value.display;
  restored.value.availableIntents = before.value.availableIntents;
  assert.deepEqual(restored, before, `${item.name}: changed non-presentation field`);
  changes.push({ case: item.name, operation: item.operation, inputRaw: item.input,
    priorEnvelope: expected[index], correctedEnvelope: actual[index],
    reason: "Exact natural completion keeps the explicit Finish obligation and advances the implicit PWA display phase. Counts and canonical identity stay unchanged." });
}
const shapeFixture = new URL("../fixtures/pwa-completion-shape-errors-v1.json", import.meta.url);
if (process.argv[2] === "--capture-shape-errors") {
  assert.equal(existsSync(shapeFixture), false, "intentional shape error fixture cannot be refreshed");
  writeFileSync(shapeFixture, JSON.stringify(shapeChanges, null, 2) + "\n");
} else {
  assert.deepEqual(shapeChanges, JSON.parse(readFileSync(shapeFixture, "utf8")), "unexplained envelope drift");
}
writeFileSync(`${temp}/core-pwa12-official-envelope-comparison.json`, JSON.stringify({ officialCases: baseline.cases.length,
  adversarialCases: baseline.adversarial.length, unchanged: cases.length - changes.length - shapeChanges.length,
  intentionalChanges: changes, intentionalShapeErrors: shapeChanges, officialSha256: sha256 }, null, 2));
console.log(`${baseline.cases.length} official envelopes and ${baseline.adversarial.length} controls compared; ${changes.length} presentation changes, ${shapeChanges.length} exact fixture-backed shape errors.`);
