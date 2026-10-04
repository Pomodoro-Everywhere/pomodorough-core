import assert from "node:assert/strict";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";

const directory = process.env.PWA10_EVIDENCE_DIR || "/var/folders/r_/_mr22dqn24d31b7460cz8z5m0000gn/T/opencode";
const fixturePath = new URL("../fixtures/legacy-dependency-checker-v1.json", import.meta.url);
const operation = "workspace.legacyDependencyPlan.v1";
const oracle = process.env.PWA10_NATIVE_ORACLE || new URL("../target/debug/examples/artifact_parity_oracle", import.meta.url).pathname;

if (process.argv.includes("--capture")) {
  assert.equal(existsSync(fixturePath), false, "checker baseline cannot be replaced");
  const original = JSON.parse(readFileSync(`${directory}/core-pwa10-independent-check.json`, "utf8"));
  assert.equal(original.cases, 70); assert.equal(original.passed, 61); assert.equal(original.findings.length, 9);
  const cases = original.findings.map(({ name, input, rawEnvelope, assertion }) => ({
    name, operation, inputRaw: input, originalEnvelope: rawEnvelope, originalAssertion: assertion,
    expected: name.startsWith("canonical-history-") ? "denied" : "blocked" }));
  writeFileSync(fixturePath, JSON.stringify({ originalCases: 70, originalPassed: 61, cases }, null, 2) + "\n");
}

const fixture = JSON.parse(readFileSync(fixturePath, "utf8"));
const mode = process.argv.includes("--red") || process.argv.includes("--capture") ? "red" : "green";
const receipts = [];
for (const item of fixture.cases) {
  const input = JSON.parse(item.inputRaw);
  const result = spawnSync(oracle, [], { input: JSON.stringify({ operation, input: item.inputRaw }) + "\n",
    encoding: "utf8", timeout: 120000, maxBuffer: 16 * 1024 * 1024 });
  assert.equal(result.status, 0, result.stderr);
  const actual = result.stdout.trimEnd(), envelope = JSON.parse(actual);
  if (mode === "red") assert.equal(actual, item.originalEnvelope, `${item.name}: original rejection receipt changed`);
  else {
    if (item.expected === "blocked") assert.equal(envelope.ok, true, `${item.name}: ${envelope.error}`);
    if (envelope.ok) {
      assert.equal(envelope.value.outcome, "blocked", item.name);
      assert.deepEqual(envelope.value.workspace, input.workspace);
      assert.deepEqual(envelope.value.outgoing, input.outgoing);
      assert.deepEqual(envelope.value.metadataWrites, []);
      assert.equal(envelope.value.timerDependencies, null);
      assert.equal(envelope.value.recovery.blocksSync, true);
      assert.equal(envelope.value.recovery.blocksMutations, true);
      const restarted = spawnSync(oracle, [], { input: JSON.stringify({ operation,
        input: JSON.stringify({ ...input, workspace: envelope.value.workspace }) }) + "\n", encoding: "utf8" });
      assert.equal(restarted.status, 0, restarted.stderr);
      assert.equal(restarted.stdout.trimEnd(), actual, `${item.name}: reopen changed complete blocked return`);
    }
  }
  receipts.push({ ...item, actualEnvelope: actual });
}
writeFileSync(`${directory}/core-pwa10-checker-${mode}-receipts.json`, JSON.stringify({ mode, cases: receipts.length, receipts }, null, 2));
console.log(`${receipts.length} exact checker inputs verified in ${mode} mode.`);
