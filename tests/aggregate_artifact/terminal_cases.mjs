import { changed, fixture, vector } from "./cases.mjs";
import { branch } from "./semantics.mjs";

export function terminalRequest() {
  const source = fixture("reconciliation-terminal-v3");
  return { local: source.local, sent: structuredClone(source.local),
    response: JSON.parse(source.http.responseRaw), neverSent: source.neverSent,
    timerDependencies: source.timerDependencies };
}

function pairRequest(item) {
  const source = fixture("workspace-terminal-v1");
  const input = terminalRequest();
  input.local = structuredClone(source.request.local);
  input.sent = structuredClone(input.local);
  input.response.acknowledgements = [];
  input.response.serverTime = "2026-07-20T12:00:00Z";
  input.response.serverHlcWallMs = 1784548800000;
  input.response.canonicalTimer = item.cleared ? null : { ...source.timer, ...item.timerOverrides };
  input.response.history = item.missingHistory ? [] : [{ ...source.history, ...item.historyOverrides }];
  if (item.commands) {
    input.local.commands = [source.command];
    if (item.safe) input.neverSent.commands = [source.command.id];
  }
  if (item.retainedCommands) input.local.commands = item.retainedCommands;
  if (item.sameTimeSibling) input.response.history.push({ ...source.history,
    id: "aaa-sibling", timerId: "aaa-sibling", commandId: "finish-sibling" });
  return input;
}

function evidenceCase(name, input) {
  const equals = { schemaVersion: 3 };
  if (input.response.canonicalTimer?.lastIntent?.deviceId) {
    equals["timer.lastIntent.deviceId"] = input.response.canonicalTimer.lastIntent.deviceId;
  }
  return branch("reconcile.rebase.v3", name, input, "terminalRebase", {
    equals,
    same: { canonicalResponse: "response", baseTimer: "response.canonicalTimer",
      baseHistory: "response.history", baseTasks: "response.tasks" },
  });
}

function partialProofCase() {
  const input = terminalRequest();
  const operation = { id: "frozen-task", deviceId: "retained-device", type: "delete", taskId: "deleted-task",
    occurredAt: "2026-10-02T12:02:58Z", hlcWallMs: 1790942578000, hlcCounter: 0, extension: [null, "", {}] };
  input.local.taskOperations = [operation, { ...operation, id: "safe-task", hlcCounter: 1, title: "" }];
  input.neverSent.taskOperations = ["safe-task"];
  return branch("reconcile.rebase.v3", "partial-proof-raw-retained", input, "terminalPartial", {
    equals: { "projectionPending.taskOperations": [], "timer.status": "completed" },
    same: { pendingTaskOperations: "local.taskOperations", canonicalResponse: "response" },
  });
}

export function terminalCases() {
  const source = fixture("workspace-terminal-v1");
  const cases = source.cases.map((item) => evidenceCase(item.name, pairRequest(item)));
  cases.push(evidenceCase("actual-http-200-finish", terminalRequest()));
  const metadata = terminalRequest();
  metadata.response.canonicalTimer.lastIntent.deviceId = "origin-device";
  metadata.response.canonicalTimer.extension = { null: null, empty: "" };
  metadata.response.history[0].extension = [false, {}];
  cases.push(evidenceCase("raw-native-metadata", metadata), partialProofCase());
  cases.push(...source.rejections.map((item) => vector("reconcile.rebase.v3", `conflict-${item.name}`,
    pairRequest(item), false)));
  const input = terminalRequest();
  cases.push(vector("reconcile.rebase.v3", "missing-ack", changed(input, "response.acknowledgements", []), false));
  cases.push(vector("reconcile.rebase.v3", "bad-native-device",
    changed(input, "response.canonicalTimer.lastIntent.deviceId", false), false));
  return cases;
}
