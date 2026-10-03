"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const crypto = require("node:crypto");
const a = require("./pwa_ownership_source_adapter.cjs");
const f = require(a.server + "/web/test/p222-completion-fixture.js");
const compiled = a.compileStorage();
const receipts = [];
const seed = JSON.parse(fs.readFileSync(__dirname + "/../fixtures/pwa-ownership-plan-v1.json", "utf8"));

test.after(() => {
  if (process.env.CORE_PWA09_EVIDENCE) fs.writeFileSync(process.env.CORE_PWA09_EVIDENCE,
    JSON.stringify({ sourceSha256: compiled.sourceSha256, receipts }, null, 2));
});

async function prepared(t) {
  const current = await f.fixture(t);
  const official = current.core.call.bind(current.core);
  const bytes = fs.readFileSync(a.server + "/web/pomodorough_core.wasm");
  assert.equal(crypto.createHash("sha256").update(bytes).digest("hex"), "895621370566284e08f03385146bdfe07b41c78212273dd8124302d05dfeaed4");
  assert.throws(() => official("workspace.ownershipPlan.v1", seed.request), /unsupported shared-core operation: workspace.ownershipPlan.v1/);
  t.mock.method(current.core, "call", a.native);
  compiled.storage.setSharedCore(current.core);
  const start = { ...seed.request.workspace.local.commands[0], occurredAt: new Date(f.nowMs).toISOString(), hlcWallMs: f.nowMs };
  await f.seedQueues(current.client.use.database(), { commands: [start] });
  return { ...current, start, baselineError: "unsupported shared-core operation: workspace.ownershipPlan.v1" };
}

function input(client, extra = {}) {
  return { ...client.use.captureDatabaseContext(), expectedUserId: f.sync.accountOwnerId(f.user),
    deviceId: "device-local", tabId: "tab-local", timerId: "ownership-timer",
    nowMs: f.nowMs + 1000, leaseMs: 30000, ...extra };
}

async function restore(database, records) {
  const transaction = database.transaction(Object.keys(records), "readwrite");
  for (const [name, rows] of Object.entries(records)) {
    const store = transaction.objectStore(name); store.clear();
    for (const row of rows) store.put(row);
  }
  await f.storage.transactionDone(transaction);
}

function captureOwnerWrites(database) {
  const transaction = database.transaction;
  const writes = [];
  database.transaction = function (...args) {
    const current = transaction.apply(this, args);
    if (args[1] !== "readwrite") return current;
    const store = current.objectStore("meta");
    const put = store.put.bind(store), remove = store.delete.bind(store);
    store.put = (row) => {
      if (row.key === "timerOwner") writes.push({ kind: "recordTimerOwner", ...structuredClone(row.value) });
      return put(row);
    };
    store.delete = (key) => {
      if (key === "timerOwner") writes.push({ kind: "removeTimerOwner" });
      return remove(key);
    };
    return current;
  };
  return { writes, restore() { database.transaction = transaction; } };
}

async function compare(current, method, args, intentional = false) {
  const database = current.client.use.database();
  const before = await f.dump(database);
  const originalWrites = captureOwnerWrites(database);
  const originalReturn = await f.storage[method](database, ...args);
  originalWrites.restore();
  const originalAfter = await f.dump(database);
  await restore(database, before);
  const start = a.calls.length;
  const nativeWrites = captureOwnerWrites(database);
  const completeReturn = await compiled.storage[method](database, ...args);
  nativeWrites.restore();
  const after = await f.dump(database);
  const calls = a.calls.slice(start);
  assert.ok(calls.length, `${method} did not reach new Core method`);
  for (const call of calls) {
    assert.deepEqual(call.observed.results.timerOwner, before.meta.find((row) => row.key === "timerOwner"));
    assert.deepEqual(call.observed.results.commands, before.pending);
    if (!call.observed.installed) {
      for (const [name, store] of Object.entries(a.stores)) assert.deepEqual(call.decoded.workspace.local[name], before[store]);
      assert.deepEqual(call.decoded.workspace.displayContext.projectionPending, f.meta(before, "projectionPending") ?? null);
      assert.deepEqual(call.decoded.workspace.neverSent, f.meta(before, "deliveryProof") ?? {});
    }
  }
  assert.deepEqual(f.meta(after, "timerOwner") ?? null, calls.at(-1).completeReturn.ownership);
  assert.deepEqual(nativeWrites.writes, calls.flatMap((call) => call.completeReturn.ownershipWrites));
  if (!intentional) {
    assert.deepEqual(completeReturn, originalReturn);
    assert.deepEqual(after, originalAfter);
    assert.deepEqual(nativeWrites.writes, originalWrites.writes);
  }
  const receipt = { method, args: args.map((value) => JSON.parse(JSON.stringify(value))), before, originalReturn,
    originalAfter, originalOwnerWrites: originalWrites.writes, completeReturn, after,
    ownerWrites: nativeWrites.writes, calls, intentional, baselineError: current.baselineError };
  receipts.push(receipt);
  return receipt;
}

