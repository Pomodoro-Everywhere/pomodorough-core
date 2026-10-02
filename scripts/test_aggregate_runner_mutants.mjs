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
