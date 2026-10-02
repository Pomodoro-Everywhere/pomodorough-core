import { changed, fixture, vector } from "./cases.mjs";

export function projectionCases() {
  const source = fixture("workspace-projection-v1");
  const operation = "workspace.project.v1";
  const cases = source.cases.map((item) => vector(operation, item.name,
    { ...source.request, ...item.overrides }));
  for (const title of ["omitted", "empty"]) {
    const input = structuredClone(source.request);
    if (title === "omitted") delete input.local.taskOperations[0].title;
    cases.push(vector(operation, `raw-delete-${title}`, input));
  }
  for (const kind of ["finish", "cancel", "clear"]) {
    const input = structuredClone(source.request);
    const command = { ...input.local.commands[0], id: kind, type: kind,
      deviceSequence: 2, hlcCounter: 12, observedElapsedMs: 1000 };
    input.local.commands.push(command);
    input.neverSent.commands.push(kind);
    cases.push(vector(operation, `terminal-${kind}`, input));
  }
  cases.push(vector(operation, "natural-expiry",
    changed(source.request, "now", "2026-07-20T12:01:00Z")));
  const terminal = fixture("workspace-terminal-v1");
  const reopen = changed(terminal.request, "base.canonicalTimer", terminal.timer);
  reopen.base.history = [terminal.history];
  cases.push(vector(operation, "terminal-pair-reopen", reopen));
  cases.push(vector(operation, "terminal-pair-conflict",
    changed(reopen, "base.history.0.timerId", "conflicting-timer"), false));
  return cases;
}

export function readModelCases() {
  const source = fixture("read-model-v1");
  const cases = [];
  for (const profile of Object.keys(fixture("workspace-intent-v1").profiles)) {
    for (const timer of [null, source.timer]) {
      const input = changed(source.request, "source.value.base.canonicalTimer", timer);
      input.profile = profile;
      cases.push(vector("workspace.readModel.v1", `${profile}-${timer ? "running" : "idle"}`, input));
    }
  }
  return cases;
}

export function intentCases() {
  const source = fixture("workspace-intent-v1");
  const cases = [];
  for (const profile of Object.keys(source.profiles)) {
    const start = changed(source.request, "compatibility", profile);
    start.identities.commandUuids = start.identities.commandUuids.slice(0, 1);
    cases.push(vector("workspace.intent.v1", `${profile}-start`, start));
    const pause = changed(start, "workspace.base.canonicalTimer", source.timer);
    pause.intent = { kind: "pause" };
    pause.requestedTimer = source.timer;
    cases.push(vector("workspace.intent.v1", `${profile}-pause`, pause));
    cases.push(vector("workspace.intent.v1", `${profile}-stale-noop`,
      changed(pause, "requestedTimer.id", "stale-timer")));
  }
  return cases;
}

export function bootstrapCases() {
  const source = fixture("bootstrap-workspace-v1");
  const cases = [];
  for (const profile of ["appleWorkspace", "androidRepository", "desktopStorage", "pwaStorage"]) {
    for (const item of source.cases) {
      const input = item.path.length
        ? changed(source.request, item.path.join("."), item.value)
        : structuredClone(source.request);
      input.profile = profile;
      cases.push(vector("bootstrap.workspacePlan.v1", `${profile}-${item.name}`, input));
    }
  }
  return cases;
}