const renewals = [
  ["missing retained foreign Start", null, {}],
  ["same tab live", { ...seed.owner, leaseExpiresAtMs: f.nowMs + 31000 }, {}],
  ["renamed tab live", { ...seed.owner, leaseExpiresAtMs: f.nowMs + 31000 }, { tabId: "renamed", nowMs: f.nowMs + 30999 }],
  ["renamed tab exact expiry", { ...seed.owner, leaseExpiresAtMs: f.nowMs + 31000 }, { tabId: "renamed", nowMs: f.nowMs + 31000 }],
  ["renamed tab past expiry", { ...seed.owner, leaseExpiresAtMs: f.nowMs + 31000 }, { tabId: "renamed", nowMs: f.nowMs + 31001 }],
  ["foreign device expired", { ...seed.owner, deviceId: "foreign", leaseExpiresAtMs: 0 }, {}],
  ["null lease", { ...seed.owner, leaseExpiresAtMs: null }, { tabId: "renamed" }],
];

for (const length of [4, 2]) test(`populated owner tuple ${length} rejects without IDB normalization`, async (t) => {
  const current = await prepared(t);
  const database = current.client.use.database();
  const corrupt = ["ownership-timer", "device-local", "tab-local", f.nowMs + 31000].slice(0, length);
  await f.seedMeta(database, { timerOwner: corrupt });
  const before = await f.dump(database);
  const originalWrites = captureOwnerWrites(database);
  const originalReturn = await f.storage.renewTimerOwnership(database, input(current.client));
  originalWrites.restore();
  const originalAfter = await f.dump(database);
  assert.equal(originalReturn, false);
  assert.deepEqual(originalWrites.writes, []);
  assert.deepEqual(originalAfter, before);
  const start = a.calls.length;
  const nativeWrites = captureOwnerWrites(database);
  let completeReturn, error;
  try { completeReturn = await compiled.storage.renewTimerOwnership(database, input(current.client)); }
  catch (caught) { error = { name: caught.name, message: caught.message }; }
  finally { nativeWrites.restore(); }
  const after = await f.dump(database);
  const calls = a.calls.slice(start);
  const receipt = { case: t.name, before, originalReturn, originalAfter,
    originalOwnerWrites: originalWrites.writes, completeReturn, error, after, ownerWrites: nativeWrites.writes, calls };
  receipts.push(receipt);
  assert.ok(error, "new Core must reject the raw array before an owner write");
  assert.equal(completeReturn, undefined);
  assert.deepEqual(nativeWrites.writes, []);
  assert.deepEqual(after, before);
  assert.deepEqual(calls[0].decoded.ownership, f.meta(before, "timerOwner"));
  assert.equal(JSON.parse(calls[0].rejectedEnvelopeRaw).ok, false);
  database.close(); const reopened = await current.open();
  receipt.reopened = await f.dump(reopened.use.database());
  assert.deepEqual(receipt.reopened, before);
});

