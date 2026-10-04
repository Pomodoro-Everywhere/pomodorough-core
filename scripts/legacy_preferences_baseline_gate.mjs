import assert from "node:assert/strict";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { nativeCorpus } from "../tests/aggregate_artifact/corpus.mjs";
import { invoke } from "../tests/aggregate_artifact/abi_host.mjs";

const temp = process.env.PWA11_EVIDENCE_DIR || "/var/folders/r_/_mr22dqn24d31b7460cz8z5m0000gn/T/opencode";
const baselinePath = `${temp}/core-pwa11-preserved-envelopes.json`;
const binary = process.env.PWA11_BASELINE_ORACLE || `${temp}/core-pwa11-numeric-baseline`;
const artifact = process.env.PWA11_OFFICIAL_ARTIFACT || `${temp}/pwa-core-045/pomodorough_core.wasm`;
const hash = (bytes) => createHash("sha256").update(bytes).digest("hex");

function dispatch(cases, preserved) {
  const input = cases.map(({ operation, input }) => JSON.stringify({ operation, input })).join("\n") + "\n";
  const command = preserved ? binary : "rustup";
  const args = preserved ? [] : ["run", "1.97.1", "cargo", "run", "--quiet", "--locked", "--example", "artifact_parity_oracle"];
  const result = spawnSync(command, args, { cwd: new URL("../", import.meta.url), input, encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024, timeout: 120000 });
  assert.equal(result.status, 0, result.stderr);
  const responses = result.stdout.trimEnd().split("\n");
  assert.equal(responses.length, cases.length);
  return responses;
}

function adversarial(cases) {
  const seeds = ["workspace.project.v1", "workspace.intent.v1", "workspace.ownershipPlan.v1", "reconcile.rebase.v3"];
  return seeds.flatMap((operation) => {
    const seed = cases.find((item) => item.operation === operation && item.ok);
    return ["1.0", "9007199254740992", "18446744073709551616", "1e400", "90.49999999999999"].map((token) => ({
      operation, name: `baseline-numeric-${token}`, input: seed.input.replace(/"hlcCounter":\d+/, `"hlcCounter":${token}`)
        .replace(/"leaseDurationMs":\d+/, `"leaseDurationMs":${token}`),
    }));
  });
}

if (process.argv[2] === "--capture") {
  assert.equal(existsSync(baselinePath), false, "preserved baseline cannot be refreshed");
  const corpus = nativeCorpus();
  const cases = corpus.cases.filter((item) => item.operation !== "workspace.legacyPreferences.v1");
  assert.equal(cases.length, 1077, "all official 0.45 cases must be preserved");
  const bytes = readFileSync(artifact);
  assert.equal(hash(bytes), "845090328b2f44056480c3930e9bb684a3874b8f3cbcbd4253ddd92f67c6f5d6");
  const { instance } = await WebAssembly.instantiate(bytes);
  const expected = dispatch(cases, true);
  cases.forEach((item, index) => assert.equal(invoke(instance.exports, item), expected[index], item.name));
  const extra = adversarial(cases);
  writeFileSync(baselinePath, JSON.stringify({ artifactSha256: hash(bytes), nativeSha256: hash(readFileSync(binary)),
    cases, expected, adversarial: extra, adversarialExpected: dispatch(extra, true) }, null, 2));
  console.log(`Preserved ${cases.length} exact official/native envelopes and ${extra.length} numeric/error controls.`);
} else {
  const baseline = JSON.parse(readFileSync(baselinePath, "utf8"));
  assert.equal(hash(readFileSync(binary)), baseline.nativeSha256, "baseline executable changed");
  assert.deepEqual(dispatch(baseline.cases, false), baseline.expected, "existing envelope drift");
  assert.deepEqual(dispatch(baseline.adversarial, false), baseline.adversarialExpected, "numeric/error precedence drift");
  writeFileSync(`${temp}/core-pwa11-preserved-envelope-gate.json`, JSON.stringify({
    officialCases: baseline.cases.length, adversarialCases: baseline.adversarial.length,
    differences: 0, artifactSha256: baseline.artifactSha256, baselineNativeSha256: baseline.nativeSha256,
  }, null, 2));
  console.log(`Zero differences across ${baseline.cases.length} official envelopes and ${baseline.adversarial.length} controls.`);
}
