"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const Module = require("node:module");
const { spawnSync, execFileSync } = require("node:child_process");
const crypto = require("node:crypto");
const server = path.resolve(__dirname, "../../server");
const f = require(server + "/web/test/p222-completion-fixture.js");
const workspaceCore = require(server + "/web/workspace-core.js");
const tx = require(server + "/web/workspace-transaction.js");
const records = [];
const officialPath = process.env.CORE_PWA07_OFFICIAL;
const oracle = process.env.CORE_PWA07_ORACLE || path.resolve(__dirname, "../target/debug/examples/artifact_parity_oracle");
const mode = process.env.CORE_PWA07_MODE || "native";
assert.ok(officialPath && process.env.CORE_PWA07_SOURCE_EVIDENCE, "official artifact and evidence path required");
assert.ok(["native", "official"].includes(mode));
const officialBytes = fs.readFileSync(officialPath);
assert.equal(crypto.createHash("sha256").update(officialBytes).digest("hex"), "895621370566284e08f03385146bdfe07b41c78212273dd8124302d05dfeaed4");
const original = Object.fromEntries(["sync-storage.js", "app-state.js", "app-storage.js", "app-actions.js"].map((name) => [name, originalModule(name)]));
test.after(() => fs.writeFileSync(process.env.CORE_PWA07_SOURCE_EVIDENCE, JSON.stringify({ mode, records }, null, 2)));

function originalModule(name) {
  const filename = server + "/web/" + name;
  const source = execFileSync("git", ["show", `50c86a2:web/${name}`], { cwd: server, encoding: "utf8" });
  const loaded = new Module(filename);
  loaded.filename = filename;
  loaded.paths = Module._nodeModulePaths(path.dirname(filename));
  loaded._compile(source, filename);
  return { api: loaded.exports, sha256: crypto.createHash("sha256").update(source).digest("hex") };
}

function native(operation, input) {
  const inputRaw = JSON.stringify(input);
  const result = spawnSync(oracle, [], { input: JSON.stringify({ operation, input: inputRaw }) + "\n", encoding: "utf8", maxBuffer: 16 * 1024 * 1024 });
  assert.equal(result.status, 0, result.stderr);
  const envelope = JSON.parse(result.stdout);
  if (!envelope.ok) throw new Error(envelope.error);
  return envelope.value;
}

async function current(t, overrides = {}) {
  const fixture = await f.fixture(t, overrides);
  assert.deepEqual(fs.readFileSync(server + "/web/pomodorough_core.wasm"), officialBytes);
  const saved = fixture.core.call;
  fixture.officialCall = saved.bind(fixture.core);
  if (mode === "native") fixture.core.call = native;
  t.after(() => { fixture.core.call = saved; });
  return fixture;
}

