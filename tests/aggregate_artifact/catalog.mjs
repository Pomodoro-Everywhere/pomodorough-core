import { invalidCases } from "./cases.mjs";
import { bootstrapCases, intentCases, projectionCases, readModelCases } from "./workspace_cases.mjs";
import { completionMutationCases, completionStateCases } from "./completion_cases.mjs";
import { batchCases } from "./planning_cases.mjs";
import { clockCases } from "./clock_cases.mjs";
import { sentCases } from "./sent_cases.mjs";
import { queuedCases } from "./queued_cases.mjs";
import { fractionalReadCases, missingReadingCase, taskTotalCases } from "./read_cases.mjs";
import { savedCases } from "./batch_scenarios.mjs";
import { terminalCases } from "./terminal_cases.mjs";
import { retainedIntentCases } from "./terminal_provenance_cases.mjs";
import { displayCases } from "./pwa_display_cases.mjs";
import { displayMatrix } from "./pwa_display_matrix.mjs";
import { admissionCases } from "./pwa_display_admission.mjs";
import { ownershipCases } from "./pwa_ownership_cases.mjs";
import { legacyCases } from "./legacy_preferences_cases.mjs";
import { legacyNumericCases } from "./legacy_preferences_numeric_cases.mjs";
import { naturalCases } from "./pwa_natural_cases.mjs";
import { checkerCases, shapeCases, provenanceCases } from "./pwa_checker_cases.mjs";
import { dependencyCases } from "./legacy_dependencies_cases.mjs";
import { releaseCases } from "./pwa_release_cases.mjs";
import { selectionCases, selectionShapeCases } from "./pwa_selection_cases.mjs";
import { repairCases } from "./pwa_cycle_cases.mjs";

const overflowPaths = {
  "workspace.legacyDependencyPlan.v1": "workspace.local.commands.0.hlcCounter",
  "workspace.legacyPreferences.v1": "workspace.canonicalHead.counter",
  "workspace.ownershipPlan.v1": "clock.nowMs",
  "workspace.project.v1": "canonicalHead.counter",
  "workspace.readModel.v1": "source.value.base.durationsMs.focus",
  "workspace.intent.v1": "allocation.deviceSequence",
  "workspace.completionMutation.v1": "allocation.deviceSequence",
  "bootstrap.workspacePlan.v1": "remote.durationsMs.focus",
  "sync.batchPlan.v1": "queues.commands.0.hlcCounter",
  "timer.completionState.v1": "afterHistory.0.plannedDurationMs",
  "clock.observe.v1": "reading.wallSeconds",
  "reconcile.rebase.v3": "response.serverHlcCounter",
};

export function aggregateCases() {
  const cases = [...projectionCases(), ...readModelCases(), ...intentCases(),
    ...completionMutationCases(), ...bootstrapCases(), ...batchCases(),
    ...completionStateCases(), ...sentCases(), ...clockCases(), ...queuedCases(),
    ...fractionalReadCases(), missingReadingCase(), ...taskTotalCases(), ...savedCases(), ...terminalCases(),
    ...retainedIntentCases(), ...retainedIntentCases("workspace.project.v1"), ...displayCases(), ...displayMatrix(), ...admissionCases(), ...ownershipCases(), ...legacyCases(), ...legacyNumericCases(), ...naturalCases(),
    ...checkerCases(), ...shapeCases(), ...provenanceCases(), ...dependencyCases(), ...releaseCases(),
    ...selectionCases(), ...selectionShapeCases(), ...repairCases()];
  for (const [operation, path] of Object.entries(overflowPaths)) {
    cases.push(...invalidCases(cases.find((item) => item.operation === operation && item.ok), path));
  }
  return cases;
}
