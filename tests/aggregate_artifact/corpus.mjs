import assert from "node:assert/strict";
import { aggregateCases } from "./catalog.mjs";
import { nativeResponses, validateEnvelopes } from "./native_oracle.mjs";
import { assertCoverage, assertSemantics } from "./semantics.mjs";
import { generatedScenarios } from "./generated_scenarios.mjs";
import { deferredScenarios } from "./deferred_scenarios.mjs";
import { batchScenarios } from "./batch_scenarios.mjs";
import { terminalScenarios } from "./terminal_scenarios.mjs";
import { displayScenarios } from "./pwa_display_scenarios.mjs";
import { admissionScenarios } from "./pwa_display_admission.mjs";
import { ownershipScenarios } from "./pwa_ownership_cases.mjs";
import { legacyScenarios } from "./legacy_preferences_cases.mjs";
import { naturalScenarios } from "./pwa_natural_cases.mjs";
import { dependencyScenarios } from "./legacy_dependencies_cases.mjs";
import { releaseScenarios } from "./pwa_release_cases.mjs";
import { selectionScenarios } from "./pwa_selection_cases.mjs";
import { assertSelectionPreservation } from "./pwa_selection_preservation.mjs";
import { cycleScenarios } from "./pwa_cycle_cases.mjs";

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
  terminalScenarios(call);
  displayScenarios(call);
  admissionScenarios(call);
  ownershipScenarios(call);
  legacyScenarios(call);
  naturalScenarios(call);
  dependencyScenarios(call);
  releaseScenarios(call);
  selectionScenarios(call);
  cycleScenarios(call);
  validateEnvelopes(cases, expected.join("\n"));
  const hits = assertCoverage(cases);
  assertSelectionPreservation(cases, expected);
  return { staticCases, cases, expected, hits };
}

export function nativeCorpus() {
  const staticCases = aggregateCases();
  const initial = nativeResponses(staticCases);
  let cursor = 0;
  return runCorpus(staticCases, (item) => cursor < initial.length
    ? initial[cursor++] : nativeResponses([item])[0]);
}
