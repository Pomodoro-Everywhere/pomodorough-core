import { fixture, vector } from "./cases.mjs";
import { branch } from "./semantics.mjs";
import { terminalRequest } from "./terminal_cases.mjs";

export function retainedIntentRequest(item) {
  const input = terminalRequest();
  input.response.history = [];
  Object.assign(input.local.commands[0], item.commandOverrides);
  input.sent = structuredClone(input.local);
  if (!item.acknowledged) {
    input.sent.commands = [];
    input.response.acknowledgements = [];
  }
  if (item.extensions) {
    input.local.commands[0].extension = { empty: "", null: null, nested: [false, {}] };
    input.response.canonicalTimer.extension = { kept: true };
    Object.assign(input.response.canonicalTimer.lastIntent,
      { deviceId: "origin-device", extension: [null, "", {}] });
  }
  if (Object.hasOwn(item.commandOverrides ?? {}, "taskId")) {
    input.response.canonicalTimer.taskId = "12345678-1234-4234-8234-123456789010";
  }
  return input;
}

export function retainedIntentWorkspace(input) {
  const { response } = input;
  const fields = ["canonicalTimer", "history", "tasks", "durationsMs", "autoStartBreaks", "selectedTaskId"];
  return { base: Object.fromEntries(fields.map((field) => [field, response[field]])),
    local: input.local, neverSent: input.neverSent, timerDependencies: input.timerDependencies,
    canonicalHead: { wallMs: response.serverHlcWallMs, counter: response.serverHlcCounter }, now: response.serverTime };
}

function control(item, operation) {
  const request = retainedIntentRequest(item);
  const workspace = operation === "workspace.project.v1";
  const input = workspace ? retainedIntentWorkspace(request) : request;
  const timer = request.response.canonicalTimer;
  const equals = { "workspace.history.0.commandId": timer.lastIntent.commandId,
    "workspace.history.0.completedAt": timer.anchorAt, "workspace.history.0.endedAt": timer.anchorAt,
    "workspace.history.0.phase": timer.phase, "workspace.history.0.plannedDurationMs": timer.plannedDurationMs,
    "workspace.canonicalTimer.status": "completed", "projectionPending.commands": [] };
  if (!workspace) Object.assign(equals, { baseHistory: [], pending: item.acknowledged ? [] : request.local.commands });
  if (item.extensions) equals["workspace.canonicalTimer.lastIntent.deviceId"] = "origin-device";
  if (timer.taskId) equals["workspace.history.0.taskId"] = timer.taskId;
  return branch(operation, item.name, input, workspace ? "workspaceMissingHistory" : "terminalMissingHistory", {
    equals, lengths: { "workspace.history": 1 },
    same: workspace ? {} : { canonicalResponse: "response", baseTimer: "response.canonicalTimer" } });
}

export function retainedIntentCases(operation = "reconcile.rebase.v3") {
  const source = fixture("reconciliation-terminal-v3");
  const negatives = source.retainedIntentRejections.map((item) => {
    const request = retainedIntentRequest(item);
    return vector(operation, item.name, operation === "workspace.project.v1" ? retainedIntentWorkspace(request) : request,
      false, "conflicting workspace terminal timer/history");
  });
  return [...negatives, ...source.retainedIntentControls.map((item) => control(item, operation))];
}
