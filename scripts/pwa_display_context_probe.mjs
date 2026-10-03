// Read existing official bytes and raw migration receipts. Never build WASM.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { fixture, vector } from "../tests/aggregate_artifact/cases.mjs";
import { invoke } from "../tests/aggregate_artifact/abi_host.mjs";
import { nativeResponses } from "../tests/aggregate_artifact/native_oracle.mjs";
import { finishRequest, intentRequest, meta, rawWorkspace, readRequest } from "../tests/aggregate_artifact/pwa_display_cases.mjs";
import { nativeCorpus } from "../tests/aggregate_artifact/corpus.mjs";

const [officialPath, receiptsPath, serverPath, outputPath] = process.argv.slice(2);
if (!officialPath || !receiptsPath || !serverPath || !outputPath) throw new Error("usage: node scripts/pwa_display_context_probe.mjs OFFICIAL_WASM CHECKER_EVIDENCE SERVER_ROOT OUTPUT_JSON");
const source = fixture("pwa-display-context-v1");
const receipts = JSON.parse(readFileSync(receiptsPath, "utf8"));
for (const observation of source.observations) {
  assert.deepEqual(observation.persisted, receipts.find((receipt) => receipt.case === observation.case).persisted);
}
const bytes = readFileSync(officialPath);
assert.equal(bytes.length, 2385109);
assert.equal(createHash("sha256").update(bytes).digest("hex"), source.provenance.officialSha256);
const { instance } = await WebAssembly.instantiate(bytes, {});
const baseline = spawnSync("git", ["show", "50c86a2:web/app-state.js"], { cwd: serverPath, encoding: "utf8" });
assert.equal(baseline.status, 0, baseline.stderr);
const module = { exports: {} };
runInNewContext(baseline.stdout, { module, globalThis: {}, Date, Number, JSON, Object, Math, Set });
const evidence = [];

function native(item) {
  const envelope = nativeResponses([item])[0];
  evidence.push({ ...item, inputDecoded: JSON.parse(item.input), completeEnvelopeRaw: envelope, completeEnvelope: JSON.parse(envelope) });
  return JSON.parse(envelope).value;
}

function baselineQueues(records, workspace) {
  let selected;
  const state = { deviceId: meta(records, "deviceId"), hlcWallMs: 0, hlcCounter: 0 };
  const local = { baseTimer: workspace.base.canonicalTimer, baseHistory: workspace.base.history,
    baseTasks: workspace.base.tasks, baseDurationsMs: workspace.base.durationsMs,
    baseAutoStartBreaks: workspace.base.autoStartBreaks, baseSelectedTaskId: workspace.base.selectedTaskId,
    selectedPhase: "focus", pending: records.pending, pendingTaskOperations: records.pendingTasks,
    pendingDurationOperations: records.pendingDurations, pendingAutoStartOperations: records.pendingAutoStarts,
    pendingSelectedTaskOperations: records.pendingSelectedTasks, projectionPending: meta(records, "projectionPending"),
    deliveryProof: meta(records, "deliveryProof"), canonicalHead: meta(records, "canonicalHead") };
  const use = module.exports.create({ state, external: { host: {}, syncCore: { validClockSample: () => false,
    trustedNow: () => Date.parse("2026-08-31T12:00:01Z"), compareTimerCommands: () => 0 },
    syncStorage: { projectState(input) { selected = input.queues; return { ...workspace.base, canonicalTimer: null }; } } }, use: {} });
  use.projectOwnerState(local);
  return JSON.parse(JSON.stringify(selected));
}