for (const [name, owner, extra] of renewals) test(`actual frozen renewal parity: ${name}`, async (t) => {
  const current = await prepared(t);
  if (owner) await f.seedMeta(current.client.use.database(), { timerOwner: owner });
  const receipt = await compare(current, "renewTimerOwnership", [input(current.client, extra)]);
  current.client.use.database().close();
  const reopened = await current.open();
  assert.deepEqual(await f.dump(reopened.use.database()), receipt.after);
  receipt.reopened = await f.dump(reopened.use.database());
});

async function incoming(current, device, status = "running") {
  const records = await f.dump(current.client.use.database());
  const timer = f.project(current.core, records, f.nowMs).canonicalTimer;
  return f.snapshot({ revision: 4, canonicalTimer: { ...timer, status, startedByDeviceId: device } });
}

for (const device of ["device-local", "device-foreign"]) test(`actual canonical install parity: ${device}`, async (t) => {
  const current = await prepared(t);
  const snapshot = await incoming(current, device, "paused");
  const args = { ...input(current.client), snapshot, hlc: { wallMs: f.nowMs + 1000, counter: 0 },
    queueIds: Object.fromEntries(Object.keys(a.stores).map((name) => [name, []])),
    timerOwnerClaim: input(current.client) };
  const receipt = await compare(current, "applySyncResponse", [args]);
  assert.equal(Boolean(f.meta(receipt.after, "timerOwner")), device === "device-local");
  current.client.use.database().close();
  const reopened = await current.open();
  assert.deepEqual(await f.dump(reopened.use.database()), receipt.after);
  receipt.reopened = await f.dump(reopened.use.database());
});

test("canonical install preserves valid foreign owner record without renewing", async (t) => {
  const current = await prepared(t);
  const owner = { ...seed.owner, deviceId: "foreign-owner", tabId: "old-tab", leaseExpiresAtMs: 0 };
  await f.seedMeta(current.client.use.database(), { timerOwner: owner });
  const snapshot = await incoming(current, "device-local", "paused");
  const receipt = await compare(current, "applySyncResponse", [{ ...input(current.client), snapshot,
    hlc: { wallMs: f.nowMs + 1000, counter: 0 }, queueIds: Object.fromEntries(Object.keys(a.stores).map((name) => [name, []])),
    timerOwnerClaim: input(current.client) }]);
  assert.deepEqual(f.meta(receipt.after, "timerOwner"), owner);
  assert.deepEqual(receipt.calls[0].completeReturn.ownershipWrites, []);
});

test("actual V3 claim acceptance prunes acknowledged Start owner and preserves complete return", async (t) => {
  const current = await prepared(t);
  const database = current.client.use.database();
  const deviceId = current.client.state.deviceId;
  const initial = await f.dump(database);
  initial.pending[0].deviceId = deviceId;
  await restore(database, initial);
  await f.seedMeta(database, { deliveryProof: { commands: [initial.pending[0].id] },
    timerOwner: { ...seed.owner, deviceId, leaseExpiresAtMs: f.nowMs + 31000 } });
  const claimed = await f.storage.claimWorkspaceBatch(database, { ...current.client.use.captureAccountContext(),
    deviceId, localNowMs: f.nowMs });
  const captured = await f.dump(database);
  const saved = f.meta(captured, "outgoingSync");
  assert.deepEqual(saved, claimed.claim);
  const snapshot = f.snapshot({ revision: 4 });
  const response = { ...snapshot, acknowledgements: saved.sent.commands.map((row) => ({ commandId: row.id, outcome: "rejected", reason: "stale" })),
    taskAcknowledgements: [], durationAcknowledgements: [], autoStartAcknowledgements: [], selectedTaskAcknowledgements: [],
    serverHlcWallMs: f.nowMs + 1000, serverHlcCounter: 0 };
  const receipt = await compare(current, "applySyncResponse", [{ ...input(current.client, { deviceId }), snapshot,
    capturedClaim: saved, hlc: { wallMs: f.nowMs + 1000, counter: 0 }, serverHlc: { wallMs: f.nowMs + 1000, counter: 0 },
    queueIds: Object.fromEntries(Object.keys(a.stores).map((name) => [name, saved.sent[name].map((row) => row.id)])),
    timerOwnerClaim: input(current.client, { deviceId }), reconciliation: { response, sent: saved.sent, deviceId } }], true);
  assert.deepEqual(receipt.completeReturn, receipt.originalReturn);
  assert.deepEqual(receipt.after, { ...receipt.originalAfter,
    meta: receipt.originalAfter.meta.filter((row) => row.key !== "timerOwner") });
  assert.deepEqual(receipt.originalOwnerWrites, []);
  assert.equal(receipt.completeReturn.applied, true);
  assert.equal(f.meta(receipt.after, "timerOwner"), undefined);
  assert.deepEqual(receipt.after.pending, []);
  assert.equal(f.meta(receipt.after, "outgoingSync"), undefined);
  assert.deepEqual(receipt.calls[0].completeReturn.ownershipWrites, [{ kind: "removeTimerOwner" }]);
  receipt.rawResponse = JSON.stringify(response);
  receipt.captured = captured;
  database.close(); const reopened = await current.open();
  receipt.reopened = await f.dump(reopened.use.database());
  assert.deepEqual(receipt.reopened, receipt.after);
});

