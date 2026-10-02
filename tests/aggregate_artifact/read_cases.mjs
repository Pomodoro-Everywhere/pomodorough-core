import { fixture, changed } from "./cases.mjs";
import { branch } from "./semantics.mjs";

export function fractionalReadCases() {
  const source = fixture("read-model-v1");
  const input = changed(source.request, "profile", "pwaStorage");
  input.source.value.base.canonicalTimer = source.timer;
  input.monotonic = { nowMs: 5000.375, continuityId: "session", anchor: {
    timerId: "timer-one", anchorAt: "2026-03-08T07:29:00Z", elapsedAtAnchorMs: 120000,
    sampledTrustedNowMs: 1772955000000, sampledMonotonicMs: 4999.875, continuityId: "session" } };
  return [["fractional-live", input, 180000.5, 180001],
    ["fractional-discontinuity", changed(input, "monotonic.continuityId", "new-session"), 180000, 180000],
    ["fractional-no-observation", changed(input, "monotonic", null), 180000, 180000]]
    .map(([name, request, elapsed, observed]) => branch("workspace.readModel.v1", name, request,
      "fractionalRead", { equals: { "canonical.status": "running", "canonical.elapsedMs": elapsed,
        "canonical.observedElapsedMs": observed, "canonical.remainingMs": 1500000 - elapsed } }));
}

export function missingReadingCase() {
  const input = fixture("workspace-intent-v1").request;
  input.compatibility = "pwaStorage";
  input.intent = { kind: "selectPhase", phase: "focus" };
  input.workspace.base.canonicalTimer = fixture("workspace-intent-v1").timer;
  input.observation.monotonicAnchor = { timerId: "existing-timer", anchorAt: "2026-07-20T12:00:00Z",
    elapsedAtAnchorMs: 5000, sampledTrustedNowMs: 1784548810000,
    sampledMonotonicMs: 100.125, continuityId: "page" };
  input.clock.continuityId = "page";
  return branch("workspace.intent.v1", "pwa-missing-reading", input, "missingReading", {
    lengths: { commands: 0 }, same: { "observation.monotonicAnchor": "observation.monotonicAnchor" },
    equals: { "projection.canonicalTimer.status": "running" } });
}

export function taskTotalCases() {
  const source = fixture("read-model-v1");
  const task = fixture("workspace-intent-desktop-known-tasks-v1").knownTasks[0];
  const input = source.request;
  input.source.value.base.tasks = [task];
  input.source.value.base.selectedTaskId = task.id;
  input.source.value.base.history = source.checkerHistory.map((row) =>
    ({ ...row, taskId: row.taskId === "current" ? task.id : row.taskId }));
  const totals = branch("workspace.readModel.v1", "populated-task-totals", input, "taskTotals", {
    equals: { "tasks.total": 1, "cadence.completedFocusToday": 3,
      "cadence.completedFocusTodayPlannedDurationMs": 1320000,
      "tasks.completedFocusTodayByTask": { [task.id]: { count: 1, plannedDurationMs: 900000 } } } });
  const boundaries = [
    ["23-hour-day", "2026-03-08T05:00:00Z", "2026-03-09T04:00:00Z", "2026-03-08T07:30:00Z"],
    ["25-hour-day", "2026-11-01T04:00:00Z", "2026-11-02T05:00:00Z", "2026-11-02T04:59:59Z"],
  ].map(([name, start, end, observedAt]) => {
    const request = structuredClone(input);
    Object.assign(request, { observedAt, calendarIntervals: [{ start, end }] });
    request.source.value.base.history = [start, end].map((completedAt, i) => ({
      id: `boundary-${i}`, timerId: `boundary-${i}`, taskId: task.id,
      phase: "focus", status: "completed", plannedDurationMs: 60000, completedAt }));
    return branch("workspace.readModel.v1", name, request, "taskTotals", { equals: {
      "cadence.completedFocusToday": 1, "cadence.completedFocusTotal": 2,
      "tasks.completedFocusTodayByTask": { [task.id]: { count: 1, plannedDurationMs: 60000 } } } });
  });
  return [totals, ...boundaries];
}
