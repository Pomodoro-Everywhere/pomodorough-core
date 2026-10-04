import assert from "node:assert/strict";
import test from "node:test";
import { probeRunner, verifyRunner } from "./aggregate_runner_probe.mjs";

function replacement(before, after) {
  return (source) => {
    assert.ok(source.includes(before), `mutation target missing: ${before}`);
    return source.replace(before, after);
  };
}

test("actual runner reaches pinned native oracle and every host dispatch", async () => {
  await verifyRunner();
});

test("static checker kills actual native-oracle bypass mutant", async () => {
  const mutations = { "aggregate_wasm_parity.mjs": replacement("const corpus = nativeCorpus();",
    "const corpus = {staticCases:[],cases:[],expected:[],hits:{}};") };
  await assert.rejects(() => probeRunner(mutations, { argument: "--native-only" }), /native oracle/);
  await assert.rejects(() => verifyRunner(mutations), /native trace|branch hit counts|native oracle/);
});

test("static checker kills skipped required vector mutant inside actual corpus runner", async () => {
  const mutations = { "aggregate_artifact/corpus.mjs": replacement("const staticCases = aggregateCases();",
    "const staticCases = aggregateCases().slice(1);") };
  await assert.rejects(() => verifyRunner(mutations), /semantic branch hit counts/);
});

test("static checker kills actual native comparison bypass mutant", async () => {
  const mutations = { "aggregate_artifact/abi_host.mjs": replacement(
    "assertParity(item, corpus.expected[cursor++], actual);", "cursor += 1;") };
  // With the comparison removed the mismatch run succeeds, so verifyRunner itself fails.
  await assert.rejects(() => verifyRunner(mutations), /actual runner accepted artifact\/native mismatch/);
});

test("static checker kills skipped stateful generated dependency scenario mutant", async () => {
  const mutations = { "aggregate_artifact/corpus.mjs": replacement("generatedScenarios(call);", "void call;") };
  await assert.rejects(() => verifyRunner(mutations), /semantic branch hit counts/);
});

test("static checker kills entire artifact runner bypass mutant", async () => {
  const mutations = { "aggregate_wasm_parity.mjs": replacement(
    "exerciseArtifact(instance.exports, cases, expected, corpus);", "void instance;") };
  await assert.rejects(() => probeRunner(mutations), /skipped required dispatches/);
});

test("static checker kills skipped PWA display admission scenarios", async () => {
  await assert.rejects(() => verifyRunner({ "aggregate_artifact/corpus.mjs": (source) =>
    source.replace("  admissionScenarios(call);", "") }), /semantic branch hit counts/);
});

test("static checker kills skipped actual owner lease boundary scenarios", async () => {
  await assert.rejects(() => verifyRunner({ "aggregate_artifact/corpus.mjs": (source) =>
    source.replace("  ownershipScenarios(call);", "") }), /semantic branch hit counts/);
});

test("static checker kills skipped raw shape rejection dispatches", async () => {
  await assert.rejects(() => verifyRunner({ "aggregate_artifact/corpus.mjs": (source) =>
    source.replace("  const staticCases = aggregateCases();", "  const staticCases = aggregateCases().filter((item) => !item.rejectionHit);") }),
  /required raw rejection hit counts/);
});

test("static checker kills skipped legacy restart dispatches", async () => {
  await assert.rejects(() => verifyRunner({ "aggregate_artifact/corpus.mjs": (source) =>
    source.replace("  legacyScenarios(call);", "") }), /semantic branch hit counts/);
});

test("static checker kills skipped raw legacy rejection dispatches", async () => {
  await assert.rejects(() => verifyRunner({ "aggregate_artifact/corpus.mjs": (source) =>
    source.replace("  const staticCases = aggregateCases();", "  const staticCases = aggregateCases().filter((item) => item.rejectionHit !== 'legacyPreferences');") }),
  /required raw rejection hit counts/);
});

test("static checker kills skipped numeric preservation dispatches", async () => {
  await assert.rejects(() => verifyRunner({ "aggregate_artifact/corpus.mjs": (source) =>
    source.replace("  const staticCases = aggregateCases();", "  const staticCases = aggregateCases().filter((item) => item.hit !== 'legacyNumeric');") }),
  /semantic branch hit counts/);
});

test("static checker kills skipped numeric rejection dispatches", async () => {
  await assert.rejects(() => verifyRunner({ "aggregate_artifact/corpus.mjs": (source) =>
    source.replace("  const staticCases = aggregateCases();", "  const staticCases = aggregateCases().filter((item) => item.rejectionHit !== 'legacyNumeric');") }),
  /required raw rejection hit counts/);
});

