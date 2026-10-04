"use strict";
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const Module = require("node:module");
const { spawnSync } = require("node:child_process");
const test = require("node:test");

const suite = process.env.POMODOROUGH_ROOT || path.resolve(__dirname, "../..");
const temp = process.env.PWA11_EVIDENCE_DIR || "/var/folders/r_/_mr22dqn24d31b7460cz8z5m0000gn/T/opencode";
const frozenRoot = process.env.PWA11_FROZEN_ROOT || path.join(temp, "pwa-core-045/baseline/web");
const server = path.join(suite, "server");
const f = require(path.join(server, "web/test/p222-completion-fixture.js"));
const currentStorage = require(path.join(server, "web/sync-storage.js"));
const workspace = require(path.join(server, "web/workspace-core.js"));
const receipts = [];
const operation = "workspace.legacyPreferences.v1";
const domains = Object.keys(f.sync.emptyNeverSent());
const uuids = [1, 2, 3, 4, 5].map((n) => `${String(n).repeat(8)}-${String(n).repeat(4)}-4${String(n).repeat(3)}-8${String(n).repeat(3)}-${String(n).repeat(12)}`);
const frozenCache = new Map();

function frozen(name, committed = false) {
  const key = `${name}-${committed}`;
  if (frozenCache.has(key)) return frozenCache.get(key);
  const filename = path.join(server, "web", name), module = new Module(filename);
  module.filename = filename; module.paths = Module._nodeModulePaths(path.dirname(filename));
  const source = committed ? spawnSync("git", ["show", `50c86a2:web/${name}`], { cwd: server, encoding: "utf8" }) : null;
  if (source) assert.equal(source.status, 0, source.stderr);
  module._compile(source ? source.stdout : fs.readFileSync(path.join(frozenRoot, name), "utf8"), filename);
  frozenCache.set(key, module.exports);
  return module.exports;
}

function native(input) {
  const request = { operation, input: typeof input === "string" ? input : JSON.stringify(input) };
  assert.deepEqual(JSON.parse(request.input), typeof input === "string" ? JSON.parse(input) : input);
  const command = process.env.PWA11_NATIVE_ORACLE || "rustup";
  const args = process.env.PWA11_NATIVE_ORACLE ? [] : ["run", "1.97.1", "cargo", "run", "--quiet", "--locked", "--example", "artifact_parity_oracle"];
  const result = spawnSync(command, args, {
    cwd: path.join(suite, "pomodorough-core"), encoding: "utf8", input: JSON.stringify(request) + "\n", timeout: 120000
  });
  assert.equal(result.status, 0, result.stderr);
  return { raw: request.input, envelopeRaw: result.stdout.trim(), envelope: JSON.parse(result.stdout) };
}

function rawInput(before, ownerId) {
  const input = { profile: "pwaStorage", deviceId: f.meta(before, "deviceId"),
    ownership: { ownerId, expectedOwnerId: ownerId }, settings: f.meta(before, "settings"),
    outgoing: f.meta(before, "outgoingSync") ?? null, identities: { operationUuids: uuids },
    workspace: { base: workspace.base(f.meta(before, "snapshot")),
      local: Object.fromEntries(Object.entries(require(path.join(server, "web/workspace-transaction.js")).QUEUE_STORES)
        .map(([domain, store]) => [domain, before[store]])),
      neverSent: f.meta(before, "deliveryProof") ?? {}, canonicalHead: f.meta(before, "canonicalHead") ?? null,
      timerDependencies: f.meta(before, "timerDependencies") ?? [],
      displayContext: { profile: "pwaStorage", projectionPending: f.meta(before, "projectionPending") ?? null },
      now: new Date(f.nowMs).toISOString() } };
  assert.deepEqual(input.settings, f.meta(before, "settings"));
  assert.deepEqual(input.workspace.base, workspace.base(f.meta(before, "snapshot")));
  return input;
}

async function load(database, records) {
  const transaction = database.transaction(f.stores, "readwrite");
  for (const [name, rows] of Object.entries(records)) {
    const store = transaction.objectStore(name); store.clear(); rows.forEach((row) => store.put(row));
  }
  await f.storage.transactionDone(transaction);
}

