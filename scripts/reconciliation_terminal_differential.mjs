import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { nativeCorpus } from "../tests/aggregate_artifact/corpus.mjs";
import { nativeResponses } from "../tests/aggregate_artifact/native_oracle.mjs";
import { terminalRequest, terminalCases } from "../tests/aggregate_artifact/terminal_cases.mjs";
import { vector } from "../tests/aggregate_artifact/cases.mjs";
import { invoke } from "../tests/aggregate_artifact/abi_host.mjs";
import { retainedIntentCases } from "../tests/aggregate_artifact/terminal_provenance_cases.mjs";

function baselineResponses(binary, cases) {
  const input = cases.map(({ operation, input }) => JSON.stringify({ operation, input })).join("\n") + "\n";
  const result = spawnSync(binary, [], { input, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stderr);
  const output = result.stdout.trimEnd().split("\n");
  assert.equal(output.length, cases.length);
  return output;
}

function compareLegacy(binary, corpus) {
  const legacy = corpus.cases.map((item, index) => ({ ...item, expected: corpus.expected[index] }))
    .filter((item) => item.operation !== "reconcile.rebase.v3" && !item.name.startsWith("missing-history-"));
  assert.equal(legacy.length, 501, "unchanged legacy cohort size");
  const baseline = baselineResponses(binary, legacy);
  baseline.forEach((raw, index) => assert.equal(raw, legacy[index].expected, legacy[index].name));
  const strict = terminalCases().map((item) => ({ ...item, operation: "reconcile.rebase.v2" }));
  strict.push({ ...vector("reconcile.rebase.v1", "actual-http-strict-v1", terminalRequest()), ok: false });
  assert.equal(strict.length, 42, "unchanged strict cohort size");
  const current = baselineResponses(process.env.CORE_CURRENT_ORACLE, strict);
  const previous = baselineResponses(binary, strict);
  assert.deepEqual(current, previous, "complete old strict returns and error precedence");
  const additional = ["reconcile.rebase.v1", "reconcile.rebase.v2"].flatMap((operation) =>
    retainedIntentCases().map((item) => ({ ...item, operation })));
  assert.deepEqual(baselineResponses(process.env.CORE_CURRENT_ORACLE, additional), baselineResponses(binary, additional));
  const mixed = ackReasonMixedCases();
  const mixedCurrent = baselineResponses(process.env.CORE_CURRENT_ORACLE, mixed);
  const mixedPrevious = baselineResponses(binary, mixed);
  assert.deepEqual(mixedCurrent, mixedPrevious, "ACK reason mixed-error precedence");
  mixed.forEach((item, index) => {
    const envelope = JSON.parse(mixedCurrent[index]);
    assert.equal(envelope.ok, item.ok, item.name);
    if (!item.ok) assert.equal(envelope.error, item.error, item.name);
  });
  return { legacyEnvelopes: legacy.length, strictEnvelopes: strict.length,
    additionalStrictEnvelopes: additional.length, ackReasonMixedErrorEnvelopes: mixed.length,
    ackReasonMixedErrorComparisons: mixed.map((item, index) => ({ operation: item.operation, name: item.name,
      input: JSON.parse(item.input), expectedOk: item.ok, expectedError: item.error,
      baselineEnvelope: mixedPrevious[index], currentEnvelope: mixedCurrent[index] })) };
}

function ackReasonMixedCases() {
  const cases = [];
  for (const operation of ["reconcile.rebase.v1", "reconcile.rebase.v2"]) {
    for (const [index, reason] of [undefined, null, false, 0, {}, [], "", "server rejected"].entries()) {
      for (const conflict of ["none", "overlap", "localPhase", "serverHlc", "revision"]) {
        const input = terminalRequest();
        if (conflict !== "overlap") input.response.history = [];
        input.response.acknowledgements[0].outcome = "rejected";
        if (reason === undefined) delete input.response.acknowledgements[0].reason;
        else input.response.acknowledgements[0].reason = reason;
        if (conflict === "localPhase") input.local.commands[0].phase = "unknown";
        if (conflict === "serverHlc") input.response.serverHlcWallMs = -1;
        if (conflict === "revision") input.response.revision = -1;
        const failures = { overlap: "canonical timer overlaps timer history", localPhase: "invalid timer command",
          serverHlc: "invalid canonical response server HLC", revision: "invalid canonical response revision" };
        const error = failures[conflict] ?? (typeof reason === "string" ? null : "invalid acknowledgements set");
        cases.push(vector(operation, `${conflict}-reason-${index}`, input, error === null,
          error === null ? null : `invalid shared-core input: ${error}`));
      }
    }
  }
  return cases;
}

function actualHttp(evidence) {
  const before = JSON.stringify(evidence);
  assert.deepEqual(JSON.parse(evidence.finishResponseRaw), evidence.finishResponse);
  assert.deepEqual(JSON.parse(evidence.finishRequestRaw), evidence.finishRequest);
  const claim = evidence.sourceClaim.claim.claim;
  assert.equal(claim.body, evidence.finishRequestRaw);
  const persisted = evidence.sourceClaim.persisted;
  const meta = Object.fromEntries(persisted.meta.map(({ key, value }) => [key, value]));
  assert.deepEqual(meta.outgoingSync, claim);
  const input = { local: { commands: persisted.pending, taskOperations: persisted.pendingTasks,
    durationOperations: persisted.pendingDurations, autoStartOperations: persisted.pendingAutoStarts,
    selectedTaskOperations: persisted.pendingSelectedTasks }, sent: claim.sent,
    neverSent: evidence.sourceClaim.claim.proof, timerDependencies: meta.timerDependencies,
    response: JSON.parse(evidence.finishResponseRaw) };
  assert.deepEqual(input.local.commands, evidence.sourceClaim.plan.workspace.local.commands);
  const output = JSON.parse(nativeResponses([vector("reconcile.rebase.v3", "live-http", input)])[0]).value;
  assert.deepEqual(output.canonicalResponse, evidence.finishResponse);
  assert.deepEqual(output.baseTimer, evidence.finishResponse.canonicalTimer);
  assert.deepEqual(output.baseHistory, evidence.finishResponse.history);
  assert.deepEqual(output.pending, []);
  assert.equal(output.timer.lastIntent.commandId, evidence.finishRequest.commands[0].id);
  assert.equal(output.timer.anchorAt, evidence.finishRequest.commands[0].occurredAt);
  assert.equal(output.timer.plannedDurationMs, evidence.finishRequest.commands[0].plannedDurationMs);
  assert.equal(JSON.stringify(evidence), before, "claim, proof, queues, and logical clock unchanged");
  return { input, output, claimBody: claim.body, logicalClock: meta.hlc };
}

const [baseline, httpPath, officialPath] = process.argv.slice(2);
assert.ok(baseline && process.env.CORE_CURRENT_ORACLE, "baseline path and CORE_CURRENT_ORACLE required");
const corpus = nativeCorpus();
const parity = compareLegacy(baseline, corpus);
const probe = vector("reconcile.rebase.v3", "baseline-unsupported", terminalRequest());
const unsupported = JSON.parse(baselineResponses(baseline, [probe])[0]);
assert.deepEqual(unsupported, { ok: false, error: "unsupported shared-core operation: reconcile.rebase.v3" });
const oldOverlap = JSON.parse(baselineResponses(baseline, [{ ...probe, operation: "reconcile.rebase.v2" }])[0]);
assert.deepEqual(oldOverlap, { ok: false, error: "invalid shared-core input: canonical timer overlaps timer history" });
const live = httpPath ? actualHttp(JSON.parse(readFileSync(httpPath, "utf8"))) : null;
if (officialPath) {
  const { instance } = await WebAssembly.instantiate(readFileSync(officialPath));
  assert.deepEqual(JSON.parse(invoke(instance.exports, probe)), unsupported);
  assert.deepEqual(JSON.parse(invoke(instance.exports, { ...probe, operation: "reconcile.rebase.v2" })), oldOverlap);
}
const receipt = { ...parity, corpusEnvelopes: corpus.cases.length,
  baselineUnsupported: unsupported, baselineOverlap: oldOverlap, officialOldChecked: Boolean(officialPath), live };
if (process.env.CORE_TERMINAL_RECEIPT) writeFileSync(process.env.CORE_TERMINAL_RECEIPT, JSON.stringify(receipt, null, 2));
console.log(JSON.stringify({ ...receipt, ackReasonMixedErrorComparisons: undefined,
  live: live ? { finishId: live.output.timer.lastIntent.commandId,
  status: live.output.timer.status, pending: live.output.pending.length, rawEvidenceEqual: true, claimAndClockUnchanged: true } : null }, null, 2));