for (const [index, observation] of source.observations.entries()) {
  const workspace = rawWorkspace(index);
  const queued = baselineQueues(observation.persisted, workspace);
  const projected = native(vector("workspace.project.v1", `checker-project-${index}`, { ...workspace, now: "2026-08-31T12:00:01Z" }));
  assert.deepEqual(projected.displayContext.projectionPending, queued);
  assert.deepEqual(projected.projectionPending.commands, []);
  const model = native(vector("workspace.readModel.v1", `checker-read-${index}`, readRequest(workspace)));
  assert.equal(model.canonical.status, "running");
  const oldWorkspace = structuredClone(workspace);
  delete oldWorkspace.displayContext;
  const oldInput = vector("workspace.readModel.v1", `official-idle-${index}`, readRequest(oldWorkspace));
  const old = JSON.parse(invoke(instance.exports, oldInput));
  assert.equal(old.ok, true);
  assert.equal(old.value.canonical.status, "idle");
  const unknownInput = vector("workspace.project.v1", `official-context-negative-${index}`, { ...workspace, now: "2026-08-31T12:00:01Z" });
  const unknownRaw = invoke(instance.exports, unknownInput);
  const unknown = JSON.parse(unknownRaw);
  assert.equal(unknown.ok, false);
  assert.match(unknown.error, /unknown field `displayContext`/);
  evidence.push({ officialIdleInput: oldInput, officialIdleEnvelope: old, officialNegativeInput: unknownInput, officialNegativeEnvelopeRaw: unknownRaw,
    persisted: observation.persisted, baselineQueues: queued });
  const pauseInput = intentRequest(workspace);
  pauseInput.requestedTimer = projected.workspace.canonicalTimer;
  const paused = native(vector("workspace.intent.v1", `checker-pause-${index}`, pauseInput));
  assert.equal(paused.commandOutcomes[0].outcome, "applied");
  const finishInput = finishRequest(paused.workspace);
  for (const field of ["allocation", "observation", "selection"]) finishInput[field] = paused[field];
  finishInput.requestedTimer = paused.projection.canonicalTimer;
  const finished = native(vector("workspace.completionMutation.v1", `checker-finish-${index}`, finishInput));
  assert.equal(finished.commandOutcomes[0].outcome, "applied");
  for (const [operation, input] of [["workspace.readModel.v1", readRequest(workspace)],
    ["workspace.intent.v1", pauseInput], ["workspace.completionMutation.v1", finishInput]]) {
    const item = vector(operation, `official-raw-context-${index}`, input);
    const envelopeRaw = invoke(instance.exports, item);
    assert.match(JSON.parse(envelopeRaw).error, /unknown field `displayContext`/);
    evidence.push({ officialNegativeInput: item, officialNegativeEnvelopeRaw: envelopeRaw });
  }
  const bootstrap = fixture("bootstrap-workspace-v1").request;
  bootstrap.profile = "pwaStorage";
  bootstrap.local.workspace = { ...oldWorkspace, now: "2026-08-31T12:00:01Z" };
  bootstrap.local.projectionPending = workspace.displayContext.projectionPending;
  const goodInput = vector("bootstrap.workspacePlan.v1", `official-bootstrap-${index}`, bootstrap);
  const goodRaw = invoke(instance.exports, goodInput);
  const good = JSON.parse(goodRaw);
  assert.equal(good.ok, true);
  assert.deepEqual(native(goodInput), good.value);
  bootstrap.local.projectionPending.commands = [{ ...workspace.local.commands[0], extension: "untrusted" }];
  const rejectedInput = vector("bootstrap.workspacePlan.v1", `official-bootstrap-corrupt-${index}`, bootstrap, false, "retained payloads");
  const rejectedRaw = invoke(instance.exports, rejectedInput);
  const rejected = JSON.parse(rejectedRaw);
  assert.equal(rejected.ok, false);
  assert.match(rejected.error, /retained payloads/);
  native(rejectedInput);
  evidence.push({ officialBootstrapAcceptedInput: goodInput, officialBootstrapAcceptedEnvelopeRaw: goodRaw,
    officialBootstrapRejectedInput: rejectedInput, officialBootstrapRejectedEnvelopeRaw: rejectedRaw });
  for (const [name, stored] of [["incomplete", { commands: [] }],
    ["corrupt-array", { ...workspace.displayContext.projectionPending, commands: "corrupt" }],
    ["extra-domain", { ...workspace.displayContext.projectionPending, unexpected: true }],
    ["missing-domain", { commands: [], taskOperations: [], durationOperations: [], autoStartOperations: [], unexpected: true }]]) {
    bootstrap.local.projectionPending = stored;
    const item = vector("bootstrap.workspacePlan.v1", `official-bootstrap-${name}-${index}`, bootstrap, false);
    const officialRaw = invoke(instance.exports, item);
    const nativeRaw = nativeResponses([item])[0];
    assert.equal(nativeRaw, officialRaw);
    evidence.push({ bootstrapNegativeInput: item, officialEnvelopeRaw: officialRaw, nativeEnvelopeRaw: nativeRaw });
  }
}
const corpus = nativeCorpus();
const lifecycle = corpus.cases.flatMap((item, index) => item.name.startsWith("pwa-")
  ? [{ ...item, inputDecoded: JSON.parse(item.input), completeEnvelopeRaw: corpus.expected[index], completeEnvelope: JSON.parse(corpus.expected[index]) }] : []);
writeFileSync(outputPath, JSON.stringify({ provenance: source.provenance,
  verification: { nativeCaseCount: corpus.cases.length, semanticHits: corpus.hits }, evidence, lifecycle, limits: [
  "Native capability verified. Official 0.43 lacks this capability.",
  "No new WASM was built or hosted artifact verified.",
  "Persistence and transport authenticity remain host preconditions." ] }, null, 2));
console.log(`Raw checker records, preserved source, official negatives and ${corpus.cases.length} native corpus cases verified: ${outputPath}`);