async function apply(database, input, plan, fail = false) {
  const transaction = database.transaction(f.stores, "readwrite");
  for (const [domain, store] of Object.entries(require(path.join(server, "web/workspace-transaction.js")).QUEUE_STORES)) {
    plan.operations[domain].forEach((operation) => transaction.objectStore(store).add(operation));
  }
  if (plan.writeSettings) transaction.objectStore("meta").put({ key: "settings", value: plan.settings });
  if (plan.consumedIdentityCount) {
    for (const [key, value] of [["deliveryProof", plan.workspace.neverSent], ["projectionPending", plan.workspace.displayContext.projectionPending]]) {
      transaction.objectStore("meta").put({ key, value });
    }
  }
  if (fail) transaction.abort();
  await f.storage.transactionDone(transaction);
  return plan;
}

async function oldMigrations(client, core, committed = false) {
  let cursor = 0;
  const external = { ...client.external, host: { ...client.external.host, crypto: { randomUUID: () => uuids[cursor++] } } };
  const app = frozen("app-storage.js", committed).create({ state: client.state, external, use: client.use });
  app.setDatabaseForTest(client.use.database());
  const durationReturn = await app.bootstrapLegacyDurations();
  const storage = frozen("sync-storage.js", committed); storage.setSharedCore(core);
  const context = client.use.captureAccountContext();
  const autoStartReturn = await storage.migrateLegacyAutoStart(client.use.database(), { ...context, operationId: uuids[cursor], nowMs: f.nowMs });
  if (autoStartReturn.migrated) cursor += 1;
  const selectedTaskReturn = await storage.migrateLegacySelectedTask(client.use.database(), { ...context, operationId: uuids[cursor], nowMs: f.nowMs });
  return { durationReturn: durationReturn ?? null, autoStartReturn, selectedTaskReturn };
}

const scenarios = [
  ["all-phases-explicit-false", { durations: { focus: 30, short_break: 7.5, long_break: "20" }, autoStartBreaks: false, autoStartBreaksExplicit: true }],
  ["default", { durations: { focus: 25, short_break: 5, long_break: 15 } }],
  ["null-and-unknown", { durations: { focus: null, short_break: null, unknown: 50 } }],
  ["rounded-default", { durations: { focus: 24.5, short_break: 4.5, long_break: 14.5 } }],
  ["invalid", { durations: { focus: "bad", short_break: {}, long_break: "Infinity" } }],
  ["bounds", { durations: { focus: -1, short_break: 0, long_break: 181 } }],
  ["coercion", { durations: { focus: [30.5], short_break: "0x08", long_break: true } }],
  ["whitespace-radix", { durations: { focus: "\ufeff30\u00a0", short_break: "0b1000", long_break: "0o24" } }],
  ["implicit-false", { autoStartBreaks: false, autoStartBreaksExplicit: false, selectedTaskId: null }],
  ["explicit-true", { autoStartBreaks: true }],
  ["selected-string", { selectedTaskId: "legacy-selected-task" }],
  ["completed-marker", { durations: { focus: 30 }, durationSyncBootstrapped: true,
    autoStartBreaks: true, autoStartSyncBootstrapped: true, selectedTaskId: "keep", selectedTaskSyncBootstrapped: true }],
];