async function observed(client, core, method, args) {
  const before = await f.dump(client.use.database());
  const storageReturns = captureDurationReturns(client.use);
  const calls = [];
  const dispatch = core.call.bind(core);
  core.call = (operation, input) => {
    const inputRaw = JSON.stringify(input);
    const returned = dispatch(operation, input);
    calls.push({ operation, inputRaw, decoded: JSON.parse(inputRaw), completeReturn: structuredClone(returned) });
    assert.deepEqual(returned, dispatch(operation, JSON.parse(inputRaw)));
    return returned;
  };
  let completeProductionReturn;
  try { completeProductionReturn = await client.use[method](...args); }
  finally { core.call = dispatch; storageReturns.restore(); }
  const after = await f.dump(client.use.database());
  if (mode === "native") assert.deepEqual(client.notices, [], `${method}: unexpected production notice`);
  else assert.ok(client.notices.every((notice) => notice === "Change saved. Display waits for synchronization of retained work."));
  const planned = calls.find((call) => ["workspace.intent.v1", "workspace.completionMutation.v1"].includes(call.operation));
  assert.ok(planned, `${method} did not reach Core`);
  const request = planned.decoded;
  const queues = Object.fromEntries(Object.entries(tx.QUEUE_STORES).map(([domain, store]) => [domain, before[store]]));
  assert.deepEqual(request.workspace.local, queues);
  assert.deepEqual(request.workspace.base, workspaceCore.base(f.meta(before, "snapshot")));
  assert.deepEqual(request.workspace.displayContext, { profile: "pwaStorage", projectionPending: f.meta(before, "projectionPending") ?? null });
  assert.deepEqual(request.workspace.neverSent, f.sync.neverSentForQueues(f.meta(before, "deliveryProof"), queues, f.meta(before, "outgoingSync")?.sent));
  assert.deepEqual(request.workspace.canonicalHead, f.meta(before, "canonicalHead") ?? null);
  assert.deepEqual(f.meta(after, "snapshot"), f.meta(before, "snapshot"));
  assert.deepEqual(f.meta(after, "projectionPending"), planned.completeReturn.workspace.displayContext.projectionPending);
  assert.deepEqual(f.meta(after, "deliveryProof"), planned.completeReturn.workspace.neverSent);
  assert.deepEqual(f.meta(after, "outgoingSync"), f.meta(before, "outgoingSync"));
  if (method === "persistDurationOperation") assert.deepEqual(storageReturns.values, [{
    operation: planned.completeReturn.durableOperations.durationOperations[0],
    pendingDurationOperations: planned.completeReturn.workspace.local.durationOperations }]);
  return { method, args, before, after, calls, planned, completeProductionReturn,
    storageReturns: storageReturns.values, state: structuredClone(client.state), notices: structuredClone(client.notices) };
}

function captureDurationReturns(use) {
  const saved = use.persistDurationOperation;
  const values = [];
  use.persistDurationOperation = async (...args) => {
    const returned = await saved(...args);
    values.push(structuredClone(returned));
    return returned;
  };
  return { values, restore() { use.persistDurationOperation = saved; } };
}

async function originalApp(t, overrides) {
  const core = await require(server + "/web/shared-core.js").SharedCore.fromBytes(officialBytes);
  original["sync-storage.js"].api.setSharedCore(core);
  const host = { crypto: crypto.webcrypto, indexedDB: new (require(server + "/node_modules/fake-indexeddb").IDBFactory)(),
    navigator: { onLine: false }, console, sessionStorage: { getItem: () => "p222-tab-0", setItem() {} },
    setTimeout: () => 1, clearTimeout() {}, setInterval: () => 1, clearInterval() {} };
  const state = original["app-state.js"].api.createState(host);
  Object.assign(state, { ready: true, user: f.user, localOwnerId: f.sync.accountOwnerId(f.user), deviceId: "p222-device", bootstrapBlocked: false });
  const use = { render() {}, renderTimer() {}, renderTaskSelector() {}, renderSyncStatus() {}, renderDurations() {},
    scheduleSync() {}, showNotice() {}, queueSessionRevalidation() {} };
  const external = { host, syncCore: f.sync, syncStorage: original["sync-storage.js"].api, sharedCoreHost: { SharedCore: { load: async () => core } } };
  for (const name of ["app-state.js", "app-storage.js", "app-actions.js"]) Object.assign(use, original[name].api.create({ state, external, use }));
  use.trustedNow = () => f.nowMs;
  use.setDatabaseForTest(await use.openDatabase());
  t.after(() => use.database().close());
  await f.seedMeta(use.database(), { snapshot: f.snapshot(overrides), deviceId: state.deviceId, deviceSequence: 7,
    hlc: { wallMs: f.nowMs, counter: 2 }, uuidV7: f.storage.uuid7FromParts(f.nowMs, 20n), settings: { selectedPhase: "focus" } });
  await use.reloadPersistedState();
  return { use, state };
}

