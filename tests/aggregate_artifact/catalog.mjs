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

const overflowPaths = {
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
    ...retainedIntentCases(), ...retainedIntentCases("workspace.project.v1"), ...displayCases(), ...displayMatrix(), ...admissionCases(), ...ownershipCases()];
  for (const [operation, path] of Object.entries(overflowPaths)) {
    cases.push(...invalidCases(cases.find((item) => item.operation === operation && item.ok), path));
  }
  return cases;
}
