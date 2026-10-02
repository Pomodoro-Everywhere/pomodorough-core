import { changed, fixture, vector } from "./cases.mjs";

export function finishRequest(profile, status = "running") {
  const source = fixture("workspace-intent-v1");
  const input = changed(source.request, "compatibility", profile);
  delete input.intent;
  input.stage = "finishCommit";
  input.selection.explicit = false;
  input.identities.timerUuid = null;
  input.identities.commandUuids = input.identities.commandUuids.slice(0, 1);
  input.ownership = null;
  input.workspace.base.canonicalTimer = { ...source.timer, status };
  input.requestedTimer = input.workspace.base.canonicalTimer;
  return input;
}

function automaticRequest(item) {
  const input = finishRequest(item.profile);
  input.stage = "automaticFinishCommit";
  input.clock = Object.fromEntries(["occurredAt", "physicalNow", "observedAt"]
    .map((field) => [field, "2026-07-20T12:01:00Z"]));
  input.identities.commandUuids = ["019f7f66-a060-7000-8000-000000000001"];
  input.ownership = item.ownerDeviceId === null ? null
    : { timerId: "existing-timer", deviceId: item.ownerDeviceId };
  if (item.profile === "pwaStorage") {
    input.localTabId = "tab-local";
    input.leaseNowMs = item.leaseNowMs;
    if (input.ownership) {
      input.ownership.tabId = item.ownerTabId;
      input.ownership.leaseExpiresAtMs = item.leaseExpiresAtMs;
    }
  }
  return input;
}

export function completionMutationCases() {
  const source = fixture("completion-mutation-v1");
  const operation = "workspace.completionMutation.v1";
  const cases = source.profiles.flatMap((profile) => source.states.map((status) =>
    vector(operation, `${profile}-${status}-finish`, finishRequest(profile, status))));
  for (const item of source.automaticCases) {
    cases.push(vector(operation, item.name, automaticRequest(item)));
  }
  for (const profile of ["appleWorkspace", "androidCoordinator", "desktopStorage", "desktopTerminal"]) {
    const input = changed(fixture("completion-lifecycle-request-v1"), "compatibility", profile);
    cases.push(vector(operation, `${profile}-expiry-lifecycle`, input));
    const before = changed(input, "clock.physicalNow", "2026-07-20T12:00:54.999Z");
    before.clock.observedAt = before.clock.physicalNow;
    cases.push(vector(operation, `${profile}-before-expiry`, before));
  }
  return cases;
}

function completedHistory(id) {
  return { id: `history-${id}`, timerId: id, commandId: `finish-${id}`,
    phase: id === "z" ? "long_break" : "focus", status: "completed",
    plannedDurationMs: 60000, completedAt: "2026-08-25T12:00:00.000Z" };
}

export function completionStateCases() {
  return fixture("completion-state-v1").cases.map((item) => vector("timer.completionState.v1", item.name, {
    kind: "install", compatibility: item.compatibility,
    beforeHistory: item.before.map(completedHistory), afterHistory: item.after.map(completedHistory),
    canonicalTimer: null, selection: { phase: item.selected, generation: "0", explicit: item.explicit },
    pending: { commandIds: [], sendableCommandIds: [], otherOperationIds: [] },
    advances: [], acknowledgements: [], discardedCommandIds: [], referenceTime: "2026-08-25T12:00:00.000Z",
    calendarIntervals: [{ start: "2026-08-25T00:00:00Z", end: "2026-08-26T00:00:00Z" }],
  }));
}
