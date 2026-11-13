"use strict";

// Run the PWA read path (IndexedDB reload, getWorkspaceReadModel, the timer
// view and the cadence actions) on each workspace the Python probe sends.
const fs = require("node:fs");
const path = require("node:path");
const { createPwa, setClock, seedWorkspace, main } = require("./pwa_probe_runtime.cjs");

const view = fs.readFileSync(path.resolve(__dirname, "../../server/web/app-view.js"), "utf8");

// ActivityView keeps taskSummariesToday private to renderTasks, so the probe
// runs the production method body against the composed runtime's read model.
function method(source, name) {
  const start = source.indexOf(`    ${name}(`);
  if (start < 0) throw new Error(`missing production method: ${name}`);
  const end = source.indexOf("\n    }", start);
  if (end < 0) throw new Error(`unterminated production method: ${name}`);
  return Function(`return function ${source.slice(start, end + 6).trim()}`)();
}

const taskSummariesToday = method(view, "taskSummariesToday");

function controls(pwa, timer, readout) {
  const { elements } = pwa;
  pwa.call("renderTimerControls", timer, readout);
  return { toggle: !elements.timerToggle.disabled, finish: !elements.finishButton.disabled,
    cancel: !elements.cancelButton.disabled, clear: !elements.clearButton.disabled,
    skip: !elements.skipButton.disabled };
}

function taskRows(pwa) {
  const rows = taskSummariesToday.call({ state: pwa.state,
    use: { getWorkspaceReadModel: () => pwa.call("getWorkspaceReadModel") } });
  return Object.fromEntries(rows);
}

async function read(item) {
  const pwa = createPwa({ updateTimerCompletion: () => {}, startCompletionAlert: () => {},
    activeCompletionAlertTimerId: () => (item.ringing ? item.workspace.base.canonicalTimer?.id ?? null : null) });
  setClock(pwa, { physicalNow: item.observedAt });
  await seedWorkspace(pwa, { workspace: item.workspace, selectedPhase: item.selectedPhase,
    allocation: { deviceId: "device-pwa", deviceSequence: 0, hlc: { wallMs: 0, counter: 0 }, lastUuid: null } });
  pwa.state.logoutRecoveryRequired = Boolean(item.blocked);
  const timer = pwa.call("displayTimer");
  const status = pwa.state.readModel?.display.status || pwa.state.timer.status;
  const readout = pwa.call("timerDisplayView", timer, status);
  return { elapsed: pwa.call("elapsedFor", pwa.state.timer), remaining: readout.remaining, progress: readout.progress,
    count: pwa.call("completedFocusCountForDay"), longBreakProgress: pwa.call("longBreakProgress"),
    controls: controls(pwa, timer, readout), tasks: taskRows(pwa) };
}

main({ read });
