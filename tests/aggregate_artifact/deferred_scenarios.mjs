import { fixture } from "./cases.mjs";
import { branch } from "./semantics.mjs";
import { generatedRequest, continueRequest } from "./generated_scenarios.mjs";

function opportunity(input, result) {
  const next = continueRequest(input, result, "2026-07-20T12:01:00Z", "019f7f66-a060-7000-8000-000000000010");
  delete next.requestedTimer;
  next.stage = "deferredBreakOpportunity";
  next.identities.timerUuid = input.identities.timerUuid;
  next.lifecycle = result.lifecycle;
  next.event = { kind: "opportunity" };
  next.centralizedSession = { userId: null, authenticated: false };
  if (next.replicationMode === "iroh") {
    next.previousWorkspace = next.workspace;
    next.previousObservation = next.observation;
  }
  return next;
}

function desktopDeferred(call) {
  const input = generatedRequest("desktopStorage", 0);
  const first = call(branch("workspace.completionMutation.v1", "desktop-deferred-finish", input, "desktopDeferred", {
    equals: { "commands.*.type": ["finish"] }, lengths: { "lifecycle.pendingBreaks": 1 } }));
  const next = opportunity(input, first);
  const blocked = structuredClone(next);
  blocked.centralizedSession = { userId: "user", authenticated: true };
  call(branch("workspace.completionMutation.v1", "desktop-deferred-barrier", blocked, "desktopDeferred", {
    equals: { reason: "canonicalBarrier" }, lengths: { commands: 0 }, same: { workspace: "workspace", allocation: "allocation" } }));
  const started = call(branch("workspace.completionMutation.v1", "desktop-deferred-start", next, "desktopDeferred", {
    equals: { "commands.*.type": ["start"], sourceStatus: "pending" },
    lengths: { "lifecycle.pendingBreaks": 0, "workspace.timerDependencies": 1 } }));
  const retry = opportunity(input, started);
  retry.identities.commandUuids = [];
  call(branch("workspace.completionMutation.v1", "desktop-deferred-repeat", retry, "desktopDeferred", {
    equals: { reason: "noPendingBreak" }, lengths: { commands: 0 }, same: { workspace: "workspace" } }));
}

function appleExplicit(call) {
  const input = generatedRequest("appleWorkspace", 0);
  input.replicationMode = "iroh";
  input.selection = { phase: "long_break", generation: "5", explicit: true };
  const first = call(branch("workspace.completionMutation.v1", "apple-explicit-finish", input, "appleExplicit", {
    equals: { "commands.*.type": ["finish"] }, lengths: { "lifecycle.pendingBreaks": 1 }, same: { selection: "selection" } }));
  const next = opportunity(input, first);
  const started = call(branch("workspace.completionMutation.v1", "apple-explicit-start", next, "appleExplicit", {
    equals: { "commands.*.type": ["start"], "commands.0.phase": "short_break" },
    lengths: { "workspace.timerDependencies": 0 }, same: { selection: "selection" } }));
  const retry = opportunity(input, started);
  retry.identities.commandUuids = [];
  call(branch("workspace.completionMutation.v1", "apple-explicit-repeat", retry, "appleExplicit", {
    lengths: { commands: 0 }, equals: { reason: "noPendingBreak" }, same: { selection: "selection" } }));
  const expiry = fixture("completion-lifecycle-request-v1");
  expiry.selection = input.selection;
  call(branch("workspace.completionMutation.v1", "apple-explicit-expiry", expiry, "appleExplicit", {
    equals: { "commands.*.type": ["start"], "commands.0.phase": "short_break" }, same: { selection: "selection" } }));
}

export function deferredScenarios(call) {
  desktopDeferred(call);
  appleExplicit(call);
}
