"use strict";

// Replay Core completion requests through the composed PWA: seed IndexedDB
// with the request's records, point the browser clocks at its clock, then run
// the production completion path and read back what IndexedDB kept.
const path = require("node:path");
const { createPwa, setClock, seedWorkspace, readMeta, main } = require("./pwa_probe_runtime.cjs");

const { TIMING_MS } = require(path.resolve(__dirname, "../../server/web/app-runtime.js"));
const COMPLETION = "workspace.completionMutation.v1";
const AFTER_KEYS = ["timerOwner", "settings", "deviceSequence", "hlc", "uuidV7"];
const SENT_KEYS = ["stage", "localTabId", "leaseNowMs", "leaseDurationMs", "ownership", "requestedTimer"];
const ALERTS = { updateTimerCompletion: () => {}, startCompletionAlert: () => {},
  activeCompletionAlertTimerId: () => null };

// wallMs moves only the tab's wall clock, which the PWA uses as its lease
// clock; the Core request clock still names the completion instant.
async function seeded(input, wallMs, overrides = ALERTS) {
  const pwa = createPwa(overrides);
  pwa.clock.timerUuid = input.identities.timerUuid;
  setClock(pwa, wallMs == null ? input.clock : { physicalNow: new Date(wallMs).toISOString() });
  const database = await seedWorkspace(pwa, input);
  return { pwa, database };
}

function sentRequest(pwa) {
  const sent = pwa.log.filter((entry) => entry.operation === COMPLETION).map((entry) => entry.input);
  if (sent.length !== 1) throw new Error(`expected one completion request, saw ${sent.length}`);
  return Object.fromEntries(SENT_KEYS.map((key) => [key, sent[0][key] ?? null]));
}

// TimerLifecycle.finishTimer presents a copy of the tab's displayed timer.
// presentRequested replays a copy taken before IndexedDB moved under the tab.
async function complete({ input, wallMs, presentRequested }) {
  const { pwa, database } = await seeded(input, wallMs);
  const presented = presentRequested ? input.requestedTimer : structuredClone(pwa.state.timer);
  const plan = await pwa.call("persistWorkspaceCompletion", input.stage, presented);
  const after = await readMeta(database, AFTER_KEYS);
  return { outcome: plan.outcome, reason: plan.reason ?? "",
    ...(Object.hasOwn(plan, "retryAtMs") ? { retryAtMs: plan.retryAtMs } : {}),
    commands: plan.commands, selection: plan.selection ?? null, after, sent: sentRequest(pwa) };
}

// Automatic finish through TimerLifecycle.finishTimer: a denied tab keeps the
// timer and schedules its retry from Core's lease answer.
async function retry({ input, nowMs }) {
  const { pwa } = await seeded(input);
  const delays = [];
  pwa.host.setTimeout = (_callback, delay) => { delays.push(delay); return delays.length; };
  const saved = await pwa.call("finishTimer", true);
  const delay = (outcome) => pwa.call("completionRetryDelay", outcome, nowMs);
  return { saved, delays, sent: sentRequest(pwa), heartbeatMs: pwa.call("timerOwnerHeartbeatMs"),
    leaseMs: TIMING_MS.timerOwnerLease, missing: delay({ reason: "not_owner" }),
    nullLease: delay({ reason: "not_owner", retryAtMs: null }),
    camelCase: delay({ reason: "notOwner" }), none: delay({ reason: "stale" }) };
}

// The expiry gate in TimerLifecycle.updateTimerCompletion decides whether a
// render starts an automatic finish; count the completions it starts.
async function expiry({ input, status, remaining }) {
  const stages = [];
  const { pwa } = await seeded(input, null, { startCompletionAlert: () => {},
    activeCompletionAlertTimerId: () => null,
    persistWorkspaceCompletion: async (stage) => { stages.push(stage); return { outcome: "noop", reason: "notExpired", commands: [] }; } });
  pwa.call("updateTimerCompletion", pwa.state.timer, status, remaining, false);
  await new Promise((resolve) => setImmediate(resolve));
  return stages;
}

// A manual finish of the break an automatic finish generated: IndexedDB must
// hand Core the dependency the first commit kept, so Core can chain the
// follow-up finish to that break's start.
async function followup({ input, nextAtMs }) {
  const { pwa, database } = await seeded(input);
  const first = await pwa.call("persistWorkspaceCompletion", input.stage, structuredClone(pwa.state.timer));
  setClock(pwa, { physicalNow: new Date(nextAtMs).toISOString() });
  pwa.log.length = 0;
  const presented = structuredClone(pwa.state.timer);
  const next = await pwa.call("persistWorkspaceCompletion", "finishCommit", presented);
  const after = await readMeta(database, ["timerDependencies"]);
  return { first: first.commands, presented, sent: sentRequest(pwa), next: { outcome: next.outcome,
    reason: next.reason ?? "", commands: next.commands }, after };
}

main({ complete, retry, expiry, followup });
