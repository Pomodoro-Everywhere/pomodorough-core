import { changed, fixture, vector } from "./cases.mjs";
import { branch } from "./semantics.mjs";
import { context, emptyQueues, fields, rawWorkspace } from "./pwa_display_cases.mjs";
import { terminalRequest } from "./terminal_cases.mjs";

export function displayMatrix() {
  const cases = [];
  for (const head of [null, { wallMs: 1788177600000, counter: 2 }, { wallMs: 1788177600000, counter: 3 }, { wallMs: 1788177600000, counter: 10 }]) {
    for (const proof of [[], ["fresh"], ["01a057b0-ce00-7000-8000-000000000015", "fresh"]]) {
      const workspace = rawWorkspace();
      const first = workspace.local.commands[0];
      workspace.local.commands.push({ ...first, id: "fresh", type: "pause", hlcCounter: 4, deviceSequence: 9 });
      workspace.canonicalHead = head;
      workspace.neverSent.commands = proof;
      const fresh = proof.includes("fresh") && (!head || head.counter < 4);
      const safe = head && head.counter < 3 && proof.length === 2;
      cases.push(branch("workspace.project.v1", `pwa-matrix-${head?.counter ?? "null"}-${proof.length}`, { ...workspace, now: first.occurredAt }, "pwaMatrix", {
        equals: { "workspace.canonicalTimer.status": fresh ? "paused" : "running",
          "projectionPending.commands": safe ? workspace.local.commands : [],
          "displayContext.projectionPending.commands": fresh ? workspace.local.commands : workspace.displayContext.projectionPending.commands } }));
    }
  }
  for (const mode of ["absent", "null-records", "empty-records"]) {
    const workspace = rawWorkspace();
    if (mode === "absent") delete workspace.displayContext;
    else workspace.displayContext = context(mode === "null-records" ? null : emptyQueues());
    const equals = { "projectionPending.commands": [] };
    if (mode === "null-records") equals["workspace.canonicalTimer.status"] = "running";
    else equals["workspace.canonicalTimer"] = null;
    cases.push(branch("workspace.project.v1", `pwa-context-${mode}`, { ...workspace, now: "2026-08-31T12:00:01Z" }, "pwaMatrix", { equals }));
  }
  cases.push(...ledgerNegatives());
  return cases;
}

function ledgerNegatives() {
  const cases = [];
  const workspace = { ...rawWorkspace(), now: "2026-08-31T12:00:01Z" };
  const operation = fixture("workspace-projection-v1").request.local;
  for (const [domain, field, value] of [["commands", "occurredAt", "bad"], ["commands", "hlcWallMs", 9007199254740992],
    ["taskOperations", "taskId", ""], ["durationOperations", "durationMs", 1],
    ["autoStartOperations", "enabled", "bad"], ["selectedTaskOperations", "taskId", ""]]) {
    const input = structuredClone(workspace);
    if (domain !== "commands") input.local[domain] = [operation[domain][0]];
    input.local[domain][0][field] = value;
    input.displayContext = context(emptyQueues());
    cases.push(vector("workspace.project.v1", `pwa-hidden-corrupt-${domain}-${field}`, input, false));
  }
  const stale = structuredClone(workspace);
  stale.local.commands = [];
  cases.push(vector("workspace.project.v1", "pwa-stale-stored-record", stale, false, "retained payloads"));
  cases.push(vector("workspace.project.v1", "pwa-missing-dependency", { ...workspace,
    timerDependencies: [{ operationId: workspace.local.commands[0].id, dependsOnOperationId: "unknown" }] }, false));
  cases.push(vector("workspace.project.v1", "pwa-foreign-proof", changed(workspace, "neverSent.commands", ["foreign"]), false));
  const extensions = structuredClone(workspace);
  extensions.local.commands[0].extension = { empty: "", null: null, nested: [false, {}] };
  extensions.displayContext.projectionPending.commands[0] = structuredClone(extensions.local.commands[0]);
  cases.push(branch("workspace.project.v1", "pwa-exact-extensions", extensions, "pwaMatrix", {
    same: { "displayContext.projectionPending": "displayContext.projectionPending" }, equals: { "projectionPending.commands": [] } }));
  cases.push(vector("workspace.project.v1", "pwa-deleted-extension", changed(extensions, "displayContext.projectionPending.commands.0.extension", null), false));
  const rebase = terminalRequest();
  rebase.displayContext = context(rebase.local);
  cases.push(branch("reconcile.rebase.v3", "pwa-actual-http-ack-trim", rebase, "pwaMatrix", {
    equals: { "displayContext.projectionPending": emptyQueues(), pending: [] }, same: { canonicalResponse: "response", baseTimer: "response.canonicalTimer" } }));
  for (const [name, displayContext] of [["unknown", context({ ...rebase.local, extra: true })],
    ["rewritten", context(changed(rebase.local, "commands.0.observedElapsedMs", 0))], ["null-wrapper", null]]) {
    cases.push(vector("reconcile.rebase.v3", `pwa-v3-invalid-${name}`, { ...rebase, displayContext }, false));
  }
  const removed = terminalRequest();
  const actual = rawWorkspace();
  for (const name of fields) removed.response[name] = actual.base[name];
  Object.assign(removed.response, { serverTime: "2026-08-31T12:00:01Z", serverHlcWallMs: 1788177601000, serverHlcCounter: 0,
    acknowledgements: [{ commandId: actual.local.commands[0].id, outcome: "rejected", reason: "stale" }] });
  Object.assign(removed, { local: actual.local, sent: actual.local, neverSent: {}, timerDependencies: [], displayContext: actual.displayContext });
  cases.push(branch("reconcile.rebase.v3", "pwa-start-removed-no-synthetic-timer", removed, "pwaMatrix", {
    equals: { pending: [], timer: null, "displayContext.projectionPending": emptyQueues() }, same: { canonicalResponse: "response" } }));
  return cases;
}
