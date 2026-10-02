const fs = require("node:fs");
const { cases, sequences } = JSON.parse(fs.readFileSync(0, "utf8"));
function sourceCommand(clock, input, command, timer) {
  const probe = new Probe();
  const base = input.workspace.base;
  probe.state = {
    timer, selectedPhase: input.selection.phase,
    durationsMs: base.durationsMs, deviceId: input.allocation.deviceId
  };
  probe.host = { crypto: { randomUUID: () => input.identities.timerUuid } };
  const now = Date.parse(input.clock.observedAt);
  const monotonic = input.clock.monotonicNowMs ?? null;
  probe.use = {
    phaseConfig: () => base.durationsMs,
    trustedNow: () => now,
    elapsedFor: (active, at) => clock.elapsedFor(active, at, monotonic),
    selectedTaskIdForNextFocus: () => base.selectedTaskId,
    tr: (_key, _args, fallback) => fallback
  };
  if (!command) {
    const view = new ViewProbe();
    view.use = { elapsedFor: (active) => clock.elapsedFor(active, now, monotonic) };
    const display = view.timerDisplayView(timer, timer.status);
    return {
      elapsedMs: timer.plannedDurationMs - display.remaining,
      remainingMs: display.remaining, totalSeconds: display.totalSeconds,
      timeText: display.timeText
    };
  }
  const context = probe.timerCommandContext(input.intent.kind, {}, Date.parse(input.clock.physicalNow));
  return probe.buildTimerCommand(context, {
    id: input.identities.commandUuids[0], deviceSequence: command.deviceSequence,
    wallMs: command.hlcWallMs, counter: command.hlcCounter
  });
}
const ordinary = cases.map(({ input, command }) =>
  sourceCommand(new ClockProbe(), input, command, input.workspace.base.canonicalTimer));
const active = sequences.map((sequence) => {
  let clock = new ClockProbe();
  return sequence.map((step) => {
    if (step.resetClock) clock = new ClockProbe();
    return sourceCommand(clock, step.input, step.command, step.timer);
  });
});
process.stdout.write(JSON.stringify({ ordinary, active }));
