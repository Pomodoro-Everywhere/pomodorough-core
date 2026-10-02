"use strict";
const fs = require("node:fs");
const assert = require("node:assert/strict");
const { spawnSync } = require("node:child_process");
const sync = require(process.argv[2]);
const storage = require(process.argv[3]);
const stateModule = require(require("node:path").join(require("node:path").dirname(process.argv[3]), "app-state.js"));
const bootstrapModule = require(require("node:path").join(require("node:path").dirname(process.argv[3]), "app-bootstrap.js"));
const bridge = process.argv[4];

function dispatch(operation, input, calls) {
  const inputRaw = JSON.stringify(input);
  const result = spawnSync(bridge, [], { input: `{"operation":${JSON.stringify(operation)},"input":${inputRaw}}\n`, encoding: "utf8" });
  if (result.status !== 0) throw new Error(result.stderr);
  const envelope = JSON.parse(result.stdout);
  assert.deepStrictEqual(JSON.parse(envelope.inputRaw), envelope.inputDecoded, "Native request decode mismatch");
  assert.deepStrictEqual(envelope.inputDecoded, JSON.parse(inputRaw), "Raw request changed before Core");
  if (envelope.error) throw new Error(envelope.error);
  assert.deepStrictEqual(JSON.parse(envelope.raw), envelope.decoded, "Native response decode mismatch");
  calls.push(envelope);
  return envelope.decoded;
}
function run(input) {
  const workspace = input.local.workspace;
  const base = workspace.base;
  const calls = [];
  const host = { crypto: { randomUUID: () => "probe-tab" } };
  const state = Object.assign(stateModule.createState(host), {
    deviceId: "device-a", localOwnerId: input.local.ownerId, user: { id: input.currentUserId },
    baseTimer: base.canonicalTimer, baseHistory: base.history, baseTasks: base.tasks,
    baseDurationsMs: base.durationsMs, baseAutoStartBreaks: base.autoStartBreaks,
    baseSelectedTaskId: base.selectedTaskId, deliveryProof: workspace.neverSent,
    canonicalHead: workspace.canonicalHead, projectionPending: input.local.projectionPending || null,
    pending: workspace.local.commands, pendingTaskOperations: workspace.local.taskOperations,
    pendingDurationOperations: workspace.local.durationOperations, pendingAutoStartOperations: workspace.local.autoStartOperations,
    pendingSelectedTaskOperations: workspace.local.selectedTaskOperations, bootstrapPreview: input.remote
  });
  const observationRaw = JSON.stringify(state);
  const adapter = {
    projectSynchronizedState: value => dispatch("projection.apply.v2", value, calls),
    planBootstrap: value => dispatch("bootstrap.plan.v1", value, calls)
  };
  const nativeStorage = {
    projectState: value => storage.projectState({ ...value, sharedCore: adapter }),
    bootstrapPlan: value => storage.bootstrapPlan({ ...value, sharedCore: adapter })
  };
  const actions = stateModule.create({ state, external: { host, syncCore: sync, syncStorage: nativeStorage }, use: {} });
  const oldNow = Date.now;
  Date.now = () => Date.parse(workspace.now);
  try { actions.projectOwnerState(state); }
  catch (error) { return { productionError: error.message }; }
  finally { Date.now = oldNow; }
  const bootstrap = bootstrapModule.create({ state, external: { syncCore: sync, syncStorage: nativeStorage },
    use: { defaultDurationsMs: actions.defaultDurationsMs } });
  const local = bootstrap.localBootstrapState();
  const plan = bootstrap.buildBootstrapPlan(local);
  const projectionRaw = JSON.stringify(state);
  return { hasLocalState: sync.hasLocalState(local), hasRemoteState: sync.hasRemoteState({ ...input.remote,
      defaultDurationsMs: actions.defaultDurationsMs() }),
    localHistory: local.history, localDisplayHistoryCount: sync.completedHistoryCount(local.history),
    remoteDisplayHistoryCount: sync.completedHistoryCount(input.remote.history), plan,
    projectionCall: calls[0], emittedInput: calls[1].inputDecoded,
    observationRaw, observationDecoded: JSON.parse(observationRaw),
    projectionRaw, projectionDecoded: JSON.parse(projectionRaw) };
}
const inputs = JSON.parse(fs.readFileSync(0, "utf8"));
process.stdout.write(JSON.stringify(inputs.map(run)));
