"use strict";

// Replay each Core intent request through the PWA: seed IndexedDB with the
// request's records, point the browser clocks at its clock, then either issue
// the intent through persistCommand or read the timer view.
const { createPwa, setClock, seedWorkspace, main } = require("./pwa_probe_runtime.cjs");

function reading(pwa) {
  const timer = pwa.call("displayTimer");
  const status = pwa.state.readModel?.display.status || pwa.state.timer.status;
  const view = pwa.call("timerDisplayView", timer, status);
  return { elapsedMs: timer.plannedDurationMs - view.remaining, remainingMs: view.remaining,
    totalSeconds: view.totalSeconds, timeText: view.timeText };
}

async function seeded(input) {
  const pwa = createPwa({ updateTimerCompletion: () => {}, startCompletionAlert: () => {},
    activeCompletionAlertTimerId: () => null });
  pwa.clock.timerUuid = input.identities.timerUuid;
  setClock(pwa, input.clock);
  await seedWorkspace(pwa, input);
  return pwa;
}

async function step({ input, read }) {
  const pwa = await seeded(input);
  return read ? reading(pwa) : pwa.call("persistCommand", input.intent.kind);
}

// Retarget the displayed timer through persistRetargetOperation and report the
// intent request it handed Core with the command it kept.
async function retarget({ input }) {
  const pwa = await seeded(input);
  const command = await pwa.call("persistRetargetOperation", pwa.state.timer.id, input.intent.taskId);
  const sent = pwa.log.filter((entry) => entry.operation === "workspace.intent.v1").map((entry) => entry.input);
  if (sent.length !== 1) throw new Error(`expected one intent request, saw ${sent.length}`);
  return { command, intent: sent[0].intent, requestedTimer: sent[0].requestedTimer ?? null };
}

main({ step, retarget });
