// Execute downloaded 0.44 bytes and native dispatch. Never build WASM.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { fixture, vector } from "../tests/aggregate_artifact/cases.mjs";
import { invoke } from "../tests/aggregate_artifact/abi_host.mjs";
import { nativeCorpus } from "../tests/aggregate_artifact/corpus.mjs";

const [officialPath, baselineOracle, capturesPath, outputPath] = process.argv.slice(2);
assert.ok(officialPath && baselineOracle && capturesPath && outputPath,
  "usage: node scripts/pwa_display_admission_probe.mjs OFFICIAL_044_WASM BASELINE_ORACLE CAPTURES_ROOT OUTPUT_JSON");
const bytes = readFileSync(officialPath);
assert.equal(bytes.length, 2531627);
assert.equal(createHash("sha256").update(bytes).digest("hex"), fixture("pwa-display-admission-v1").provenance.officialSha256);
const { instance } = await WebAssembly.instantiate(bytes, {});
const corpus = nativeCorpus();
const old = baseline(corpus.cases);
const comparisons = corpus.cases.map((item, index) => {
  const officialRaw = invoke(instance.exports, item);
  assert.equal(officialRaw, old[index], `${item.name}: native 0.44 and downloaded artifact raw parity`);
  const changed = officialRaw !== corpus.expected[index];
  if (changed && item.operation === "workspace.ownershipPlan.v1") {
    assert.equal(JSON.parse(officialRaw).error, "unsupported shared-core operation: workspace.ownershipPlan.v1");
  } else if (changed) assert.match(item.name, /^pwa-(admit-|duration-context-claimed)/);
  return { ...item, nativeEnvelopeRaw: corpus.expected[index], officialEnvelopeRaw: officialRaw, changed };
});
const captures = [];
for (const phase of ["short_break", "long_break"]) {
  for (const kind of ["duration", "completion"]) captures.push(captured(phase, kind));
}
writeFileSync(outputPath, JSON.stringify({ officialSha256: fixture("pwa-display-admission-v1").provenance.officialSha256,
  hits: corpus.hits, comparisons, captures, changedCount: comparisons.filter((item) => item.changed).length }, null, 2));
console.log(JSON.stringify({ cases: comparisons.length, rawOfficialBaselineMatches: comparisons.length,
  changed: comparisons.filter((item) => item.changed).length, exactCapturedRedGreen: captures.length, outputPath }));

function baseline(cases) {
  const input = cases.map(({ operation, input }) => JSON.stringify({ operation, input })).join("\n") + "\n";
  const result = spawnSync(baselineOracle, [], { input, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stderr);
  return result.stdout.trimEnd().split("\n");
}

function captured(phase, kind) {
  const inputRaw = readFileSync(`${capturesPath}/pwa044-${phase}-${kind}-request.json`, "utf8");
  const saved = JSON.parse(readFileSync(`${capturesPath}/pwa044-${phase}-${kind}-return.json`, "utf8"));
  const operation = kind === "duration" ? "workspace.intent.v1" : "workspace.completionMutation.v1";
  const item = vector(operation, `exact-${phase}-${kind}`, inputRaw);
  const officialEnvelopeRaw = invoke(instance.exports, item);
  assert.equal(officialEnvelopeRaw, baseline([item])[0]);
  const official = JSON.parse(officialEnvelopeRaw);
  assert.deepEqual(official.value, saved);
  const result = spawnSync("rustup", ["run", "1.97.1", "cargo", "run", "--quiet", "--locked", "--example", "artifact_parity_oracle"], {
    input: JSON.stringify({ operation, input: inputRaw }) + "\n", encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  assert.equal(result.status, 0, result.stderr);
  const native = JSON.parse(result.stdout);
  assert.equal(native.ok, true, native.error);
  const expected = phase === "short_break" ? 600000 : 1800000;
  assert.equal(native.value.projection.durationsMs[phase], expected);
  assert.equal(official.value.projection.durationsMs[phase], expected / 2);
  if (kind === "duration") {
    assert.equal(native.value.groupOutcomes.durationOperations[0].outcome, "applied");
    assert.equal(official.value.groupOutcomes.durationOperations[0].outcome, "queued");
    assert.deepEqual(native.value.workspace.local, official.value.workspace.local);
    assert.deepEqual(native.value.workspace.neverSent, official.value.workspace.neverSent);
    if (phase === "short_break") assert.deepEqual(JSON.parse(inputRaw), fixture("pwa-display-admission-v1").request);
  } else {
    assert.equal(native.value.commands[1].plannedDurationMs, expected);
    assert.equal(official.value.commands[1].plannedDurationMs, expected / 2);
  }
  assert.deepEqual(native.value.workspace.base, official.value.workspace.base);
  assert.equal(native.value.workspace.canonicalHead, null);
  return { operation, inputRaw, decoded: JSON.parse(inputRaw), savedCompleteReturn: saved,
    officialEnvelopeRaw, nativeEnvelopeRaw: result.stdout.trimEnd() };
}
