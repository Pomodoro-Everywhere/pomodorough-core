
const fs = require("node:fs");
const sync = require(process.argv[2]);
const appState = require(process.argv[3]);
const rows = JSON.parse(fs.readFileSync(0, "utf8"));
const output = rows.map(row => {
  const raw = structuredClone(row.state);
  const state = { clockOffset: raw.clockOffset, hlcWallMs: raw.minimumWallMs || 0 };
  const clock = new TrustedClock(state, {}, sync);
  clock.runtime = raw.runtime || null;
  try {
    let now = null;
    if (row.action === "sample") {
      const s = row.server;
      state.clockOffset = sync.serverClockOffset(new Date(s.serverTimeMs).toISOString(),
        s.requestWallMs, s.responseWallMs, s.requestSequence);
    } else now = clock.trustedNow(row.reading.wallMs, row.reading.monotonicMs ?? null);
    state.durationsMs = { focus: 60000 };
    const actions = appState.create({ state, external: { host: {}, syncCore: sync, syncStorage: {} }, use: {} });
    const anchorAt = new Date(row.trustedAnchorMs ?? 1000000).toISOString();
    const nativeTimer = actions.normalizeTimer({ id: "clock-probe", status: "running", phase: "focus",
      plannedDurationMs: 60000, elapsedAtAnchorMs: 0, anchorAt });
    const mapped = Date.parse(nativeTimer.anchorAt);
    return { state: { clockOffset: state.clockOffset, minimumWallMs: state.hlcWallMs, runtime: clock.runtime },
      trustedNowMs: now, physicalDeltaMs: mapped - Date.parse(anchorAt),
      physicalAnchorMs: row.trustedAnchorMs == null ? null : mapped };
  } catch (error) { return { error: true }; }
});
console.log(JSON.stringify(output));