for (const [name, preferences] of scenarios) test(`frozen raw legacy parity ${name}`, async (t) => {
  const { client, core, open } = await f.fixture(t);
  const database = client.use.database(), ownerId = client.state.localOwnerId;
  await f.seedMeta(database, { settings: { selectedPhase: "focus", peerOnlySetting: { keep: null }, ...preferences },
    snapshot: f.snapshot({ durationsMs: { focus: 2100000, short_break: 300000, long_break: 900000 } }),
    canonicalHead: { wallMs: f.nowMs, counter: 2 }, deliveryProof: f.sync.emptyNeverSent() });
  const before = await f.dump(database), input = rawInput(before, ownerId), result = native(input);
  assert.equal(result.envelope.ok, true, result.envelope.error);
  const plan = result.envelope.value;
  assert.deepEqual(JSON.parse(result.raw), input);
  const returned = await apply(database, input, plan), after = await f.dump(database);
  assert.deepEqual(returned, plan);
  assert.deepEqual(f.meta(after, "snapshot"), f.meta(before, "snapshot"));
  assert.equal(plan.projection.durationsMs.focus, 2100000);
  await load(database, before);
  const frozenReturn = await oldMigrations(client, core), oldAfter = await f.dump(database);
  assert.deepEqual(f.meta(after, "settings"), f.meta(oldAfter, "settings"));
  const retainedMeta = (records) => records.meta.filter((record) => !["deliveryProof", "projectionPending"].includes(record.key));
  assert.deepEqual(retainedMeta(after), retainedMeta(oldAfter));
  for (const domain of domains) {
    const original = input.workspace.neverSent[domain] || [];
    const added = plan.operations[domain].map((operation) => operation.id);
    assert.deepEqual(plan.workspace.neverSent[domain] || [], original.concat(added));
  }
  for (const store of f.stores.slice(1)) assert.deepEqual(after[store], oldAfter[store]);
  assert.deepEqual(frozenReturn, { durationReturn: null,
    autoStartReturn: { migrated: plan.operations.autoStartOperations.length > 0, operation: plan.operations.autoStartOperations[0] ?? null },
    selectedTaskReturn: { migrated: plan.operations.selectedTaskOperations.length > 0, operation: plan.operations.selectedTaskOperations[0] ?? null } });
  await load(database, before);
  const committedReturn = await oldMigrations(client, core, true), committedAfter = await f.dump(database);
  assert.deepEqual(committedReturn, frozenReturn);
  assert.deepEqual(committedAfter, oldAfter);
  await load(database, after);
  const reopened = await open(), restartBefore = await f.dump(reopened.use.database());
  const restartInput = rawInput(restartBefore, ownerId); restartInput.identities.operationUuids = [];
  const restart = native(restartInput);
  assert.equal(restart.envelope.ok, true, restart.envelope.error);
  assert.equal(restart.envelope.value.outcome, "noop");
  await apply(reopened.use.database(), restartInput, restart.envelope.value);
  assert.deepEqual(await f.dump(reopened.use.database()), restartBefore);
  receipts.push({ case: name, before, inputRaw: result.raw, decodedInput: input,
    completeNativeReturn: returned, completeFrozenReturn: frozenReturn, completeCommittedReturn: committedReturn,
    after, frozenAfter: oldAfter, committedAfter,
    restartInputRaw: restart.raw, completeRestartReturn: restart.envelope.value });
});

test("current public duration precedence is genuine red; native imports keep frozen precedence", async (t) => {
  const { client, core } = await f.fixture(t), database = client.use.database();
  const at = new (require("node:vm").runInNewContext("Date"))().getTime();
  await f.seedMeta(database, { settings: { durations: { focus: 30 }, autoStartBreaks: false,
    autoStartBreaksExplicit: true, selectedTaskId: null, selectedTaskIdExplicit: true },
    hlc: { wallMs: at, counter: 0 }, uuidV7: null });
  const before = await f.dump(database), input = rawInput(before, client.state.localOwnerId);
  const green = native(input); assert.equal(green.envelope.ok, true, green.envelope.error);
  const calls = [], original = core.call.bind(core);
  core.call = (operation, input) => { const raw = JSON.stringify(input), returned = original(operation, input);
    assert.deepEqual(JSON.parse(raw), input); calls.push({ operation, inputRaw: raw, completeReturn: returned }); return returned; };
  let returned;
  try { returned = await currentStorage.migrateLegacyDurationPreferences(database, { ...client.use.captureAccountContext(),
    deviceId: client.state.deviceId, tabId: client.use.tabId(), nowMs: at, localNowMs: at, leaseMs: 60000 }); }
  finally { core.call = original; }
  const current = (await f.dump(database)).pendingDurations[0];
  const call = calls.find((call) => call.operation === "workspace.intent.v1");
  const decoded = JSON.parse(call.inputRaw);
  assert.deepEqual(decoded.workspace.base, input.workspace.base);
  assert.deepEqual(decoded.workspace.local, input.workspace.local);
  assert.deepEqual(decoded.workspace.canonicalHead, input.workspace.canonicalHead);
  assert.deepEqual(decoded.workspace.displayContext, input.workspace.displayContext);
  assert.equal(current.hlcWallMs, at);
  assert.equal(green.envelope.value.operations.durationOperations[0].hlcWallMs, 0);
  fs.writeFileSync(path.join(temp, "core-pwa11-http-input.json"), JSON.stringify({
    current: f.sync.durationRequestOperation(current),
    native: green.envelope.value.operations.durationOperations.map(f.sync.durationRequestOperation),
    nativeAutoStart: green.envelope.value.operations.autoStartOperations,
    nativeSelection: green.envelope.value.operations.selectedTaskOperations
  }, null, 2));
  receipts.push({ case: "current-public-red", before, calls, completeProductionReturn: returned ?? null,
    current, nativeInputRaw: green.raw, completeNativeReturn: green.envelope.value });
});

