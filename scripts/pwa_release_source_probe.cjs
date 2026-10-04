"use strict";

const test = require("node:test"), assert = require("node:assert/strict"), fs = require("node:fs");
const crypto = require("node:crypto");
const a = require("./pwa_release_source_adapter.cjs");
const f = require(a.server + "/web/test/p222-completion-fixture.js");
const seed = JSON.parse(fs.readFileSync(__dirname + "/../fixtures/pwa-ownership-plan-v1.json", "utf8"));
const matrix = JSON.parse(fs.readFileSync(__dirname + "/../fixtures/pwa-ownership-release-v1.json", "utf8"));
const receipts = [];

async function restore(database, records) {
  const tx = database.transaction(f.stores, "readwrite");
  for (const [name, rows] of Object.entries(records)) {
    const store = tx.objectStore(name); store.clear(); rows.forEach((row) => store.put(row));
  }
  await f.storage.transactionDone(tx);
}

async function prepared(t) {
  const current = await f.fixture(t), official = current.core.call.bind(current.core);
  const raw = { ...seed.request, action: { kind: "release" }, clock: { nowMs: f.nowMs } };
  let baselineError;
  try { official("workspace.ownershipPlan.v1", raw); assert.fail("official 0.45 must reject the new release action"); }
  catch (error) { baselineError = error.message; }
  assert.equal(baselineError, "invalid shared-core input: action.kind must be a JSON string enum");
  const artifactSha256 = crypto.createHash("sha256").update(fs.readFileSync(a.server + "/web/pomodorough_core.wasm")).digest("hex");
  assert.equal(artifactSha256, "845090328b2f44056480c3930e9bb684a3874b8f3cbcbd4253ddd92f67c6f5d6");
  t.mock.method(current.core, "call", a.native);
  const adapters = a.adapters(current.core), database = current.client.use.database();
  const command = { ...seed.request.workspace.local.commands[0], occurredAt: new Date(f.nowMs).toISOString(), hlcWallMs: f.nowMs };
  await f.seedQueues(database, { commands: [command], taskOperations: [{ id: "task", deviceId: "device-local", type: "delete", taskId: "task-id", title: "Retained", occurredAt: command.occurredAt, hlcWallMs: f.nowMs, hlcCounter: 0 }],
    durationOperations: [{ id: "duration", deviceId: "device-local", phase: "focus", durationMs: 1800000, occurredAt: command.occurredAt, hlcWallMs: f.nowMs, hlcCounter: 0 }],
    autoStartOperations: [{ id: "auto", deviceId: "device-local", enabled: false, occurredAt: command.occurredAt, hlcWallMs: f.nowMs, hlcCounter: 0 }],
    selectedTaskOperations: [{ id: "selection", deviceId: "device-local", taskId: null, occurredAt: command.occurredAt, hlcWallMs: f.nowMs, hlcCounter: 0 }] });
  await f.seedMeta(database, { timerOwner: { ...seed.owner, leaseExpiresAtMs: f.nowMs + 31000 },
    timerDependencies: [], deliveryProof: { commands: [command.id], taskOperations: ["task"], durationOperations: ["duration"], autoStartOperations: ["auto"], selectedTaskOperations: ["selection"] },
    canonicalHead: null, projectionPending: null, completionState: { retained: ["completion"] },
    workspaceObservation: { continuityId: "retained" }, outgoingSync: { body: "exact saved body", sent: {} } });
  return { ...current, adapters, database, baselineError, baselineRequest: raw, artifactSha256 };
}

function input(current, extra = {}) {
  return { ...current.client.use.captureDatabaseContext(), expectedUserId: f.sync.accountOwnerId(f.user),
    deviceId: "device-local", tabId: "tab-local", nowMs: f.nowMs + 1000, ...extra };
}

async function execute(storage, database, raw) {
  const captured = a.ownerWrites(database);
  try {
    const returned = await storage.releaseTimerOwnership(database, raw);
    return { returnWasUndefined: returned === undefined, completeReturn: returned, ownerWrites: captured.writes, after: await f.dump(database) };
  } catch (error) {
    return { error: { name: error.name, message: error.message }, ownerWrites: captured.writes, after: await f.dump(database) };
  } finally { captured.restore(); }
}

