"use strict";

// Unchanged production repositories and IndexedDB transactions. Only clocks,
// entropy, account ports, and the native planner transport are controlled.
const fs = require("node:fs");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const assert = require("node:assert/strict");
const web = path.resolve(__dirname, "../../server/web");
const { indexedDB } = require(path.join(web, "../node_modules/fake-indexeddb"));
const storage = require(path.join(web, "sync-storage.js"));
const app = require(path.join(web, "app-storage.js"));
const mutations = require(path.join(web, "app-actions.js"));
const syncCore = require(path.join(web, "sync-core.js"));
const { SharedCore } = require(path.join(web, "shared-core.js"));
const fixture = JSON.parse(fs.readFileSync(path.join(__dirname, "../fixtures/workspace-intent-v1.json")));
const known = JSON.parse(fs.readFileSync(path.join(__dirname, "../fixtures/workspace-intent-desktop-known-tasks-v1.json"))).knownTasks[0];
const now = 1784548810000;
const domains = { commands: "pending", taskOperations: "pendingTasks", durationOperations: "pendingDurations",
  autoStartOperations: "pendingAutoStarts", selectedTaskOperations: "pendingSelectedTasks" };
const clone = structuredClone;
Object.defineProperty(globalThis, "crypto", { value: {
  subtle: require("node:crypto").webcrypto.subtle,
  getRandomValues: bytes => { bytes.fill(0); bytes[bytes.length - 1] = 1; return bytes; },
  randomUUID: () => fixture.request.identities.timerUuid
} });
Date.now = () => now;

