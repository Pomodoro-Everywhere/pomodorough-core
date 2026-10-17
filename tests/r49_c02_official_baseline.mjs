import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { invoke } from "./aggregate_artifact/abi_host.mjs";

const artifact = process.argv[2];
assert.equal(process.argv.length, 3, "usage: node tests/r49_c02_official_baseline.mjs <official-v0.48.0.wasm>");
const bytes = readFileSync(artifact);
assert.equal(createHash("sha256").update(bytes).digest("hex"),
  "775d3d85cc4f6ced2243fd8b831e0057d6264e2202fdc2bf28d83b62e311e05a", "official v0.48.0 digest");
const run = spawnSync("rustup", ["run", "1.97.1", "cargo", "test", "--locked",
  "--test", "r49_c02_causal_break", "--", "--nocapture", "--test-threads=1"], {
  cwd: fileURLToPath(new URL("../", import.meta.url)), encoding: "utf8",
  env: { ...process.env, R49_C02_RECEIPTS: "1", RUST_BACKTRACE: "0" },
  timeout: 120000, maxBuffer: 16 * 1024 * 1024,
});
assert.ifError(run.error);
assert.equal(run.status, 0, `${run.stderr}\n${run.stdout}`);
const receipts = run.stdout.split("\n").filter(line => line.includes("RECEIPT="))
  .map(line => JSON.parse(line.slice(line.indexOf("RECEIPT=") + "RECEIPT=".length)));
assert.equal(receipts.length, 37, "complete regression trace");
const { instance } = await WebAssembly.instantiate(bytes);
let preserved = 0;
const changed = [];
for (const receipt of receipts) {
  const official = invoke(instance.exports, receipt);
  const expected = JSON.stringify(receipt.publishedEnvelope ?? { ok: true, value: receipt.published });
  assert.equal(official, expected, `${receipt.name}: complete official envelope`);
  const native = JSON.parse(receipt.native);
  const published = JSON.parse(official);
  if (receipt.native === official) {
    preserved += 1;
    continue;
  }
  assert.equal(native.ok, true, receipt.name);
  if (receipt.operation === "timer.completionPlan.v1") {
    assert.equal(published.value.generatedBreakPhase, "short_break", receipt.name);
    assert.equal(native.value.generatedBreakPhase, "long_break", receipt.name);
    published.value.generatedBreakPhase = "long_break";
  } else {
    assert.equal(receipt.name, "same command key before exact source");
    assert.equal(published.value.pending[0].phase, "long_break");
    assert.equal(published.value.pending[0].plannedDurationMs, 180000);
    assert.equal(native.value.pending[0].phase, "short_break");
    assert.equal(native.value.pending[0].plannedDurationMs, 120000);
    published.value.pending[0].phase = "short_break";
    published.value.pending[0].plannedDurationMs = 120000;
  }
  assert.deepEqual(native, published, `${receipt.name}: no unexplained envelope drift`);
  changed.push(`${receipt.operation}/${receipt.name}`);
}
assert.equal(preserved, 26, "preserved complete envelopes");
assert.equal(changed.length, 11, "fixture-backed causal corrections");
console.log(`Official v0.48.0 baseline: ${receipts.length} complete envelopes checked, ${preserved} unchanged`);
console.log(`Intentional causal corrections: ${JSON.stringify(changed)}`);