function assertObservation(call, before, raw) {
  assert.deepEqual(call.decoded, JSON.parse(call.inputRaw));
  assert.deepEqual(call.observed.results.timerOwner, before.meta.find((row) => row.key === "timerOwner"));
  assert.deepEqual(call.decoded.ownership, f.meta(before, "timerOwner") ?? null);
  for (const [domain, store] of Object.entries(a.stores)) {
    assert.deepEqual(call.observed.results[domain], before[store]);
    assert.deepEqual(call.decoded.workspace.local[domain], before[store]);
  }
  assert.deepEqual(call.decoded.workspace.base, require(a.server + "/web/workspace-core.js").base(f.meta(before, "snapshot")));
  assert.deepEqual(call.decoded.workspace.neverSent, f.meta(before, "deliveryProof"));
  assert.deepEqual(call.decoded.workspace.timerDependencies, f.meta(before, "timerDependencies"));
  assert.deepEqual(call.decoded.workspace.canonicalHead, f.meta(before, "canonicalHead"));
  assert.deepEqual(call.decoded.workspace.displayContext, { profile: "pwaStorage", projectionPending: f.meta(before, "projectionPending") });
  assert.deepEqual(call.decoded.clock, { nowMs: raw.nowMs });
  assert.equal(call.decoded.localDeviceId, raw.deviceId); assert.equal(call.decoded.localTabId, raw.tabId);
}

async function compare(current, raw, rejected = false) {
  const before = await f.dump(current.database), outputs = {};
  for (const name of ["original", "frozen", "current", "migrated"]) {
    await restore(current.database, before);
    outputs[name] = await execute(current.adapters[name], current.database, raw);
  }
  assert.deepEqual(outputs.current, outputs.original, "original/current full release parity");
  assert.deepEqual(outputs.frozen, outputs.original, "original/frozen full release parity");
  const call = a.calls.at(-1); assertObservation(call, before, raw);
  if (!rejected) {
    assert.deepEqual(outputs.migrated, outputs.current, "Core transport full method parity");
    assert.deepEqual(outputs.migrated.ownerWrites, call.completeReturn.ownershipWrites);
    assert.deepEqual(call.completeReturn.workspace, call.decoded.workspace);
    if (outputs.migrated.error) assert.deepEqual(outputs.migrated.after, before);
    else assert.deepEqual(f.meta(outputs.migrated.after, "timerOwner") ?? null, call.completeReturn.ownership);
  } else {
    assert.ok(outputs.migrated.error); assert.deepEqual(outputs.migrated.ownerWrites, []);
    assert.deepEqual(outputs.migrated.after, before);
  }
  const unrelated = (records) => ({ ...records, meta: records.meta.filter((row) => row.key !== "timerOwner") });
  assert.deepEqual(unrelated(outputs.migrated.after), unrelated(before));
  current.database.close(); const reopened = await current.open();
  const reopen = await f.dump(reopened.use.database()); assert.deepEqual(reopen, outputs.migrated.after);
  const receipt = { case: raw.case, before, ...outputs, call, reopen, baselineError: current.baselineError,
    baselineRequest: current.baselineRequest, artifactSha256: current.artifactSha256 };
  receipts.push(receipt); return receipt;
}

for (const item of matrix.successes) test(`full production release parity ${item.name}`, async (t) => {
  const current = await prepared(t), raw = input(current, { case: item.name });
  let owner = Object.hasOwn(item, "owner") ? item.owner : { ...seed.owner, leaseExpiresAtMs: f.nowMs + 31000 };
  const records = await f.dump(current.database);
  for (const [path, originalValue] of Object.entries(item.set ?? {})) {
    const value = typeof originalValue === "number" && originalValue >= 1784548800000 && originalValue <= 1784550400000
      ? f.nowMs + originalValue - 1784548800000 : originalValue;
    if (path.startsWith("ownership.")) owner[path.slice(10)] = value;
    else if (path === "clock.nowMs") raw.nowMs = value;
    else if (path === "workspace.local.commands") {
      records.pending = value; records.meta.find((row) => row.key === "deliveryProof").value.commands = [];
    } else if (path === "workspace.local.commands.0.extension.number") records.pending[0].extension.number = value;
    else assert.fail(`source fixture patch not applied: ${path}`);
  }
  for (const path of item.omit ?? []) delete owner[path.slice(10)];
  records.meta.find((row) => row.key === "timerOwner").value = owner;
  await restore(current.database, records);
  const receipt = await compare(current, raw);
  assert.equal(receipt.migrated.ownerWrites.length, item.writes);
  assert.equal(receipt.call.completeReturn.reason, item.reason);
});

for (const owner of [false, ["ownership-timer", "device-local"], { ...seed.owner, leaseExpiresAtMs: "0" },
  { ...seed.owner, extra: null }, { ...seed.owner, timerId: "" }]) test(`raw malformed owner rejects ${JSON.stringify(owner)}`, async (t) => {
  const current = await prepared(t); await f.seedMeta(current.database, { timerOwner: owner });
  await compare(current, input(current, { case: t.name }), true);
});