for (const [phase, minutes] of [["short_break", 10], ["long_break", 30]]) {
  test(`actual production offline duration and cadence ${phase}`, async (t) => {
    const overrides = { history: phase === "long_break" ? f.completedFocusHistory() : [] };
    const { client, core, open } = await current(t, overrides);
    const steps = [];
    for (const [method, args] of [["issueAutoStartOperation", [true]], ["issueDurationOperation", [phase, minutes * 60000]],
      ["issueCommand", ["start"]], ["issueCommand", ["pause"]], ["issueCommand", ["resume"]], ["finishTimer", [false]]]) {
      steps.push(await observed(client, core, method, args));
      assert.equal(steps.at(-1).completeProductionReturn, true);
    }
    const generated = steps.at(-1).planned.completeReturn.commands[1];
    assert.equal(generated.phase, phase);
    assert.equal(generated.plannedDurationMs, mode === "native" ? minutes * 60000 : minutes * 30000);
    const closed = await f.dump(client.use.database());
    client.use.database().close();
    const reopened = await open();
    await reopened.use.reloadPersistedState();
    assert.deepEqual(await f.dump(reopened.use.database()), closed);
    assert.deepEqual(reopened.state.timer, client.state.timer);
    const legacy = await originalApp(t, overrides);
    const originalReturns = [];
    for (const step of steps) originalReturns.push(await legacy.use[step.method](...step.args));
    assert.deepEqual(originalReturns, steps.map((step) => step.completeProductionReturn));
    assert.equal(legacy.state.timer.plannedDurationMs, minutes * 60000);
    assert.equal(legacy.state.timer.phase, generated.phase);
    const legacyAfter = await f.dump(legacy.use.database());
    records.push({ case: t.name, steps, closed, reopened: await f.dump(reopened.use.database()),
      originalReturns, legacyAfter, originalSourceHashes: Object.fromEntries(Object.entries(original).map(([name, module]) => [name, module.sha256])) });
  });
}

test("peer fresh false preference prevents generated Start", async (t) => {
  const { client, core, open } = await current(t, { autoStartBreaks: true });
  const started = await observed(client, core, "issueCommand", ["start"]);
  const peer = await open();
  await peer.use.reloadPersistedState();
  const disabled = await observed(peer, core, "issueAutoStartOperation", [false]);
  const finished = await observed(client, core, "finishTimer", [false]);
  assert.deepEqual(finished.planned.completeReturn.commands.map((command) => command.type), mode === "native" ? ["finish"] : ["finish", "start"]);
  assert.equal(finished.state.autoStartBreaks, mode !== "native");
  const legacy = await originalApp(t, { autoStartBreaks: true });
  const originalReturns = [await legacy.use.issueCommand("start"), await legacy.use.issueAutoStartOperation(false), await legacy.use.finishTimer(false)];
  assert.deepEqual(originalReturns, [started, disabled, finished].map((step) => step.completeProductionReturn));
  assert.equal(legacy.state.autoStartBreaks, false);
  assert.equal(legacy.state.timer.status, "completed");
  records.push({ case: t.name, started, disabled, finished, originalReturns, legacyAfter: await f.dump(legacy.use.database()) });
});

