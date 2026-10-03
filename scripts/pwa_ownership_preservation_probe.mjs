// Compare accepted PWA07 raw envelopes and downloaded official missing-operation returns.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { nativeCorpus } from "../tests/aggregate_artifact/corpus.mjs";
import { invoke } from "../tests/aggregate_artifact/abi_host.mjs";
import { rejectionCoverage } from "../tests/aggregate_artifact/semantics.mjs";

const [acceptedPath, officialPath, outputPath] = process.argv.slice(2);
assert.ok(acceptedPath && officialPath && outputPath,
  "usage: node scripts/pwa_ownership_preservation_probe.mjs ACCEPTED_PWA07_JSON OFFICIAL_044_WASM OUTPUT_JSON");
const accepted = JSON.parse(readFileSync(acceptedPath, "utf8"));
const oracle = process.env.CORE_PWA09_ORACLE || new URL("../target/debug/examples/artifact_parity_oracle", import.meta.url).pathname;
const requests = accepted.comparisons.map(({ operation, input }) => JSON.stringify({ operation, input })).join("\n") + "\n";
const native = spawnSync(oracle, [], { input: requests, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
assert.equal(native.status, 0, native.stderr);
const envelopes = native.stdout.trimEnd().split("\n");
assert.equal(envelopes.length, accepted.comparisons.length);
const preserved = accepted.comparisons.map((item, index) => {
  assert.equal(envelopes[index], item.nativeEnvelopeRaw, `${item.operation}/${item.name}: accepted PWA07 drift`);
  return { operation: item.operation, name: item.name, inputRaw: item.input,
    acceptedEnvelopeRaw: item.nativeEnvelopeRaw, nativeEnvelopeRaw: envelopes[index] };
});
const bytes = readFileSync(officialPath);
const officialSha256 = createHash("sha256").update(bytes).digest("hex");
assert.equal(officialSha256, "895621370566284e08f03385146bdfe07b41c78212273dd8124302d05dfeaed4");
const { instance } = await WebAssembly.instantiate(bytes, {});
const corpus = nativeCorpus();
const added = corpus.cases.flatMap((item, index) => {
  if (item.operation !== "workspace.ownershipPlan.v1") return [];
  const officialEnvelopeRaw = invoke(instance.exports, item);
  const envelope = JSON.parse(officialEnvelopeRaw);
  assert.equal(envelope.ok, false);
  assert.equal(envelope.error, "unsupported shared-core operation: workspace.ownershipPlan.v1");
  return [{ ...item, officialEnvelopeRaw, nativeEnvelopeRaw: corpus.expected[index] }];
});
assert.ok(added.some((item) => item.ok) && added.some((item) => !item.ok));
writeFileSync(outputPath, JSON.stringify({ officialSha256, acceptedPath, preserved, added, cases: corpus.cases.length,
  hits: corpus.hits, rejectionHits: rejectionCoverage(corpus.cases) }, null, 2));
console.log(JSON.stringify({ cases: corpus.cases.length, preservedRawEnvelopes: preserved.length, ownershipDispatches: added.length,
  ownershipSuccesses: added.filter((item) => item.ok).length, ownershipRejections: added.filter((item) => !item.ok).length, outputPath }));