test.after(() => {
  if (process.env.CORE_PAGEHIDE_EVIDENCE) fs.writeFileSync(process.env.CORE_PAGEHIDE_EVIDENCE,
    JSON.stringify({ sources: a.sources, receipts }, null, 2) + "\n");
});

test("release owner-write abort preserves the whole transaction through cold reopen", async (t) => {
  const current = await prepared(t), before = await f.dump(current.database);
  const transaction = current.database.transaction;
  current.database.transaction = function (...args) {
    const tx = transaction.apply(this, args);
    if (args[1] === "readwrite") {
      const store = tx.objectStore("meta"), put = store.put.bind(store);
      store.put = (row) => { const result = put(row); if (row.key === "timerOwner") tx.abort(); return result; };
    }
    return tx;
  };
  const outputs = {};
  for (const name of ["original", "frozen", "current", "migrated"]) {
    outputs[name] = await execute(current.adapters[name], current.database, input(current));
    assert.deepEqual(outputs[name].after, before);
  }
  current.database.transaction = transaction;
  assert.ok(outputs.migrated.error);
  assert.deepEqual(outputs.migrated, outputs.current); assert.deepEqual(outputs.current, outputs.original);
  const call = a.calls.at(-1); assertObservation(call, before, input(current));
  assert.deepEqual(outputs.migrated.ownerWrites, call.completeReturn.ownershipWrites);
  current.database.close(); const reopened = await current.open();
  const reopen = await f.dump(reopened.use.database()); assert.deepEqual(reopen, before);
  receipts.push({ case: t.name, before, outputs, call, reopen });
});

for (const boundary of ["stored-account", "assert-current", "after-core"]) test(`account guard ${boundary} rejects release without B writes`, async (t) => {
  const current = await prepared(t), raw = input(current, { case: t.name });
  if (boundary === "stored-account") await f.seedMeta(current.database, { snapshot: f.snapshot({ user: { ...f.user, id: "account-B", accountIncarnation: "d".repeat(64) } }) });
  if (boundary === "assert-current") raw.assertCurrent = () => { throw new current.adapters.current.AccountOwnershipError(); };
  const before = await f.dump(current.database), state = structuredClone(current.client.state), start = a.calls.length;
  let outputs;
  if (boundary === "after-core") {
    let checked = 0;
    raw.assertCurrent = () => { if (++checked === 3) throw new current.adapters.current.AccountOwnershipError(); };
    outputs = await execute(current.adapters.migrated, current.database, raw);
    assert.equal(a.calls.length, start + 1);
  } else {
    outputs = await execute(current.adapters.current, current.database, raw);
    const migrated = await execute(current.adapters.migrated, current.database, raw);
    assert.deepEqual(migrated, outputs); assert.equal(a.calls.length, start);
  }
  assert.equal(outputs.error.name, "AccountOwnershipError");
  assert.deepEqual(outputs.ownerWrites, []); assert.deepEqual(outputs.after, before);
  assert.deepEqual(current.client.state, state);
  current.database.close(); const reopened = await current.open();
  const reopen = await f.dump(reopened.use.database()); assert.deepEqual(reopen, before);
  receipts.push({ case: t.name, before, outputs, reopen, ownershipDispatches: a.calls.length - start });
});

test("release during bootstrap preserves gate and pending resolution metadata", async (t) => {
  const current = await prepared(t);
  await f.seedMeta(current.database, { bootstrapGate: { token: "retained", expiresAtMs: f.nowMs + 30000 },
    bootstrapResolution: { payload: { requestId: "retained", body: "exact" } } });
  await compare(current, input(current, { case: t.name }));
});

for (const status of ["pause", "finish", "cancel", "clear", "replacement"]) test(`release never prunes ${status} timer`, async (t) => {
  const current = await prepared(t), before = await f.dump(current.database), start = before.pending[0];
  const terminal = { ...start, id: `terminal-${status}`, deviceSequence: 2, hlcCounter: 1,
    type: status === "replacement" ? "start" : status, timerId: status === "replacement" ? "new-timer" : start.timerId };
  await f.seedQueues(current.database, { commands: [terminal] });
  await compare(current, input(current, { case: t.name }));
});

for (const offset of [-1, 0, 1]) test(`release observes actual stored peer expiry ${offset}`, async (t) => {
  const current = await prepared(t), before = await f.dump(current.database), expiry = f.meta(before, "timerOwner").leaseExpiresAtMs;
  await compare(current, input(current, { case: t.name, tabId: "peer", nowMs: expiry + offset }));
});

for (const nowMs of [-1, 1.5, 9007199254740992]) test(`release refuses raw clock ${nowMs} without normalization`, async (t) => {
  const current = await prepared(t);
  await compare(current, input(current, { case: t.name, nowMs }), true);
});

