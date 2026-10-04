"use strict";
const assert = require("node:assert/strict"), fs = require("node:fs"), path = require("node:path"), test = require("node:test");
const a = require("./pwa_selection_source_adapter.cjs");
const f = require(path.join(a.root, "server/web/test/p222-completion-fixture.js"));
const observations = require("../fixtures/pwa-natural-completion-v1.json");
const operations = new Set(["workspace.intent.v1", "workspace.readModel.v1", "workspace.completionMutation.v1", "timer.completionState.v1"]);

async function load(database, records) {
  const tx = database.transaction(f.stores, "readwrite");
  for (const [name, rows] of Object.entries(records)) {
    const store = tx.objectStore(name); store.clear(); rows.forEach((row) => store.put(row));
  }
  await f.storage.transactionDone(tx);
}

function wire(core, t) {
  const original = core.call.bind(core), calls = [];
  core.call = (operation, input) => {
    if (!operations.has(operation)) return original(operation, input);
    const receipt = a.dispatch(operation, input); calls.push({ operation, ...receipt }); return receipt.completeReturn;
  };
  t.after(() => { core.call = original; }); return calls;
}

function canonicalUnchanged(before, after) {
  assert.deepEqual(f.meta(after, "snapshot"), f.meta(before, "snapshot"));
  for (const name of f.stores.slice(1)) assert.deepEqual(after[name], before[name]);
  for (const key of ["hlc", "deviceSequence", "uuidV7", "timerOwner", "outgoingSync", "canonicalHead", "timerDependencies", "deliveryProof"]) {
    assert.deepEqual(f.meta(after, key), f.meta(before, key), key);
  }
}

async function frozenPublicChoice(client, frozen, before, expected, phase, calls) {
  await load(client.use.database(), before);
  a.bind(client, frozen.storage); await client.use.reloadPersistedState();
  assert.equal(await client.use.issuePhaseSelection(phase), true, client.notices.join("; "));
  const planner = calls.findLast((item) => item.operation === "workspace.intent.v1" && item.input.intent.kind === "selectPhase");
  assert.deepEqual(planner.completeReturn, expected);
  const model = client.use.getWorkspaceReadModel(); assert.equal(model.display.phase, phase);
  return { planner, model };
}

for (const phase of ["focus", "long_break"]) test(`public natural chooser records Core-owned ${phase} choice through current and frozen storage`, async (t) => {
  const { client, core } = await f.fixture(t), calls = wire(core, t), raw = observations.receipts[0];
  const database = client.use.database(), at = Date.parse(raw.request.input.clock.observedAt);
  t.mock.timers.setTime(at); client.use.trustedNow = () => at;
  await load(database, raw.persistedBefore); await client.use.reloadPersistedState();
  assert.equal(client.use.getWorkspaceReadModel().display.phase, "short_break");
  const legacyBefore = await f.dump(database);
  const legacyActionReturn = await client.use.issuePhaseSelection(phase);
  const legacy = calls.findLast((item) => item.operation === "workspace.intent.v1" && item.input.intent.kind === "selectPhase");
  assert.deepEqual(a.dispatch(legacy.operation, legacy.inputRaw, a.baseline).completeReturn, legacy.completeReturn);
  assert.equal(legacy.completeReturn.selection.generation, "0"); assert.equal(legacy.completeReturn.selection.explicit, false);
  assert.equal(legacyActionReturn, phase !== "focus");
  await load(database, legacyBefore);
  await f.seedMeta(database, { completionState: { selection: { phase: "focus", generation: "7", explicit: false },
    lifecycle: { consumedCompletions: [], pendingBreaks: [] } } });
  const current = a.adapter(core); a.bind(client, current.storage);
  await client.use.reloadPersistedState(); const before = await f.dump(database);
  assert.equal(await client.use.issuePhaseSelection(phase), true, client.notices.join("; "));
  const selected = calls.findLast((item) => item.operation === "workspace.intent.v1" && item.input.intent.kind === "selectPhase");
  const persistedRead = current.reads.at(-1);
  assert.deepEqual(selected.input.selection, persistedRead.completionState.selection);
  assert.deepEqual(selected.input.lifecycle, persistedRead.completionState.lifecycle);
  assert.deepEqual(selected.input.workspace.base, raw.request.input.workspace.base);
  assert.equal(selected.completeReturn.selection.generation, "8"); assert.equal(selected.completeReturn.selection.explicit, true);
  assert.throws(() => a.dispatch(selected.operation, selected.inputRaw, a.baseline), /unknown field.*lifecycle/);
  const after = await f.dump(database); canonicalUnchanged(before, after);
  const publicRead = client.use.getWorkspaceReadModel();
  assert.equal(publicRead.display.phase, phase); assert.equal(publicRead.cadence.completedFocusTotal, 1);
  const state = f.meta(after, "completionState");
  assert.deepEqual(state, { selection: selected.completeReturn.selection, lifecycle: selected.completeReturn.lifecycle });
  const input = { ...client.use.captureAccountContext(), deviceId: client.state.deviceId, tabId: client.use.tabId(),
    nowMs: at, localNowMs: at, leaseMs: 60000, timerUuid: selected.input.identities.timerUuid,
    intent: { kind: "selectPhase", phase }, entropy: (bytes) => bytes.fill(0) };
  await load(database, before); const frozen = a.adapter(core, a.frozen);
  const frozenReturn = await frozen.storage.planWorkspaceMutation(database, input);
  const frozenCall = calls.at(-1), frozenAfter = await f.dump(database);
  await load(database, before); const currentReturn = await current.storage.planWorkspaceMutation(database, input);
  const currentCall = calls.at(-1);
  assert.equal(currentCall.inputRaw, frozenCall.inputRaw);
  assert.deepEqual(frozenReturn, currentReturn);
  assert.deepEqual(await f.dump(database), frozenAfter);
  assert.deepEqual(frozen.reads.at(-1).completionState, current.reads.at(-1).completionState);
  const publicFrozen = await frozenPublicChoice(client, frozen, before, selected.completeReturn, phase, calls);
  assert.deepEqual(await f.dump(database), after);
  database.close(); client.use.setDatabaseForTest(await client.use.openDatabase());
  assert.deepEqual(await f.dump(client.use.database()), after);
  await client.use.reloadPersistedState();
  assert.equal(client.use.getWorkspaceReadModel().display.phase, phase);
  const read = publicRead;
  a.receipts.push({ phase, legacyActionReturn, legacyCompletePlanner: legacy, before, selected, after,
    completeCurrentReturn: currentReturn, completeFrozenReturn: frozenReturn, currentCall, frozenCall, rawRecordRead: persistedRead, publicRead, publicFrozen, read });
});

