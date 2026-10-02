import assert from "node:assert/strict";
import { aggregateCases } from "./catalog.mjs";
import { nativeResponses, validateEnvelopes } from "./native_oracle.mjs";
import { assertCoverage, assertSemantics } from "./semantics.mjs";
import { generatedScenarios } from "./generated_scenarios.mjs";
import { deferredScenarios } from "./deferred_scenarios.mjs";
import { batchScenarios } from "./batch_scenarios.mjs";

export function runCorpus(staticCases, dispatch) {
  const cases = [];
  const expected = [];
  const call = (item) => {
    const raw = dispatch(item);
    const envelope = JSON.parse(raw);
    assert.equal(envelope.ok, item.ok, `${item.name}: ${envelope.error ?? "unexpected success"}`);
    if (envelope.ok) assertSemantics(item, envelope.value);
    cases.push(item);
    expected.push(raw);
    return envelope.value;
  };
  staticCases.forEach(call);
  generatedScenarios(call);
  deferredScenarios(call);
  batchScenarios(call);
  validateEnvelopes(cases, expected.join("\n"));
  const hits = assertCoverage(cases);
  return { staticCases, cases, expected, hits };
}

export function nativeCorpus() {
  const staticCases = aggregateCases();
  const initial = nativeResponses(staticCases);
  let cursor = 0;
  return runCorpus(staticCases, (item) => cursor < initial.length
    ? initial[cursor++] : nativeResponses([item])[0]);
}