test("stale canonical response claims only actual stored timer and keeps full returned snapshot", async (t) => {
  const current = await prepared(t);
  const stored = await incoming(current, "device-local", "paused");
  stored.revision = 5;
  await f.seedMeta(current.client.use.database(), { snapshot: stored });
  const snapshot = f.snapshot({ revision: 4 });
  const receipt = await compare(current, "applySyncResponse", [{ ...input(current.client), snapshot,
    hlc: { wallMs: f.nowMs + 1000, counter: 0 }, queueIds: Object.fromEntries(Object.keys(a.stores).map((name) => [name, []])),
    timerOwnerClaim: input(current.client) }]);
  assert.equal(receipt.completeReturn.stale, true);
  assert.deepEqual(receipt.completeReturn.snapshot, stored);
  assert.equal(f.meta(receipt.after, "timerOwner").timerId, stored.canonicalTimer.id);
});

for (const device of ["device-local", "device-foreign"]) test(`actual Keep Remote bootstrap parity: ${device}`, async (t) => {
  const current = await prepared(t);
  const database = current.client.use.database();
  const snapshot = await incoming(current, device, "paused");
  await f.storage.acquireBootstrapGate(database, { token: "bootstrap-tab", nowMs: f.nowMs, leaseMs: 300000 });
  const pending = await f.storage.captureResolution(database, { userId: f.sync.accountOwnerId(f.user),
    requestId: "owner-bootstrap", deviceId: "device-local", expectedRevision: 3, strategy: "keep_remote" }, { gateToken: "bootstrap-tab" });
  const canonical = { ...input(current.client), snapshot, hlc: { wallMs: f.nowMs + 1000, counter: 0 }, timerOwnerClaim: input(current.client) };
  const receipt = await compare(current, "applyResolution", [pending, canonical]);
  assert.equal(Boolean(f.meta(receipt.after, "timerOwner")), device === "device-local");
  current.client.use.database().close();
  const reopened = await current.open();
  assert.deepEqual(await f.dump(reopened.use.database()), receipt.after);
  receipt.reopened = await f.dump(reopened.use.database());
});

test("peer-applied bootstrap stale success claims from stored canonical record", async (t) => {
  const current = await prepared(t);
  const stored = await incoming(current, "device-local", "paused");
  await f.seedMeta(current.client.use.database(), { snapshot: stored });
  const pending = { userId: f.sync.accountOwnerId(f.user), payload: { deviceId: "device-local", strategy: "keep_remote" } };
  const receipt = await compare(current, "applyResolution", [pending, { snapshot: stored,
    hlc: { wallMs: f.nowMs + 1000, counter: 0 }, timerOwnerClaim: input(current.client) }]);
  assert.equal(receipt.completeReturn.staleSuccess, true);
  assert.deepEqual(receipt.completeReturn.snapshot, stored);
});

