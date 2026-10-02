import { fixture, patchInput, vector } from "./cases.mjs";

function sentRequest(source, item) {
  const input = structuredClone(source.base);
  if (item.profile === "pwaRejectedFinish") {
    input.compatibility = item.profile;
    input.sentContext = { kind: "pwa", commands: [], rollbackHistory: item.history ? [source.history] : [] };
  }
  input.sentContext.commands = item.commands.map((id) => structuredClone(source.commands[id]));
  if (item.canonical) input.canonicalTimer = source.timer;
  if (item.history) input.afterHistory = [source.history];
  if (item.ackTimer) input.sentContext.acknowledgementTimer = source.timer;
  if (item.projected) input.sentContext.nextProjectionTimer = source.timer;
  if (item.ackHistory) input.sentContext.acknowledgementHistory = [source.history];
  return patchInput(input, item.patch);
}

export function sentCases() {
  const source = fixture("completion-sent-v1");
  return source.cases.map((item) => vector("timer.completionState.v1", item.name,
    sentRequest(source, item), !item.error, item.error ?? null));
}