test("claimed task and selection rows permit latest display winner without new delivery proof", async (t) => {
  const { client, core, open } = await current(t);
  const title = "Claimed display task";
  const task = core.call("task.identity.v1", { title });
  const steps = [];
  steps.push(await observed(client, core, "issueTaskOperation", ["upsert", task]));
  steps.push(await observed(client, core, "issueSelectedTaskOperation", [task.id]));
  steps.push(await observed(client, core, "issueCommand", ["start"]));
  const claim = await f.storage.claimWorkspaceBatch(client.use.database(), { ...client.use.captureAccountContext(), deviceId: client.state.deviceId, localNowMs: f.nowMs });
  const closed = await f.dump(client.use.database());
  client.use.database().close();
  const cold = await open();
  await cold.use.reloadPersistedState();
  steps.push(await observed(cold, core, "issueSelectedTaskOperation", [null]));
  steps.push(await observed(cold, core, "deleteTask", [task]));
  const legacy = await originalApp(t, {});
  const originalReturns = [];
  for (const step of steps.slice(0, 3)) originalReturns.push(await legacy.use[step.method](...step.args));
  const legacyBeforeClaim = await f.dump(legacy.use.database());
  const legacySent = Object.fromEntries(Object.entries(tx.QUEUE_STORES).map(([domain, store]) => [domain, legacyBeforeClaim[store]]));
  const originalClaim = await original["sync-storage.js"].api.retireProofAndPersistOutgoing(legacy.use.database(), legacySent, { ownerId: f.sync.accountOwnerId(f.user) });
  await legacy.use.reloadPersistedState();
  for (const step of steps.slice(3)) originalReturns.push(await legacy.use[step.method](...step.args));
  if (mode === "native") assert.deepEqual(originalReturns, steps.map((step) => step.completeProductionReturn));
  assert.deepEqual(legacy.state.tasks, []);
  assert.equal(legacy.state.timer.taskId, null);
  records.push({ case: t.name, steps, claim, closed, after: await f.dump(cold.use.database()),
    originalReturns, originalClaim, legacyBeforeClaim, legacyAfter: await f.dump(legacy.use.database()) });
  assert.equal(cold.state.selectedTaskId, null);
  assert.equal(cold.state.timer.taskId, undefined);
  assert.deepEqual(cold.state.tasks, mode === "native" ? [] : [{ id: task.id, title: task.title }]);
  for (const name of Object.keys(tx.QUEUE_STORES)) for (const row of claim.claim.sent[name]) {
    assert.ok(!f.meta(steps.at(-1).after, "deliveryProof")[name].includes(row.id));
  }
});

test("original production mixed claimed duration keeps latest edit visible", async (t) => {
  const { client, core } = await current(t);
  const first = await observed(client, core, "persistDurationOperation", ["focus", 1800000]);
  const claim = await f.storage.claimWorkspaceBatch(client.use.database(), { ...client.use.captureAccountContext(), deviceId: client.state.deviceId, localNowMs: f.nowMs });
  const second = await observed(client, core, "persistDurationOperation", ["focus", 2700000]);
  assert.equal(second.state.durationsMs.focus, mode === "native" ? 2700000 : 1800000);
  assert.deepEqual(second.planned.completeReturn.retiredDurationOperationIds, []);
  const legacy = await originalApp(t, {});
  const storageReturns = captureDurationReturns(legacy.use);
  const originalReturns = [await legacy.use.issueDurationOperation("focus", 1800000)];
  const beforeClaim = await f.dump(legacy.use.database());
  const sent = Object.fromEntries(Object.entries(tx.QUEUE_STORES).map(([domain, store]) => [domain, beforeClaim[store]]));
  const originalClaim = await original["sync-storage.js"].api.retireProofAndPersistOutgoing(legacy.use.database(), sent, { ownerId: f.sync.accountOwnerId(f.user) });
  await legacy.use.reloadPersistedState();
  const claimedRows = await f.dump(legacy.use.database());
  const inFlightDurationIds = f.meta(claimedRows, "outgoingSync").sent.durationOperations.map((row) => row.id);
  legacy.use.setInFlightDurationOperationIds(inFlightDurationIds);
  originalReturns.push(await legacy.use.issueDurationOperation("focus", 2700000));
  storageReturns.restore();
  assert.deepEqual(originalReturns, [true, true]);
  assert.deepEqual(storageReturns.values, [first.completeProductionReturn, second.completeProductionReturn]);
  assert.equal(legacy.state.durationsMs.focus, 2700000);
  records.push({ case: t.name, first, claim, second, originalReturns, originalClaim, beforeClaim,
    claimedRows, inFlightDurationIds, originalStorageReturns: storageReturns.values, legacyAfter: await f.dump(legacy.use.database()) });
});