test("two independent instances serialize choices using the state read inside each transaction", async (t) => {
  const { client, core, open } = await f.fixture(t), calls = wire(core, t), raw = observations.receipts[0];
  const at = Date.parse(raw.request.input.clock.observedAt); t.mock.timers.setTime(at);
  await load(client.use.database(), raw.persistedBefore);
  await f.seedMeta(client.use.database(), { completionState: { selection: { phase: "focus", generation: "11", explicit: false }, lifecycle: {} } });
  const peer = await open(), storage = a.adapter(core);
  for (const instance of [client, peer]) { instance.use.trustedNow = () => at; a.bind(instance, storage.storage); await instance.use.reloadPersistedState(); }
  const before = await f.dump(client.use.database());
  const returned = await Promise.all([client.use.issuePhaseSelection("focus"), peer.use.issuePhaseSelection("long_break")]);
  assert.deepEqual(returned, [true, true]);
  const choices = calls.filter((item) => item.operation === "workspace.intent.v1" && item.input.intent.kind === "selectPhase");
  assert.deepEqual(choices.map((item) => item.input.selection.generation), ["11", "12"]);
  assert.deepEqual(choices.map((item) => item.completeReturn.selection.generation), ["12", "13"]);
  const after = await f.dump(client.use.database()); canonicalUnchanged(before, after);
  assert.equal(f.meta(after, "completionState").selection.generation, "13");
  assert.equal(f.meta(after, "completionState").selection.phase, "long_break");
  peer.use.database().close(); const restarted = await open();
  assert.deepEqual(await f.dump(restarted.use.database()), after);
  a.receipts.push({ case: "serial-transactions", before, choices, after, returned });
});

test.after(() => fs.writeFileSync(path.join(a.directory, "core-pwa-selection-source.json"), JSON.stringify({ hashes: a.hashes, receipts: a.receipts }, null, 2)));

for (const invalid of ["phase", "generation", "lifecycle", "timer"]) test(`invalid ${invalid} aborts the actual choice transaction`, async (t) => {
  const { client, core } = await f.fixture(t); wire(core, t);
  const raw = observations.receipts[0], at = Date.parse(raw.request.input.clock.observedAt);
  t.mock.timers.setTime(at); await load(client.use.database(), raw.persistedBefore);
  const state = { selection: { phase: "focus", generation: invalid === "generation" ? "9223372036854775807" : "3", explicit: false }, lifecycle: {} };
  if (invalid === "lifecycle") state.lifecycle = { consumedCompletions: [["timer", null, "focus"]] };
  await f.seedMeta(client.use.database(), { completionState: state });
  if (invalid === "timer") {
    const snapshot = f.meta(await f.dump(client.use.database()), "snapshot");
    snapshot.canonicalTimer.status = "invalid"; await f.seedMeta(client.use.database(), { snapshot });
  }
  const before = await f.dump(client.use.database()), current = a.adapter(core);
  await assert.rejects(current.storage.planWorkspaceMutation(client.use.database(), {
    ...client.use.captureAccountContext(), deviceId: client.state.deviceId, tabId: client.use.tabId(),
    nowMs: at, localNowMs: at, leaseMs: 60000, timerUuid: raw.request.input.identities.timerUuid,
    intent: { kind: "selectPhase", phase: invalid === "phase" ? { focus: null } : "focus" }, entropy: (bytes) => bytes.fill(0) }));
  const after = await f.dump(client.use.database()); assert.deepEqual(after, before);
  a.receipts.push({ case: `invalid-${invalid}`, before, after, outcome: "aborted" });
});