test("explicit selected-task null imports despite matching default", async (t) => {
  const { client } = await f.fixture(t), database = client.use.database();
  await f.seedMeta(database, { settings: { selectedTaskId: null, selectedTaskIdExplicit: true } });
  const before = await f.dump(database), input = rawInput(before, client.state.localOwnerId), result = native(input);
  assert.equal(result.envelope.ok, true, result.envelope.error);
  assert.deepEqual(result.envelope.value.operations.selectedTaskOperations, [{ id: uuids[0], taskId: null,
    occurredAt: "1970-01-01T00:00:00.000Z", hlcWallMs: 0, hlcCounter: 0 }]);
  receipts.push({ case: "explicit-null-extension", before, inputRaw: result.raw, completeNativeReturn: result.envelope.value });
});

test("raw claims, owner errors, and aborted migration preserve all persisted inputs", async (t) => {
  const { client } = await f.fixture(t), database = client.use.database();
  const old = { id: "existing-legacy", phase: "focus", durationMs: 1800000, occurredAt: "1970-01-01T00:00:01Z",
    hlcWallMs: 0, hlcCounter: 0, ownerId: "old-tab", extension: { empty: "", explicit: null } };
  await f.seedQueues(database, { durationOperations: [old] });
  await f.seedMeta(database, { settings: { durations: { focus: 30 } }, deliveryProof: f.sync.emptyNeverSent(),
    projectionPending: f.sync.emptyNeverSent(), outgoingSync: { ownerId: client.state.localOwnerId,
      sent: { durationOperations: [old] }, body: ' { "durationOperations": [' + JSON.stringify(old) + '] } ' } });
  const before = await f.dump(database), input = rawInput(before, client.state.localOwnerId);
  for (const missingBody of [false, true]) {
    const raw = structuredClone(input); if (missingBody) delete raw.outgoing.body;
    const result = native(raw); assert.equal(result.envelope.ok, true, result.envelope.error);
    assert.deepEqual(result.envelope.value.outgoing, raw.outgoing);
    assert.deepEqual(result.envelope.value.workspace.local.durationOperations[0], old);
    assert.equal(result.envelope.value.outgoingAction, "preserve");
    receipts.push({ case: `saved-claim-missing-body-${missingBody}`, inputRaw: result.raw, completeNativeReturn: result.envelope.value });
  }
  for (const boundary of ["owner", "proof", "identity", "timestamp"]) {
    const raw = structuredClone(input);
    if (boundary === "owner") raw.ownership.expectedOwnerId = "foreign";
    if (boundary === "proof") raw.workspace.neverSent.durationOperations = [old.id];
    if (boundary === "identity") raw.identities.operationUuids = [old.id];
    if (boundary === "timestamp") delete raw.workspace.local.durationOperations[0].occurredAt;
    const result = native(raw); assert.equal(result.envelope.ok, false);
    assert.deepEqual(await f.dump(database), before);
    receipts.push({ case: `raw-error-${boundary}`, inputRaw: result.raw, completeNativeError: result.envelope, unchanged: true });
  }
  const plan = native(input).envelope.value;
  await assert.rejects(() => apply(database, input, plan, true));
  assert.deepEqual(await f.dump(database), before);
  const returned = await apply(database, input, plan);
  assert.deepEqual(returned, plan);
  receipts.push({ case: "transaction-abort-retry", before, after: await f.dump(database), completeNativeReturn: returned });
});

test.after(() => fs.writeFileSync(path.join(temp, "core-pwa11-source-parity.json"), JSON.stringify({ receipts }, null, 2)));

module.exports = { f, native, rawInput, load, apply, oldMigrations, temp, uuids };
