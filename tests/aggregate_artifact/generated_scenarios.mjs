import { fixture } from "./cases.mjs";
import { finishRequest } from "./completion_cases.mjs";
import { branch } from "./semantics.mjs";
import { reconcileChildren } from "./reconciliation_scenarios.mjs";

export function generatedRequest(profile, prior) {
  const input = finishRequest(profile);
  const source = fixture("workspace-intent-v1").request;
  input.workspace.base.autoStartBreaks = true;
  input.identities = source.identities;
  input.ownership = { timerId: "existing-timer", deviceId: "device-local" };
  input.workspace.base.history = Array.from({ length: prior }, (_, i) => ({
    id: `past-${i}`, timerId: `past-timer-${i}`, commandId: `past-finish-${i}`,
    phase: "focus", status: "completed", plannedDurationMs: 60000, completedAt: "2026-07-20T11:00:00Z" }));
  if (profile === "pwaStorage") {
    input.localTabId = "tab-local";
    input.leaseNowMs = 1784548810000;
    input.leaseDurationMs = 30000;
    Object.assign(input.ownership, { tabId: "tab-local", leaseExpiresAtMs: 1784548870000 });
  }
  return input;
}

export function continueRequest(input, output, at, uuid) {
  const next = structuredClone(input);
  for (const field of ["workspace", "allocation", "observation", "selection"]) next[field] = output[field];
  next.requestedTimer = output.projection.canonicalTimer;
  next.clock = { occurredAt: at, physicalNow: at, observedAt: at };
  next.identities = { commandUuids: [uuid], timerUuid: null };
  return next;
}

function children(call, profile, input, first) {
  const prefix = `${profile}-generated`;
  const pause = continueRequest(fixture("workspace-intent-v1").request, first,
    "2026-07-20T12:00:11Z", "019f7f65-e0f8-7000-8000-000000000001");
  Object.assign(pause, { compatibility: profile, intent: { kind: "pause" } });
  const paused = call(branch("workspace.intent.v1", `${prefix}-pause`, pause, "childPause", {
    equals: { "commands.*.type": ["pause"], "workspace.timerDependencies.1.dependsOnOperationId": first.commands[1].id },
    lengths: { "workspace.timerDependencies": 2, commands: 1 } }));
  const finish = continueRequest(input, paused,
    "2026-07-20T12:00:12Z", "019f7f65-e4e0-7000-8000-000000000001");
  finish.ownership.timerId = first.commands[1].timerId;
  const finished = call(branch("workspace.completionMutation.v1", `${prefix}-child-finish`, finish, "childFinish", {
    equals: { "commands.*.type": ["finish"], "workspace.timerDependencies.2.dependsOnOperationId": paused.commands[0].id },
    lengths: { "workspace.timerDependencies": 3, "workspace.local.commands": 4, commands: 1 } }));
  reconcileChildren(call, profile, first, finished);
}

export function generatedScenarios(call) {
  for (const profile of ["appleWorkspace", "androidCoordinator", "pwaStorage"]) {
    for (const [prior, phase] of [[2, "short_break"], [3, "long_break"]]) {
      const input = generatedRequest(profile, prior);
      const first = call(branch("workspace.completionMutation.v1", `${profile}-generated-${phase}`, input, "generated", {
        equals: { outcome: "planned", "commands.*.type": ["finish", "start"], "commands.1.phase": phase,
          "workspace.timerDependencies.0.generatedBreak": true, "commandOutcomes.*.outcome": ["applied", "applied"] },
        lengths: { commands: 2, atomicCommandIds: 2, "workspace.timerDependencies": 1 } }));
      if (prior === 2) children(call, profile, input, first);
    }
  }
}
