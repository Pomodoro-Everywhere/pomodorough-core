"use strict";
require("./natural_completion_native_bridge.cjs");
const assert = require("node:assert/strict"), fs = require("node:fs"), path = require("node:path"), Module = require("node:module");
const test = require("node:test");
const root = process.env.POMODOROUGH_ROOT || path.resolve(__dirname, "../..");
const temp = process.env.PWA12_EVIDENCE_DIR || "/var/folders/r_/_mr22dqn24d31b7460cz8z5m0000gn/T/opencode";
const f = require(path.join(root, "server/web/test/p222-completion-fixture.js"));
const storage = require(path.join(root, "server/web/sync-storage.js"));
const transaction = require(path.join(root, "server/web/workspace-transaction.js"));
const evidence = JSON.parse(fs.readFileSync(path.join(__dirname, "../fixtures/pwa-natural-completion-v1.json"), "utf8"));
const receipts = [];

async function load(database, records) {
  const tx = database.transaction(f.stores, "readwrite");
  for (const [name, rows] of Object.entries(records)) {
    const store = tx.objectStore(name); store.clear(); rows.forEach((row) => store.put(row));
  }
  await storage.transactionDone(tx);
}

function frozen(core) {
  const filename = path.join(root, "server/web/sync-storage.js"), module = new Module(filename);
  module.filename = filename; module.paths = Module._nodeModulePaths(path.dirname(filename));
  module._compile(fs.readFileSync(path.join(temp, "pwa-core-045/baseline/web/sync-storage.js"), "utf8"), filename);
  module.exports.setSharedCore(core);
  return module.exports;
}

for (const [index, original] of evidence.receipts.entries()) test(`native intentional fix through current/frozen public flow ${index}`, async (t) => {
  const { client, core, open } = await f.fixture(t);
  const database = client.use.database(), before = original.persistedBefore, at = Date.parse(original.request.input.clock.observedAt);
  await load(database, before); t.mock.timers.setTime(at); client.use.trustedNow = () => at;
  await client.use.reloadPersistedState();
  const model = client.use.getWorkspaceReadModel();
  assert.equal(model.display.phase, "short_break"); assert.ok(model.availableIntents.includes("finish"));
  const calls = [], call = core.call.bind(core);
  core.call = (operation, input) => {
    const inputRaw = JSON.stringify(input), value = call(operation, input);
    calls.push({ operation, inputRaw, input: JSON.parse(inputRaw), value: structuredClone(value) }); return value;
  };
  const input = { ...client.use.captureAccountContext(), deviceId: client.state.deviceId, tabId: client.use.tabId(),
    nowMs: at, localNowMs: at, leaseMs: 60000, timerUuid: original.request.input.identities.timerUuid,
    stage: original.request.input.stage, requestedTimer: client.state.timer, entropy: (bytes) => bytes.fill(0) };
  const current = await storage.planWorkspaceMutation(database, input), after = await f.dump(database);
  const receipt = calls.find((item) => item.operation === "workspace.completionMutation.v1");
  assert.deepEqual(current, receipt.value); assert.deepEqual(JSON.parse(receipt.inputRaw), receipt.input);
  assert.deepEqual(receipt.input.workspace.base, original.request.input.workspace.base);
  for (const [domain, store] of Object.entries(transaction.QUEUE_STORES)) assert.deepEqual(receipt.input.workspace.local[domain], before[store]);
  for (const key of ["neverSent", "canonicalHead", "timerDependencies", "displayContext"]) {
    assert.deepEqual(receipt.input.workspace[key], original.request.input.workspace[key]);
  }
  assert.equal(current.outcome, "planned"); assert.equal(current.commands.length, 1);
  assert.deepEqual(f.meta(after, "snapshot"), f.meta(before, "snapshot"));
  await load(database, before);
  const old = frozen(core);
  const frozenReturn = await old.planWorkspaceMutation(database, input);
  const frozenCall = calls.filter((item) => item.operation === "workspace.completionMutation.v1").at(-1);
  assert.equal(frozenCall.inputRaw, receipt.inputRaw);
  assert.deepEqual(frozenReturn, current); assert.deepEqual(await f.dump(database), after);
  await load(database, before); await client.use.reloadPersistedState();
  const returned = await client.use.finishTimer(input.stage === "automaticFinishCommit");
  assert.equal(returned, true);
  const publicAfter = await f.dump(database), publicPlan = calls.filter((item) => item.operation === "workspace.completionMutation.v1").at(-1);
  assert.equal(publicAfter.pending.filter((command) => command.type === "finish").length, 1);
  assert.equal(f.meta(publicAfter, "settings").selectedPhase, "short_break");
  assert.equal(client.use.getWorkspaceReadModel().cadence.completedFocusTotal, 1);
  const reopened = await open(); await reopened.use.reloadPersistedState();
  assert.deepEqual(await f.dump(reopened.use.database()), publicAfter);
  receipts.push({ case: index, originalCompleteReturn: original.completeProductionReturn, before,
    completeCurrentReturn: current, completeFrozenReturn: frozenReturn, after, publicActionReturn: returned,
    completePublicPlanner: publicPlan, publicAfter, model });
});

test.after(() => fs.writeFileSync(path.join(temp, "core-pwa12-source-green.json"), JSON.stringify({ receipts }, null, 2)));

for (const field of ["stage", "compatibility", "replicationMode", "selection"]) {
  test(`malformed completion ${field} aborts the actual PWA transaction without writes`, async (t) => {
    const { client, core } = await f.fixture(t), database = client.use.database();
    const original = evidence.receipts[0], at = Date.parse(original.request.input.clock.observedAt);
    await load(database, original.persistedBefore); t.mock.timers.setTime(at); client.use.trustedNow = () => at;
    await client.use.reloadPersistedState();
    const before = await f.dump(database), call = core.call.bind(core), calls = [];
    const sharedCore = { call(operation, input) {
      if (operation === "workspace.completionMutation.v1") {
        input = structuredClone(input);
        input[field] = field === "selection" ? ["focus", "0", false] : { [input[field]]: null };
        calls.push({ operation, inputRaw: JSON.stringify(input) });
      }
      return call(operation, input);
    } };
    await assert.rejects(storage.planWorkspaceMutation(database, { ...client.use.captureAccountContext(),
      deviceId: client.state.deviceId, tabId: client.use.tabId(), nowMs: at, localNowMs: at,
      leaseMs: 60000, timerUuid: original.request.input.identities.timerUuid,
      stage: "finishCommit", requestedTimer: client.state.timer, sharedCore }), /must be a JSON/);
    assert.equal(calls.length, 1); assert.deepEqual(await f.dump(database), before);
    receipts.push({ case: `malformed-${field}`, before, after: await f.dump(database), calls, outcome: "aborted" });
  });
}
