// Transport shim for the existing shared finishApplied operation; no phase policy.
const {spawnSync} = require("node:child_process");
const fs = require("node:fs");
const requests = JSON.parse(fs.readFileSync(0, "utf8"));
function finishAppliedPlan(input) {
  const reference = new Date(input.referenceMs);
  const start = new Date(reference.getFullYear(), reference.getMonth(), reference.getDate());
  const end = new Date(reference.getFullYear(), reference.getMonth(), reference.getDate() + 1);
  const request = {operation: "timer.completionPlan.v1", input: {
    kind: "finishApplied", source: {commandId: input.commandId, timerId: input.timerId,
      phase: input.phase, occurredAt: input.occurredAt}, history: input.history,
    autoStartBreaks: input.autoStartBreaks, localDeviceId: input.localDeviceId,
    ownership: null, dayStart: start.toISOString(), dayEnd: end.toISOString()
  }};
  const result = spawnSync(process.argv[2], [], {input: JSON.stringify(request) + "\n", encoding: "utf8"});
  if (result.status !== 0) throw new Error(result.stderr);
  const output = JSON.parse(result.stdout);
  if (output.error) throw new Error("error:" + output.error);
  return output;
}
const phases = requests.map((input) => {
  const policy = new CompletionPlanPolicy({history: input.sentContext.rollbackHistory, deviceId: "probe"},
    {phaseConfig: () => ({focus: {}, short_break: {}, long_break: {}})}, {finishAppliedPlan});
  try {
    return policy.selectedPhaseAfterCommandAcknowledgements(input.selection.phase,
      input.sentContext.commands, input.acknowledgements, input.sentContext.rollbackHistory);
  } catch (error) {
    if (error.message !== "error:invalid timer history") throw error;
    return error.message;
  }
});
process.stdout.write(JSON.stringify(phases));