test("new display context transaction abort preserves complete persisted state", async (t) => {
  const { client, core } = await current(t);
  await client.use.issueAutoStartOperation(true);
  const before = await f.dump(client.use.database());
  const database = client.use.database();
  const transaction = database.transaction.bind(database);
  const plans = [];
  const dispatch = core.call.bind(core);
  t.mock.method(core, "call", (operation, input) => {
    const returned = dispatch(operation, input);
    if (operation === "workspace.intent.v1") plans.push({ inputRaw: JSON.stringify(input), completeReturn: returned });
    return returned;
  });
  t.mock.method(database, "transaction", (...args) => {
    const current = transaction(...args);
    if (args[1] === "readwrite") {
      const meta = current.objectStore("meta");
      const put = meta.put.bind(meta);
      t.mock.method(meta, "put", (row) => {
        const result = put(row);
        if (row.key === "projectionPending") current.abort();
        return result;
      });
    }
    return current;
  });
  const returned = await client.use.issueDurationOperation("short_break", 600000);
  assert.equal(returned, false);
  assert.equal(plans.length, 1);
  assert.deepEqual(await f.dump(client.use.database()), before);
  records.push({ case: t.name, before, after: await f.dump(client.use.database()), completeProductionReturn: returned, plans });
});

async function restore(database, records) {
  const transaction = database.transaction(Object.keys(records), "readwrite");
  for (const [name, rows] of Object.entries(records)) {
    const store = transaction.objectStore(name);
    store.clear();
    for (const row of rows) store.put(row);
  }
  await f.storage.transactionDone(transaction);
}

function installInput(http) {
  const responseRaw = http.requests.at(-1).responseRaw;
  const response = JSON.parse(responseRaw);
  const capturedClaim = f.meta(http.aborted, "outgoingSync");
  const snapshot = f.meta(http.after, "snapshot");
  const serverHlc = { wallMs: response.serverHlcWallMs, counter: response.serverHlcCounter };
  return { responseRaw, input: { capturedClaim, expectedUserId: f.sync.accountOwnerId(snapshot.user), snapshot,
    queueIds: Object.fromEntries(Object.entries(tx.QUEUE_STORES).map(([domain, store]) => [domain, http.aborted[store].map((row) => row.id)])),
    promoteCommands: [], clockOffset: null, serverHlc, hlc: serverHlc,
    reconciliation: { response, sent: capturedClaim.sent, deviceId: f.meta(http.aborted, "deviceId") } } };
}

test("actual raw V3 ACK install abort preserves claim and complete persisted rows", async (t) => {
  assert.ok(process.env.CORE_PWA07_HTTP_EVIDENCE, "actual pwa044 HTTP evidence required");
  const http = JSON.parse(fs.readFileSync(process.env.CORE_PWA07_HTTP_EVIDENCE, "utf8"));
  const { client, core, officialCall } = await current(t);
  const database = client.use.database();
  const { input, responseRaw } = installInput(http);
  await restore(database, http.aborted);
  const before = await f.dump(database);
  assert.deepEqual(before, http.aborted);
  const transaction = database.transaction.bind(database);
  database.transaction = (...args) => {
    const current = transaction(...args);
    if (args[1] === "readwrite") {
      const meta = current.objectStore("meta");
      const put = meta.put.bind(meta);
      meta.put = (row) => {
        if (row.key === "canonicalResponse") throw new Error("Injected V3 install abort");
        return put(row);
      };
    }
    return current;
  };
  await assert.rejects(f.storage.applySyncResponse(database, input), /Injected V3 install abort/);
  database.transaction = transaction;
  const aborted = await f.dump(database);
  assert.deepEqual(aborted, before);
  const nativeReturn = await f.storage.applySyncResponse(database, input);
  const nativeAfter = await f.dump(database);
  await restore(database, before);
  const saved = core.call;
  core.call = officialCall;
  const officialReturn = await f.storage.applySyncResponse(database, input);
  core.call = saved;
  const officialAfter = await f.dump(database);
  assert.deepEqual(nativeReturn, officialReturn);
  assert.deepEqual(nativeAfter, officialAfter);
  assert.deepEqual(f.meta(nativeAfter, "canonicalResponse"), JSON.parse(responseRaw));
  assert.deepEqual(f.meta(nativeAfter, "projectionPending"), f.meta(http.after, "projectionPending"));
  assert.deepEqual(f.meta(nativeAfter, "reconciledWorkspace"), f.meta(http.after, "reconciledWorkspace"));
  records.push({ case: t.name, responseRaw, input, before, aborted, nativeReturn, officialReturn, nativeAfter, officialAfter });
});
