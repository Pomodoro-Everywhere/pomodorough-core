const fs = require('node:fs');
const path = require('node:path');
const root = path.resolve(__dirname, '../..');
const state = fs.readFileSync(path.join(root, 'server/web/app-state.js'), 'utf8');
const actions = fs.readFileSync(path.join(root, 'server/web/app-actions.js'), 'utf8');
const view = fs.readFileSync(path.join(root, 'server/web/app-view.js'), 'utf8');
const fixture = JSON.parse(fs.readFileSync(path.join(__dirname, '../fixtures/read-model-v1.json'), 'utf8'));

function method(source, name, scope) {
  const start = source.indexOf(`    ${name}(`);
  if (start < 0) throw new Error(`missing production method: ${name}`);
  const end = source.indexOf('\n    }', start);
  if (end < 0) throw new Error(`unterminated production method: ${name}`);
  return Function(...Object.keys(scope), `return function ${source.slice(start, end + 6).trim()}`)(...Object.values(scope));
}

const clampNumber = (value, min, max) => Math.max(min, Math.min(max, value));
const positiveNumber = (value, fallback) => Number(value) > 0 ? Number(value) : fallback;
const elapsedFor = method(state, 'elapsedFor', { clampNumber, positiveNumber });
const clock = { trustedNow: () => Date.parse(fixture.request.observedAt), monotonicNow: () => null,
  elapsedMonotonicAnchor: null, elapsedFor };
const viewRead = method(view, 'timerDisplayView', {});
const historyDateMs = method(actions, 'historyDateMs', {});
const dayCount = method(actions, 'completedFocusCountForDay', {});
const progress = method(actions, 'longBreakProgress', {});
const renderControls = method(view, 'renderTimerControls', {});
class FixedDate extends Date {
  constructor(...args) { super(...(args.length ? args : [Date.parse(fixture.request.observedAt)])); }
}
const taskSummaries = method(view, 'taskSummariesToday', { Date: FixedDate });
const effectiveHistoryTaskId = method(view, 'effectiveHistoryTaskId', {});
const timer = fixture.timer;
const elapsed = clock.elapsedFor(timer);
const readout = viewRead.call({ use: { elapsedFor: (value) => clock.elapsedFor(value) } }, timer, timer.status);
const counts = fixture.cadence.map(({ count }) => {
  const history = Array.from({ length: count }, () => ({ phase: 'focus', status: 'completed', completedAt: '2026-03-08T05:00:00Z' }));
  history.push({ phase: 'focus', status: 'completed', completedAt: '2026-03-08T04:59:59.999Z' });
  history.push({ phase: 'focus', status: 'completed', completedAt: '2026-03-09T04:00:00Z' });
  const owner = { historyDateMs };
  return { count: dayCount.call(owner, history, new Date(fixture.request.observedAt)),
    progress: progress(count) };
});
const readiness = ['running', 'paused', 'completed'].flatMap((status) => [false, true].map((blocked) => {
  const elements = { timerToggle: {}, finishButton: {}, cancelButton: {}, clearButton: {} };
  const owner = { elements, use: { controlsBlocked: () => blocked,
    activeCompletionAlertTimerId: () => null, updateTimerCompletion: () => {} } };
  renderControls.call(owner, timer, { status, remaining: 1000 });
  return { status, blocked, toggle: !elements.timerToggle.disabled,
    finish: !elements.finishButton.disabled, cancel: !elements.cancelButton.disabled,
    clear: !elements.clearButton.disabled };
}));
const taskRows = taskSummaries.call({ state: { history: fixture.checkerHistory },
  use: { historyDateMs, positiveNumber }, effectiveHistoryTaskId });
process.stdout.write(JSON.stringify({ elapsed, remaining: readout.remaining, progress: readout.progress,
  counts, readiness, tasks: { current: taskRows.get('current'), removed: taskRows.get('removed'),
    unassigned: taskRows.has(null) } }));
