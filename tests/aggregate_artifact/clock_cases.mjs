import { fixture, patchInput, vector } from "./cases.mjs";

function clockRequest(source, item) {
  const input = structuredClone(source.templates[item.profile]);
  if (item.saved) {
    const field = { desktopTrustedClock: "sample", pwaTrustedClock: "clockOffset" }[item.profile];
    if (field) input.state[field] = structuredClone(source[item.saved]);
    else input.state = structuredClone(source[item.saved]);
  }
  if (item.requestSample) input.state.requestSample = structuredClone(source.androidSample);
  if (item.persistedAndroid) Object.assign(input.state, {
    serverClockOffsetMs: 100, serverClockUncertaintyMs: 1,
    serverClockSamplePhysicalMs: 1000000, serverClockSampleElapsedRealtimeMs: 20000,
    serverClockBootId: "boot-a", retainedWallMs: 1000100,
  });
  if (item.runtimeDesktop) input.state.anchor = structuredClone(source.desktopSample);
  if (item.runtimePwa) input.state.runtime = { identity: "100:1:1000000", monotonicMs: 20.25, wallMs: 1000100 };
  if (item.server) input.server = structuredClone(source.servers[item.profile]);
  return patchInput(input, item.patch ?? {});
}

export function clockCases() {
  const source = fixture("clock-observe-v1");
  const operation = "clock.observe.v1";
  const cases = Object.entries(source.templates).map(([profile, template]) =>
    vector(operation, `${profile}-current`, template));
  cases.push(...source.cases.map((item) =>
    vector(operation, item.name, clockRequest(source, item), !item.reject)));
  for (const name of ["clock-observe-checker-v1", "clock-observe-oracle-v1"]) {
    cases.push(...fixture(name).cases.map((item) => vector(operation, item.name, item.request, !item.reject)));
  }
  return cases;
}