test("absent persisted owner stays absent without an ownership claim", async (t) => {
  const current = await prepared(t), records = await f.dump(current.database);
  records.meta = records.meta.filter((row) => row.key !== "timerOwner");
  await restore(current.database, records);
  const receipt = await compare(current, input(current, { case: t.name }));
  assert.deepEqual(receipt.migrated.ownerWrites, []);
  assert.equal(f.meta(receipt.migrated.after, "timerOwner"), undefined);
});

test("release permits actual peer renewal at its returned expiry without queue changes", async (t) => {
  const current = await prepared(t), before = await f.dump(current.database), outputs = {};
  for (const name of ["current", "migrated"]) {
    await restore(current.database, before);
    const release = await execute(current.adapters[name], current.database, input(current));
    assert.equal(release.returnWasUndefined, true);
    const owner = f.meta(release.after, "timerOwner"), captured = a.ownerWrites(current.database);
    const returned = await current.adapters.current.renewTimerOwnership(current.database,
      input(current, { timerId: owner.timerId, tabId: "reopened-peer", nowMs: owner.leaseExpiresAtMs, leaseMs: 30000 }));
    captured.restore();
    assert.equal(returned, true);
    const after = await f.dump(current.database);
    for (const store of f.stores.slice(1)) assert.deepEqual(after[store], before[store]);
    outputs[name] = { release, completeRenewalReturn: returned, renewalWrites: captured.writes, after };
  }
  assert.deepEqual(outputs.migrated, outputs.current);
  current.database.close(); const reopened = await current.open();
  const reopen = await f.dump(reopened.use.database()); assert.deepEqual(reopen, outputs.migrated.after);
  receipts.push({ case: t.name, before, outputs, reopen });
});

for (const outcome of ["success", "failure", "account-mismatch"]) test(`actual pagehide late ${outcome} preserves replacement account`, async (t) => {
  const current = await prepared(t), before = await f.dump(current.database), initialState = structuredClone(current.client.state);
  before.meta.find((row) => row.key === "timerOwner").value = { ...seed.owner,
    deviceId: initialState.deviceId, tabId: current.client.use.tabId(), leaseExpiresAtMs: f.nowMs + 30000 };
  const outputs = {};
  for (const name of ["current", "migrated"]) {
    await restore(current.database, before); Object.assign(current.client.state, structuredClone(initialState));
    outputs[name] = await latePagehide(current, current.adapters[name], outcome);
  }
  assert.deepEqual(outputs.migrated, outputs.current);
  assert.deepEqual(outputs.migrated.after, outputs.migrated.beforeB);
  assert.deepEqual(outputs.migrated.stateAfter, outputs.migrated.stateB);
  assert.deepEqual(outputs.migrated.effects, []);
  receipts.push({ case: t.name, hostOnly: true, outputs });
});

async function latePagehide(current, storage, outcome) {
  let settle, entered, pending;
  const gate = new Promise((resolve) => { settle = resolve; }), ready = new Promise((resolve) => { entered = resolve; });
  const listeners = {}, effects = [];
  const host = { ...current.client.external.host, addEventListener: (name, callback) => { listeners[name] = callback; },
    document: { addEventListener() {} }, console: { warn: (...args) => effects.push(args.map((arg) => arg?.message ?? arg)) } };
  const external = { ...current.client.external, host, syncStorage: { ...storage,
    releaseTimerOwnership(database, raw) {
      pending = storage.releaseTimerOwnership(database, raw).then(async (value) => {
        entered(); await gate;
        if (outcome === "account-mismatch") throw new storage.AccountOwnershipError();
        if (outcome === "failure") throw new Error("delayed release failure");
        return value;
      });
      return pending;
    } } };
  const view = require(a.server + "/web/app-view.js").create({ state: current.client.state, external, use: current.client.use });
  view.setupConnectivityEvents(); listeners.pagehide(); await ready;
  const user = { ...f.user, id: "account-B", accountIncarnation: "d".repeat(64) };
  await f.seedMeta(current.database, { snapshot: f.snapshot({ user }), timerOwner: { ...seed.owner, tabId: "B-tab" } });
  Object.assign(current.client.state, { user, localOwnerId: f.sync.accountOwnerId(user), sessionIdentityValidated: true });
  const beforeB = await f.dump(current.database), stateB = structuredClone(current.client.state);
  settle(); try { await pending; } catch { /* The actual pagehide catch owns this failure. */ }
  await new Promise((resolve) => setImmediate(resolve));
  return { beforeB, stateB, after: await f.dump(current.database), stateAfter: structuredClone(current.client.state), effects };
}