test("concurrent renewed tabs serialize actual IDB lock and preserve one live owner", async (t) => {
  const current = await prepared(t);
  const database = current.client.use.database();
  const before = await f.dump(database);
  const args = [input(current.client, { tabId: "first" }), input(current.client, { tabId: "second" })];
  const originalReturn = await Promise.all(args.map((raw) => f.storage.renewTimerOwnership(database, raw)));
  const originalAfter = await f.dump(database);
  await restore(database, before);
  const start = a.calls.length;
  const completeReturn = await Promise.all(args.map((raw) => compiled.storage.renewTimerOwnership(database, raw)));
  const after = await f.dump(database);
  assert.deepEqual(completeReturn, [true, false]);
  assert.deepEqual(completeReturn, originalReturn);
  assert.deepEqual(after, originalAfter);
  assert.equal(f.meta(after, "timerOwner").tabId, "first");
  receipts.push({ case: t.name, before, originalReturn, originalAfter, completeReturn, after, calls: a.calls.slice(start) });
});

test("stale renewal preserves actual claim and terminal pruning has explicit old-source difference", async (t) => {
  const current = await prepared(t);
  const stale = await compare(current, "renewTimerOwnership", [input(current.client, { timerId: "stale" })]);
  assert.equal(stale.originalReturn, false);
  assert.ok(f.meta(stale.originalAfter, "timerOwner"));
  assert.equal(stale.calls[0].completeReturn.ownershipWrites[0].timerId, "ownership-timer");
  await f.seedMeta(current.client.use.database(), { timerOwner: { ...seed.owner, leaseExpiresAtMs: f.nowMs + 31000 } });
  await f.seedQueues(current.client.use.database(), { commands: [{ ...current.start, id: "finish", type: "finish",
    deviceSequence: 2, hlcCounter: 1, observedElapsedMs: 1000 }] });
  const terminal = await compare(current, "renewTimerOwnership", [input(current.client)], true);
  assert.equal(terminal.originalReturn, true);
  assert.equal(terminal.completeReturn, false);
  assert.deepEqual(terminal.calls[0].completeReturn.ownershipWrites, [{ kind: "removeTimerOwner" }]);
});

test("raw malformed owner and expiry overflow abort complete IDB write group", async (t) => {
  const current = await prepared(t);
  const database = current.client.use.database();
  for (const owner of [false, { ...seed.owner, leaseExpiresAtMs: "0" }, { ...seed.owner, unknown: true }]) {
    await f.seedMeta(database, { timerOwner: owner });
    const before = await f.dump(database);
    await assert.rejects(compiled.storage.renewTimerOwnership(database, input(current.client)));
    assert.deepEqual(await f.dump(database), before);
  }
  await f.seedMeta(database, { timerOwner: null });
  const before = await f.dump(database);
  await assert.rejects(compiled.storage.renewTimerOwnership(database, input(current.client, { leaseMs: 9007199254740991 })), /lease expiry overflow/);
  assert.deepEqual(await f.dump(database), before);
  receipts.push({ case: t.name, before, after: await f.dump(database) });
});

async function rejection(operation) {
  try { await operation(); assert.fail("transaction unexpectedly committed"); }
  catch (error) { return { name: error.name, message: error.message }; }
}

