"use strict";
const assert = require("node:assert/strict"), fs = require("node:fs"), path = require("node:path"), test = require("node:test");
const a = require("./pwa_selection_source_adapter.cjs"), f = require(path.join(a.root, "server/web/test/p222-completion-fixture.js"));
const natural = require("../fixtures/pwa-natural-completion-v1.json").receipts[0];
const operations = new Set(["workspace.intent.v1", "workspace.readModel.v1", "workspace.completionMutation.v1", "timer.completionState.v1"]);
const receipts = [];

async function load(database, records) {
  const transaction = database.transaction(f.stores, "readwrite");
  for (const [name, rows] of Object.entries(records)) {
    const store = transaction.objectStore(name); store.clear(); for (const row of rows) store.put(row);
  }
  await f.storage.transactionDone(transaction);
}

function wire(core, t) {
  const original = core.call.bind(core), calls = [];
  core.call = (operation, input) => {
    if (!operations.has(operation)) return original(operation, input);
    const receipt = { operation, inputRaw: JSON.stringify(input), input: structuredClone(input) };
    calls.push(receipt);
    try { const result = a.dispatch(operation, input); Object.assign(receipt, result); return result.completeReturn; }
    catch (error) { receipt.error = error.message; receipt.envelopeRaw = error.coreEnvelopeRaw; throw error; }
  };
  t.after(() => { core.call = original; }); return calls;
}

async function setup(t, previous = false) {
  const { client, core } = await f.fixture(t), calls = wire(core, t);
  if (previous) await load(client.use.database(), natural.persistedBefore);
  await f.seedMeta(client.use.database(), { completionState: { selection: { phase: "focus", generation: "7", explicit: false },
    lifecycle: { consumedCompletions: [], pendingBreaks: [], finishEvidence: [] } } });
  const source = a.adapter(core); a.bind(client, source.storage);
  client.use.trustedNow = () => Date.now();
  if (previous) t.mock.timers.setTime(Date.parse(natural.request.input.clock.observedAt));
  await client.use.reloadPersistedState(); return { client, calls, source };
}

function last(calls, operation, kind) {
  return calls.findLast((call) => call.operation === operation && (!kind || call.input.intent?.kind === kind));
}

async function cycle(t, client, calls, phase, count, during = false) {
  const database = client.use.database(), before = await f.dump(database);
  assert.equal(await client.use.issuePhaseSelection(phase), true, client.notices.join("; "));
  const choice = last(calls, "workspace.intent.v1", "selectPhase");
  assert.equal(choice.completeReturn.selection.explicit, true);
  const chosenRecords = await f.dump(database);
  assert.equal(await client.use.issueCommand("start"), true, client.notices.join("; "));
  const start = last(calls, "workspace.intent.v1", "start"), started = await f.dump(database);
  assert.equal(start.completeReturn.selection.explicit, false);
  assert.equal(start.completeReturn.selection.generation, choice.completeReturn.selection.generation);
  assert.deepEqual(f.meta(started, "snapshot"), f.meta(before, "snapshot"));
  assert.deepEqual(start.input.selection, f.meta(chosenRecords, "completionState").selection);
  assert.deepEqual(start.input.lifecycle, f.meta(chosenRecords, "completionState").lifecycle);
  assert.deepEqual(start.input.observation, f.meta(chosenRecords, "workspaceObservation"));
  if (during) assert.equal(await client.use.issuePhaseSelection(phase), true, client.notices.join("; "));
  const command = start.completeReturn.commands[0];
  const deadline = Date.parse(command.occurredAt) + command.plannedDurationMs;
  t.mock.timers.setTime(deadline);
  const model = client.use.getWorkspaceReadModel();
  const expectedPhase = during ? phase : phase === "focus" ? "short_break" : "focus";
  assert.equal(model.display.phase, expectedPhase); assert.equal(model.canonical.status, "completed");
  assert.equal(model.cadence.completedFocusTotal, count); assert.ok(model.availableIntents.includes("finish"));
  const read = last(calls, "workspace.readModel.v1"), beforeNoop = await f.dump(database);
  assert.equal(await client.use.issueCommand("restart"), false);
  assert.deepEqual(await f.dump(database), beforeNoop);
  database.close(); client.use.setDatabaseForTest(await client.use.openDatabase());
  await client.use.reloadPersistedState(); assert.equal(client.use.getWorkspaceReadModel().display.phase, expectedPhase);
  assert.equal(await client.use.finishTimer(false), true, client.notices.join("; "));
  const finish = last(calls, "workspace.completionMutation.v1"), after = await f.dump(client.use.database());
  assert.equal(finish.completeReturn.commands.length, 1); assert.equal(finish.completeReturn.selection.phase, expectedPhase);
  assert.equal(f.meta(after, "completionState").lifecycle.finishEvidence.length, (f.meta(before, "completionState")?.lifecycle.finishEvidence.length || 0) + 1);
  assert.deepEqual(f.meta(after, "snapshot"), f.meta(before, "snapshot"));
  assert.equal(client.use.getWorkspaceReadModel().cadence.completedFocusTotal, count);
  return { phase, count, during, before, chosenRecords, choice, start, started, read, model, finish, after, deadline };
}