function native(operation, input) {
  const binary = process.env.CORE_PROBE || path.join(__dirname, "../target/debug/examples/completion_policy_probe");
  const result = spawnSync(binary, [], { input: JSON.stringify({ operation, input }) + "\n", encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  const output = JSON.parse(result.stdout);
  if (output.error) throw new Error(`${operation}: ${output.error}`);
  return output;
}

async function open(name) {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(name, 1);
    request.onupgradeneeded = () => {
      for (const name of ["meta", ...Object.values(domains)]) request.result.createObjectStore(name, { keyPath: name === "meta" ? "key" : "id" });
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

async function raw(database) {
  const transaction = database.transaction(["meta", ...Object.values(domains)], "readonly");
  const [meta, ...queues] = await Promise.all([storage.requestResult(transaction.objectStore("meta").getAll()),
    ...Object.values(domains).map(name => storage.requestResult(transaction.objectStore(name).getAll()))]);
  return { meta: Object.fromEntries(meta.map(row => [row.key, row.value])),
    queues: Object.fromEntries(Object.keys(domains).map((name, index) => [name, queues[index]])) };
}

function repository(database, rows, core) {
  const projected = storage.projectState({ snapshot: rows.meta.snapshot, queues: rows.queues, nowMs: now,
    deviceId: "device-local", sharedCore: core });
  const state = { user: { id: "user-1" }, localOwnerId: "user-1", deviceId: "device-local",
    selectedPhase: "focus", timer: projected.canonicalTimer, tasks: projected.tasks, durationsMs: projected.durationsMs };
  const use = { captureAccountContext: () => ({ assertCurrent() {}, sharedCore: core }), assertExpectedAccount() {},
    trustedNow: () => now, tabId: () => "tab-a", phaseConfig: () => fixture.request.workspace.base.durationsMs,
    selectedTaskIdForNextFocus: () => projected.selectedTaskId, tr: (_key, _args, fallback) => fallback,
    elapsedFor: timer => Math.min(timer.plannedDurationMs, timer.elapsedAtAnchorMs
      + (timer.status === "running" ? Math.max(0, now - Date.parse(timer.anchorAt)) : 0)),
    compareDurationOperations: syncCore.compareDurationOperations };
  const actions = app.create({ state, external: { host: globalThis, syncStorage: storage, syncCore }, use });
  actions.setDatabaseForTest(database);
  return { actions, state, use };
}

async function invoke(database, intent, core) {
  const before = await raw(database);
  const { actions, state, use } = repository(database, before, core);
  switch (intent.kind) {
    case "addAndSelectTask": {
      Object.assign(state, { pending: clone(before.queues.commands), pendingTaskOperations: clone(before.queues.taskOperations),
        pendingSelectedTaskOperations: clone(before.queues.selectedTaskOperations), selectedTaskId: before.meta.snapshot.selectedTaskId,
        actionLocked: false });
      const calls = [], effects = [];
      const ports = { ...use, controlsBlocked: () => false,
        sharedTaskIdentity: title => core.call("task.identity.v1", { title }),
        rebuildOptimisticState: () => {
          const projected = storage.projectState({ snapshot: before.meta.snapshot, queues: { ...before.queues,
            commands: state.pending, taskOperations: state.pendingTaskOperations, selectedTaskOperations: state.pendingSelectedTaskOperations },
            nowMs: now, deviceId: state.deviceId, sharedCore: core });
          state.tasks = projected.tasks;
        }
      };
      for (const method of ["persistTaskOperation", "persistSelectedTaskOperation", "persistRetargetOperation"]) {
        ports[method] = async (...args) => {
          const returned = await actions[method](...args); calls.push({ method, returned }); return returned;
        };
      }
      for (const method of ["render", "renderTaskSelector", "renderSyncStatus", "scheduleSync", "showNotice"]) {
        ports[method] = (...args) => effects.push({ method, args });
      }
      const controller = mutations.create({ state, external: { host: globalThis, syncStorage: storage, syncCore }, use: ports });
      return { returned: await controller.addTask(intent.title), calls, effects };
    }
    case "upsertTask": return actions.persistTaskOperation("upsert", known, "user-1");
    case "deleteTask": return actions.persistTaskOperation("delete", known, "user-1");
    case "setAutoStart": return actions.persistAutoStartOperation(intent.enabled, "user-1");
    case "setDuration": return actions.persistDurationOperation(intent.phase, intent.minutes * 60000);
    case "selectTask": {
      const selection = await actions.persistSelectedTaskOperation(intent.taskId, "user-1");
      const commands = state.timer?.phase === "focus" && ["running", "paused"].includes(state.timer.status)
        ? [await actions.persistRetargetOperation(state.timer.id, intent.taskId)] : [];
      return { selection, commands };
    }
    case "cancelAndClear": return storage.cancelAndClearTimer(database, { expectedUserId: "user-1",
      deviceId: "device-local", timerId: state.timer.id, phase: state.timer.phase, nowMs: now,
      observedElapsedMs: use.elapsedFor(state.timer), withUuidV7: true, sharedCore: core });
    case "finish": return storage.finishTimer(database, { expectedUserId: "user-1", deviceId: "device-local",
      timerId: state.timer.id, phase: state.timer.phase, requestedTimer: state.timer, nowMs: now,
      observedElapsedMs: use.elapsedFor(state.timer), manual: true, withUuidV7: true, sharedCore: core });
    default: return actions.persistCommand(intent.kind);
  }
}

async function seed(database, intent, emptyTasks = false) {
  const base = clone(fixture.request.workspace.base);
  base.tasks = emptyTasks ? [] : [known];
  if (!["start", "setDuration"].includes(intent.kind)) base.canonicalTimer = clone(fixture.timer);
  if (intent.kind === "resume") base.canonicalTimer.status = "paused";
  if (intent.kind === "clear") Object.assign(base.canonicalTimer, { status: "completed", elapsedAtAnchorMs: 60000 });
  const transaction = database.transaction(["meta"], "readwrite");
  const values = { snapshot: { ...base, user: { id: "user-1" }, revision: 0 }, deviceId: "device-local",
    deviceSequence: 7, hlc: fixture.request.allocation.hlc, canonicalHead: fixture.request.workspace.canonicalHead,
    settings: { selectedPhase: "focus" }, deliveryProof: storage.emptyDeliveryProof() };
  for (const [key, value] of Object.entries(values)) transaction.objectStore("meta").put({ key, value });
  await storage.transactionDone(transaction);
}

function request(rows, intent) {
  const input = clone(fixture.request);
  const meta = rows.meta;
  input.compatibility = "pwaStorage";
  input.intent = intent;
  input.workspace = { base: Object.fromEntries(Object.keys(input.workspace.base).map(key => [key, meta.snapshot[key]])),
    local: rows.queues, canonicalHead: meta.canonicalHead ?? null, neverSent: meta.deliveryProof,
    timerDependencies: rows.queues.commands.filter(op => op.dependsOnCommandId).map(op => ({
      operationId: op.id, dependsOnOperationId: op.dependsOnCommandId })) };
  for (const domain of Object.keys(domains)) {
    const claimed = new Set((meta.outgoingSync?.sent?.[domain] || []).map(op => op.id));
    assert.ok(input.workspace.neverSent[domain].every(id => !claimed.has(id)));
  }
  input.allocation = { deviceId: meta.deviceId, deviceSequence: meta.deviceSequence, hlc: meta.hlc, lastUuid: meta.uuidV7 ?? null };
  const first = meta.uuidV7 ? storage.uuid7Parts(meta.uuidV7).randomValue + 1n : 1n;
  input.identities.commandUuids = [0, 1, 2].map(index => storage.uuid7FromParts(Math.max(now, meta.hlc.wallMs), first + BigInt(index)));
  input.observation = { canonicalAnchorAt: null, commandTimes: {} };
  if (["upsertTask", "addAndSelectTask", "deleteTask", "selectTask", "setDuration", "setAutoStart"].includes(intent.kind)) {
    input.ownership = { ownerId: meta.snapshot.user.id, expectedOwnerId: "user-1" };
    input.durability = { localTabId: "tab-a", outgoingDurationOperationIds: (meta.outgoingSync?.sent?.durationOperations || []).map(op => op.id) };
  } else input.identities.commandUuids = input.identities.commandUuids.slice(0, 2);
  return input;
}

function compare(returned, planned, intent, before, after) {
  const names = { upsertTask: "taskOperations", deleteTask: "taskOperations", setAutoStart: "autoStartOperations" };
  if (intent.kind === "addAndSelectTask") {
    assert.deepEqual(returned, { returned: true, calls: [
      { method: "persistTaskOperation", returned: planned.operations.taskOperations[0] },
      { method: "persistSelectedTaskOperation", returned: planned.operations.selectedTaskOperations[0] },
      { method: "persistRetargetOperation", returned: planned.commands[0] }], effects: [
      { method: "render", args: [] }, { method: "scheduleSync", args: [0] },
      { method: "renderTaskSelector", args: [] }, { method: "renderSyncStatus", args: [] }, { method: "scheduleSync", args: [0] }] });
  } else if (names[intent.kind]) assert.deepEqual(returned, planned.operations[names[intent.kind]][0]);
  else if (intent.kind === "selectTask") assert.deepEqual(returned, {
    selection: planned.operations.selectedTaskOperations[0], commands: planned.commands });
  else if (intent.kind === "setDuration") assert.deepEqual(returned, {
    operation: planned.operations.durationOperations[0], pendingDurationOperations: planned.workspace.local.durationOperations });
  else if (intent.kind === "finish") assert.deepEqual(returned, { transitioned: true, reason: "", commands: planned.commands,
    selectedPhase: planned.selection.phase, selectedPhaseDurationMs: planned.projection.durationsMs[planned.selection.phase] });
  else if (intent.kind === "cancelAndClear") assert.deepEqual(returned, { transitioned: true, reason: "", commands: planned.commands });
  else assert.deepEqual(returned, planned.commands[0]);
  assert.deepEqual(after.queues, planned.workspace.local);
  assert.deepEqual(after.meta.outgoingSync, before.meta.outgoingSync);
  assert.deepEqual(after.meta.deliveryProof, Object.fromEntries(Object.keys(domains).map(name => [name, planned.workspace.neverSent[name] || []])));
  assert.deepEqual({ deviceId: after.meta.deviceId, deviceSequence: after.meta.deviceSequence, hlc: after.meta.hlc,
    lastUuid: after.meta.uuidV7 }, planned.allocation);
  assert.deepEqual(after.meta.canonicalHead, before.meta.canonicalHead);
}

async function runCase(intent, nullHead, core, pendingSource = null) {
  const name = intent.kind + (nullHead ? "NullHead" : "Claimed") + (pendingSource ? `Pending${pendingSource}` : "");
  let database = await open(name);
  await seed(database, pendingSource ? { kind: "start" } : intent, pendingSource === "task");
  const initial = pendingSource === "start" ? { kind: "start" } : pendingSource === "task" ? { kind: "upsertTask", title: known.title }
    : intent.kind === "setDuration" ? { ...intent, minutes: 3 }
    : intent.kind === "setAutoStart" ? { kind: "setAutoStart", enabled: false }
    : intent.kind === "addAndSelectTask" ? { kind: "selectTask", taskId: null }
    : ["upsertTask", "deleteTask", "selectTask"].includes(intent.kind) ? { kind: "upsertTask", title: known.title }
    : ["clear", "start"].includes(intent.kind) ? { kind: "setAutoStart", enabled: true }
    : { kind: "selectTask", taskId: null };
  await invoke(database, initial, core);
  const sent = (await raw(database)).queues;
  const claimReturn = await storage.retireProofAndPersistOutgoing(database, sent, { ownerId: "user-1" });
  if (nullHead) {
    const transaction = database.transaction("meta", "readwrite");
    transaction.objectStore("meta").put({ key: "canonicalHead", value: null });
    await storage.transactionDone(transaction);
  }
  const closed = await raw(database);
  database.close();
  database = await open(name);
  try {
    const before = await raw(database);
    assert.deepEqual(before, closed);
    const input = request(before, intent);
    let operation = "workspace.intent.v1";
    const current = repository(database, before, core).state.timer;
    if (intent.kind === "finish") {
      operation = "workspace.completionMutation.v1";
      delete input.intent;
      Object.assign(input, { stage: "finishCommit", requestedTimer: current, ownership: null });
      input.identities.commandUuids = input.identities.commandUuids.slice(0, 1);
    } else if (intent.kind === "cancelAndClear") input.requestedTimer = current;
    const returned = await invoke(database, intent, core);
    const after = await raw(database);
    const planned = native(operation, input);
    compare(returned, planned, intent, before, after);
    assert.deepEqual(planned, native(operation, JSON.parse(JSON.stringify(input))), "complete restart result");
    return { name, claimReturn, rawBefore: before, rawAfter: after, input, productionReturn: returned, core: planned };
  } finally { database.close(); }
}

async function liveMonotonicCase(core) {
  const source = fs.readFileSync(path.join(web, "app-state.js"), "utf8");
  const helpers = source.slice(source.indexOf("  function positiveNumber("), source.indexOf("  function tabID("));
  const clockClass = source.slice(source.indexOf("  class TrustedClock {"), source.indexOf("  class SharedTaskCore {"));
  const Clock = Function(`${helpers}\n${clockClass}\nreturn TrustedClock;`)();
  let monotonic = 1000;
  const clock = new Clock({ clockOffset: null, hlcWallMs: now }, { performance: { now: () => monotonic } }, syncCore);
  let database = await open("live-monotonic-pending-start");
  await seed(database, { kind: "start" });
  await invoke(database, { kind: "start" }, core);
  const original = await raw(database);
  const timer = repository(database, original, core).state.timer;
  clock.trustedNow(now, monotonic);
  assert.equal(clock.elapsedFor(timer, now, monotonic), 0);
  const nativeAnchor = clone(clock.elapsedMonotonicAnchor), nativeRuntime = clone(clock.runtime);
  await storage.retireProofAndPersistOutgoing(database, original.queues, { ownerId: "user-1" });
  const transaction = database.transaction("meta", "readwrite");
  transaction.objectStore("meta").put({ key: "canonicalHead", value: null });
  await storage.transactionDone(transaction);
  database.close();
  database = await open("live-monotonic-pending-start");
  try {
    const before = await raw(database), intent = { kind: "pause" };
    const input = request(before, intent);
    monotonic = 2000;
    const physical = now + 1790000;
    Date.now = () => physical;
    const trusted = clock.trustedNow(physical, monotonic);
    assert.equal(trusted, now + 1000);
    const current = repository(database, before, core);
    const actions = app.create({ state: current.state, external: { host: globalThis, syncStorage: storage, syncCore },
      use: { ...current.use, trustedNow: clock.trustedNow.bind(clock), elapsedFor: clock.elapsedFor.bind(clock) } });
    actions.setDatabaseForTest(database);
    const returned = await actions.persistCommand("pause");
    input.clock = { occurredAt: new Date(trusted).toISOString(), physicalNow: new Date(physical).toISOString(),
      observedAt: new Date(physical).toISOString(), monotonicNowMs: monotonic, continuityId: "live-page" };
    input.identities.commandUuids = [storage.uuid7FromParts(trusted, 1n)];
    const [id, anchorAt, elapsed] = nativeAnchor.key.split("\u0000");
    input.observation.monotonicAnchor = { timerId: id, anchorAt, elapsedAtAnchorMs: Number(elapsed),
      sampledTrustedNowMs: nativeRuntime.wallMs, sampledMonotonicMs: nativeAnchor.monotonicMs, continuityId: "live-page" };
    assert.equal(nativeAnchor.elapsedMs, Number(elapsed));
    const planned = native("workspace.intent.v1", input), after = await raw(database);
    compare(returned, planned, intent, before, after);
    assert.equal(returned.observedElapsedMs, 1000);
    assert.equal(planned.commandOutcomes[0].outcome, "queued");
    assert.equal(planned.projection.canonicalTimer, null);
    return { name: "liveMonotonicPendingStart", input, nativeAnchor, nativeRuntime, rawBefore: before, rawAfter: after,
      productionReturn: returned, core: planned, continuity: "same live clock across database reopen" };
  } finally { Date.now = () => now; database.close(); }
}

async function main() {
  const core = await SharedCore.fromBytes(fs.readFileSync(path.join(web, "pomodorough_core.wasm")));
  storage.setSharedCore(core);
  const intents = [{ kind: "upsertTask", title: known.title }, { kind: "deleteTask", taskId: known.id },
    { kind: "addAndSelectTask", title: "Durable task" },
    { kind: "selectTask", taskId: known.id }, { kind: "setDuration", phase: "short_break", minutes: 4 },
    { kind: "setAutoStart", enabled: true }, ...["start", "pause", "resume", "cancel", "clear", "cancelAndClear", "finish"].map(kind => ({ kind }))];
  const results = [];
  for (const nullHead of [false, true]) for (const intent of intents) {
    if (!process.env.PROBE_CASE || process.env.PROBE_CASE === intent.kind) results.push(await runCase(intent, nullHead, core));
  }
  if (!process.env.PROBE_CASE || process.env.PROBE_CASE === "pendingStart") results.push(await runCase({ kind: "pause" }, true, core, "start"));
  if (!process.env.PROBE_CASE || process.env.PROBE_CASE === "pendingTask") results.push(await runCase({ kind: "selectTask", taskId: known.id }, false, core, "task"));
  if (!process.env.PROBE_CASE || process.env.PROBE_CASE === "liveMonotonic") results.push(await liveMonotonicCase(core));
  if (process.env.PROBE_EVIDENCE) fs.writeFileSync(process.env.PROBE_EVIDENCE, JSON.stringify(results, null, 2) + "\n");
  console.log(`${results.length} real IndexedDB claim/lost-response/reopen complete-return cases passed`);
}

main().catch(error => { console.error(error); process.exitCode = 1; });