for (const method of ["renewTimerOwnership", "applySyncResponse", "applyResolution"]) {
  test(`actual ${method} owner-write abort and cold reopen retain all stores`, async (t) => {
    const current = await prepared(t);
    const database = current.client.use.database();
    const snapshot = await incoming(current, "device-local", "paused");
    let args = [input(current.client)];
    if (method === "applySyncResponse") args = [{ ...input(current.client), snapshot,
      hlc: { wallMs: f.nowMs + 1000, counter: 0 }, queueIds: Object.fromEntries(Object.keys(a.stores).map((name) => [name, []])),
      timerOwnerClaim: input(current.client) }];
    if (method === "applyResolution") {
      await f.storage.acquireBootstrapGate(database, { token: "bootstrap-tab", nowMs: f.nowMs, leaseMs: 300000 });
      const pending = await f.storage.captureResolution(database, { userId: f.sync.accountOwnerId(f.user),
        requestId: "rollback-bootstrap", deviceId: "device-local", expectedRevision: 3, strategy: "keep_remote" }, { gateToken: "bootstrap-tab" });
      args = [pending, { snapshot, hlc: { wallMs: f.nowMs + 1000, counter: 0 }, timerOwnerClaim: input(current.client) }];
    }
    const before = await f.dump(database);
    const transaction = database.transaction.bind(database);
    t.mock.method(database, "transaction", (...parameters) => {
      const current = transaction(...parameters);
      if (parameters[1] === "readwrite") {
        const store = current.objectStore("meta"); const put = store.put.bind(store);
        t.mock.method(store, "put", (row) => { const returned = put(row); if (row.key === "timerOwner") current.abort(); return returned; });
      }
      return current;
    });
    const originalError = await rejection(() => f.storage[method](database, ...args));
    assert.deepEqual(await f.dump(database), before);
    const start = a.calls.length;
    const error = await rejection(() => compiled.storage[method](database, ...args));
    assert.deepEqual(error, originalError);
    const after = await f.dump(database);
    assert.deepEqual(after, before);
    database.close(); const reopened = await current.open();
    assert.deepEqual(await f.dump(reopened.use.database()), before);
    receipts.push({ case: t.name, before, originalError, error, after,
      reopened: await f.dump(reopened.use.database()), calls: a.calls.slice(start) });
  });
}

test("signed-out offline renewal uses local records without authentication prompt", async (t) => {
  const current = await prepared(t);
  current.client.state.user = null;
  current.client.state.localOwnerId = null;
  current.client.state.authenticated = false;
  await f.seedMeta(current.client.use.database(), { snapshot: null });
  const receipt = await compare(current, "renewTimerOwnership", [input(current.client, { expectedUserId: null })]);
  assert.equal(receipt.completeReturn, true);
  assert.equal(f.meta(receipt.after, "snapshot"), null);
  assert.deepEqual(current.client.notices, []);
});

test("account issuer rejects stale renewal before Core and cannot write replacement account", async (t) => {
  const current = await prepared(t);
  const database = current.client.use.database();
  const captured = input(current.client);
  await f.seedMeta(database, { snapshot: f.snapshot({ user: { ...f.user, id: "replacement", accountIncarnation: "d".repeat(64) } }) });
  const before = await f.dump(database);
  const start = a.calls.length;
  await assert.rejects(compiled.storage.renewTimerOwnership(database, captured), { name: "AccountOwnershipError" });
  assert.deepEqual(await f.dump(database), before);
  assert.equal(a.calls.length, start);
  receipts.push({ case: t.name, before, after: await f.dump(database), ownershipDispatches: 0 });
});

test("actual heartbeat passes local wall clock and survives window teardown outside Core", async (t) => {
  const current = await prepared(t);
  current.client.state.timer = (await incoming(current, "device-local")).canonicalTimer;
  const dispatched = [];
  const external = { ...current.client.external, syncStorage: { ...compiled.storage,
    renewTimerOwnership(database, raw) { dispatched.push({ nowMs: raw.nowMs, leaseMs: raw.leaseMs, tabId: raw.tabId });
      return compiled.storage.renewTimerOwnership(database, raw); } } };
  Object.assign(current.client.use, require(a.server + "/web/app-actions.js").create({ state: current.client.state, external, use: current.client.use }));
  current.client.use.trustedNow = () => f.nowMs + 999999;
  const context = current.client.use.captureDatabaseContext();
  const returned = await current.client.use.heartbeatTimerOwnership(context);
  assert.equal(returned, true);
  assert.equal(dispatched[0].nowMs, Date.now());
  const before = await f.dump(current.client.use.database());
  current.client.use.setDatabaseForTest(null);
  assert.equal(await current.client.use.heartbeatTimerOwnership(context), undefined);
  assert.equal(dispatched.length, 1);
  const after = await f.dump(context.database);
  assert.deepEqual(after, before);
  context.database.close();
  receipts.push({ case: t.name, dispatched, completeReturn: returned, before, after });
});