for (const phase of ["focus", "short_break", "long_break"]) for (const during of [false, true]) {
  test(`public choice starts a new ${phase} cycle and protects only its current choice, during=${during}`, async (t) => {
    const { client, calls } = await setup(t, true);
    const result = await cycle(t, client, calls, phase, phase === "focus" ? 2 : 1, during);
    receipts.push({ case: `old-terminal-${phase}-${during}`, ...result, calls });
  });
}

test("two successive public Focus cycles retain one completion each and expose the second Finish", async (t) => {
  const { client, calls } = await setup(t);
  const first = await cycle(t, client, calls, "focus", 1);
  t.mock.timers.setTime(first.deadline + 1000);
  const second = await cycle(t, client, calls, "focus", 2);
  assert.notEqual(first.start.completeReturn.commands[0].timerId, second.start.completeReturn.commands[0].timerId);
  assert.deepEqual(f.meta(second.after, "snapshot"), f.meta(first.before, "snapshot"));
  receipts.push({ case: "two-public-focus-cycles", first, second, calls });
});

for (const owner of ["local", "foreign", "peer", "missing"]) {
  test(`invented consumed Finish aborts real storage and never discharges a public action, owner=${owner}`, async (t) => {
    const { client, calls, source } = await setup(t, true), database = client.use.database();
    const marker = { timerId: client.state.timer.id, phase: "focus", commandId: "fabricated-finish" };
    await f.seedMeta(database, { completionState: { selection: { phase: "focus", generation: "8", explicit: false },
      lifecycle: { consumedCompletions: [marker], pendingBreaks: [] } } });
    const originalOwner = f.meta(await f.dump(database), "timerOwner");
    if (owner !== "local") await f.seedMeta(database, { timerOwner: owner === "missing" ? null : {
      ...originalOwner, ...(owner === "foreign" ? { deviceId: "foreign-owner" } : { tabId: "peer-tab" }) } });
    const before = await f.dump(database);
    t.mock.method(client.external.host.console, "warn", () => {});
    await assert.rejects(source.storage.planWorkspaceMutation(database, {
      ...client.use.captureAccountContext(), deviceId: client.state.deviceId, tabId: client.use.tabId(),
      nowMs: Date.now(), localNowMs: Date.now(), leaseMs: 60000, timerUuid: natural.request.input.identities.timerUuid,
      stage: "finishCommit", requestedTimer: client.state.timer }), /consumed Finish lacks/);
    assert.equal(await client.use.finishTimer(false), false);
    await client.use.reloadPersistedState();
    assert.throws(() => client.use.getWorkspaceReadModel(), /consumed Finish lacks/);
    const after = await f.dump(database); assert.deepEqual(after, before);
    receipts.push({ case: `invented-${owner}`, before, after, calls, publicActionReturn: false });
  });
}

test.after(() => fs.writeFileSync(path.join(a.directory, "core-pwa-cycle-source.json"), JSON.stringify({ hashes: a.hashes, receipts }, null, 2)));