test("static checker kills skipped natural lifecycle dispatches", async () => {
  await assert.rejects(() => verifyRunner({ "aggregate_artifact/corpus.mjs": (source) =>
    source.replace("  naturalScenarios(call);", "") }), /semantic branch hit counts/);
});

test("static checker kills skipped natural evidence rejections", async () => {
  await assert.rejects(() => verifyRunner({ "aggregate_artifact/corpus.mjs": (source) =>
    source.replace("  const staticCases = aggregateCases();", "  const staticCases = aggregateCases().filter((item) => item.rejectionHit !== 'pwaNatural');") }),
  /required raw rejection hit counts/);
});

test("static checker kills skipped completion representation guards", async () => {
  await assert.rejects(() => verifyRunner({ "aggregate_artifact/corpus.mjs": (source) =>
    source.replace("  const staticCases = aggregateCases();", "  const staticCases = aggregateCases().filter((item) => item.rejectionHit !== 'pwaStructure');") }),
  /required raw rejection hit counts/);
});

test("static checker kills skipped consumed natural provenance cases", async () => {
  await assert.rejects(() => verifyRunner({ "aggregate_artifact/corpus.mjs": (source) =>
    source.replace("  const staticCases = aggregateCases();", "  const staticCases = aggregateCases().filter((item) => item.hit !== 'pwaProvenance');") }),
  /semantic branch hit counts/);
});

test("static checker kills skipped dependency restart dispatches", async () => {
  await assert.rejects(() => verifyRunner({ "aggregate_artifact/corpus.mjs": (source) =>
    source.replace("  dependencyScenarios(call);", "") }), /semantic branch hit counts/);
});

test("static checker kills skipped dependency raw rejections", async () => {
  await assert.rejects(() => verifyRunner({ "aggregate_artifact/corpus.mjs": (source) =>
    source.replace("  const staticCases = aggregateCases();", "  const staticCases = aggregateCases().filter((item) => item.rejectionHit !== 'legacyDependencies');") }),
  /required raw rejection hit counts/);
});

test("static checker kills skipped release boundary dispatches", async () => {
  await assert.rejects(() => verifyRunner({ "aggregate_artifact/corpus.mjs": (source) =>
    source.replace("  releaseScenarios(call);", "") }), /semantic branch hit counts/);
});

for (const hit of ["pwaRelease", "pwaReleaseShape"]) test(`static checker kills skipped ${hit} rejections`, async () => {
  await assert.rejects(() => verifyRunner({ "aggregate_artifact/corpus.mjs": (source) =>
    source.replace("  const staticCases = aggregateCases();", `  const staticCases = aggregateCases().filter((item) => item.rejectionHit !== '${hit}');`) }),
  /required raw rejection hit counts/);
});

test("static checker rejects skipped explicit-choice lifecycle dispatches", async () => {
  await assert.rejects(() => verifyRunner({ "aggregate_artifact/corpus.mjs": (source) =>
    source.replace("  selectionScenarios(call);", "") }), /semantic branch hit counts/);
});

test("static checker rejects skipped selection representation dispatches", async () => {
  await assert.rejects(() => verifyRunner({ "aggregate_artifact/corpus.mjs": (source) =>
    source.replace("  const staticCases = aggregateCases();", "  const staticCases = aggregateCases().filter((item) => item.rejectionHit !== 'pwaChoiceShape');") }),
  /required raw rejection hit counts/);
});

test("static checker rejects skipping the immutable old-envelope preservation gate", async () => {
  await assert.rejects(() => verifyRunner({ "aggregate_artifact/corpus.mjs": (source) =>
    source.replace("  assertSelectionPreservation(cases, expected);", "") }), /preserved old envelopes/);
});

test("static checker rejects missing public cycle and discharge scenarios", async () => {
  await assert.rejects(() => verifyRunner({ "aggregate_artifact/corpus.mjs": (source) =>
    source.replace("  cycleScenarios(call);", "") }), /semantic branch hit counts/);
});

test("static checker rejects missing fabricated-discharge and durable-evidence rejections", async () => {
  await assert.rejects(() => verifyRunner({ "aggregate_artifact/corpus.mjs": (source) =>
    source.replace("  const staticCases = aggregateCases();", "  const staticCases = aggregateCases().filter((item) => item.rejectionHit !== 'pwaDischargeShape');") }),
    /required raw rejection hit counts/);
});
