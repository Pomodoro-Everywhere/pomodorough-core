import { invalidCases } from "./cases.mjs";
import { bootstrapCases, intentCases, projectionCases, readModelCases } from "./workspace_cases.mjs";
import { completionMutationCases, completionStateCases } from "./completion_cases.mjs";
import { batchCases } from "./planning_cases.mjs";
import { clockCases } from "./clock_cases.mjs";
import { sentCases } from "./sent_cases.mjs";
import { queuedCases } from "./queued_cases.mjs";
import { fractionalReadCases, missingReadingCase, taskTotalCases } from "./read_cases.mjs";
import { savedCases } from "./batch_scenarios.mjs";

const overflowPaths = {
  "workspace.project.v1": "canonicalHead.counter",
  "workspace.readModel.v1": "source.value.base.durationsMs.focus",
  "workspace.intent.v1": "allocation.deviceSequence",
  "workspace.completionMutation.v1": "allocation.deviceSequence",
  "bootstrap.workspacePlan.v1": "remote.durationsMs.focus",
  "sync.batchPlan.v1": "queues.commands.0.hlcCounter",
  "timer.completionState.v1": "afterHistory.0.plannedDurationMs",
  "clock.observe.v1": "reading.wallSeconds",
};

export function aggregateCases() {
  const cases = [...projectionCases(), ...readModelCases(), ...intentCases(),
    ...completionMutationCases(), ...bootstrapCases(), ...batchCases(),
    ...completionStateCases(), ...sentCases(), ...clockCases(), ...queuedCases(),
    ...fractionalReadCases(), missingReadingCase(), ...taskTotalCases(), ...savedCases()];
  for (const [operation, path] of Object.entries(overflowPaths)) {
    cases.push(...invalidCases(cases.find((item) => item.operation === operation && item.ok), path));
  }
  return cases;
}
