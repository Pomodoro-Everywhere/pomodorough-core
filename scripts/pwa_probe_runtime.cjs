"use strict";

// Compose the unchanged PWA browser modules (state, storage, actions, view)
// through the production module runtime. IndexedDB is fake-indexeddb and every
// shared-core call goes to the native Core bridge named by POMODOROUGH_CORE_BRIDGE,
// so a probe compares the PWA adapter with the Core build it checks.
const fs = require("node:fs");
const path = require("node:path");
const { execFileSync } = require("node:child_process");

const web = path.resolve(__dirname, "../../server/web");
const { IDBFactory } = require(path.resolve(web, "../node_modules/fake-indexeddb"));
const runtimeModule = require(path.join(web, "app-runtime.js"));
const syncCore = require(path.join(web, "sync-core.js"));
const syncStorage = require(path.join(web, "sync-storage.js"));
const sharedCoreHost = require(path.join(web, "shared-core.js"));
const MODULES = ["app-state.js", "app-storage.js", "app-actions.js", "app-view.js"]
  .map((name) => require(path.join(web, name)));
const QUEUES = Object.freeze({ commands: "pending", taskOperations: "pendingTasks",
  durationOperations: "pendingDurations", autoStartOperations: "pendingAutoStarts",
  selectedTaskOperations: "pendingSelectedTasks" });
const OWNER = Object.freeze({ id: "user-1" });

function readInput() {
  return JSON.parse(fs.readFileSync(0, "utf8"));
}

function nativeCore(log) {
  const bridge = process.env.POMODOROUGH_CORE_BRIDGE;
  if (!bridge) throw new Error("POMODOROUGH_CORE_BRIDGE must name the native Core bridge");
  const call = (operation, input) => {
    const output = execFileSync(bridge, { input: `${JSON.stringify({ operation, input })}\n`, encoding: "utf8" });
    const value = JSON.parse(output);
    log.push({ operation, input });
    if (value && Object.keys(value).length === 1 && typeof value.error === "string") throw new Error(value.error);
    return value;
  };
  // The PWA reaches Core only through call(); setSharedCore also demands the
  // typed wrappers, which no probed path may use.
  const absent = (name) => () => { throw new Error(`probe Core wrapper used: ${name}`); };
  return { call, projectSynchronizedState: absent("projectSynchronizedState"), planBootstrap: absent("planBootstrap"),
    reconcileSynchronizedState: absent("reconcileSynchronizedState"), planTimerCompletion: absent("planTimerCompletion"),
    tickHlc: absent("tickHlc") };
}

function probeClock() {
  return { wallMs: 0, monotonicMs: null, continuityId: "browser-session-1", timerUuid: null, issued: 0 };
}

function probeHost(clock) {
  const noop = () => {};
  return {
    indexedDB: new IDBFactory(), console: { warn: noop, error: noop, info: noop, log: noop },
    // The first UUID a session draws is its clock continuity; later draws name new timers.
    crypto: { randomUUID: () => (clock.issued++ === 0 ? clock.continuityId : clock.timerUuid) },
    performance: { now: () => clock.monotonicMs ?? undefined },
    sessionStorage: { getItem: () => "tab-local", setItem: noop },
    localStorage: { getItem: () => null, setItem: noop, removeItem: noop },
    setTimeout: () => 0, clearTimeout: noop, setInterval: () => 0, clearInterval: noop,
    addEventListener: noop, navigator: {}, location: {}, document: undefined
  };
}

function probeElements() {
  const made = new Map();
  const element = () => ({ hidden: false, disabled: false, textContent: "", classList: { toggle() {}, add() {}, remove() {} },
    setAttribute() {}, removeAttribute() {}, addEventListener() {} });
  return new Proxy({}, { get: (_target, name) => {
    if (!made.has(name)) made.set(name, element());
    return made.get(name);
  } });
}

function overridden(browserModule, overrides) {
  const names = Object.keys(overrides).filter((name) => browserModule.manifest.provides.includes(name));
  if (!names.length) return browserModule;
  return { manifest: browserModule.manifest, create: (context) => ({ ...browserModule.create(context),
    ...Object.fromEntries(names.map((name) => [name, overrides[name]])) }) };
}

// Collaborators from modules the probe does not install (sync, bootstrap,
// session) fail loudly unless a probe names a stand-in for them.
function standIns(overrides) {
  const provided = new Set(MODULES.flatMap((item) => item.manifest.provides));
  const missing = [...new Set(MODULES.flatMap((item) => item.manifest.requires))].filter((name) => !provided.has(name));
  return { manifest: { name: "probe", provides: missing }, create: () => Object.fromEntries(missing.map((name) =>
    [name, overrides[name] || (() => { throw new Error(`probe stand-in used: ${name}`); })])) };
}

function createPwa(overrides = {}) {
  const clock = probeClock();
  // UUIDv7 reservation reads globalThis.crypto; fixed entropy keeps identities replayable.
  Object.defineProperty(globalThis, "crypto", { value: { getRandomValues: fixedEntropy }, configurable: true });
  const log = [];
  syncStorage.setSharedCore(nativeCore(log));
  const host = probeHost(clock);
  const state = MODULES[0].createState(host);
  const elements = probeElements();
  const externals = { host, elements, syncCore, syncStorage, sharedCoreHost };
  const builder = runtimeModule.createRuntime({ state, externals });
  for (const browserModule of MODULES) builder.install(overridden(browserModule, overrides));
  builder.install(standIns(overrides));
  const runtime = builder.finalize();
  return { runtime, state, host, elements, clock, log, call: (name, ...args) => runtime.call(name, ...args) };
}

// Point the browser clocks at a Core request clock. The first setClock also
// pins the tab's clock continuity, so later UUID draws name new timers only.
function setClock(pwa, clock) {
  pwa.clock.wallMs = Date.parse(clock.physicalNow ?? clock.observedAt);
  pwa.clock.monotonicMs = clock.monotonicNowMs ?? null;
  Date.now = () => pwa.clock.wallMs;
  if (pwa.clock.issued > 0) return;
  if (clock.continuityId) pwa.clock.continuityId = clock.continuityId;
  if (pwa.call("clockContinuityId") !== pwa.clock.continuityId) throw new Error("probe continuity was not pinned");
}

function selectedPhase(request) {
  return request.selectedPhase ?? request.selection?.phase ?? "focus";
}

function putRecords(meta, request) {
  const workspace = request.workspace;
  const records = { snapshot: { ...workspace.base, user: OWNER, revision: 0 },
    deviceId: request.allocation.deviceId, deviceSequence: request.allocation.deviceSequence,
    hlc: request.allocation.hlc, uuidV7: request.allocation.lastUuid, canonicalHead: workspace.canonicalHead,
    timerDependencies: workspace.timerDependencies, workspaceObservation: request.observation,
    completionState: request.selection ? { selection: request.selection,
      ...(request.lifecycle ? { lifecycle: request.lifecycle } : {}) } : null,
    settings: { selectedPhase: selectedPhase(request) }, timerOwner: request.ownership,
    deliveryProof: Object.keys(workspace.neverSent || {}).length ? workspace.neverSent : null };
  for (const [key, value] of Object.entries(records)) if (value != null) meta.put({ key, value });
}

// Persist a Core request's workspace as the PWA's IndexedDB records, then load
// them through the production reload path a signed-in tab uses.
async function seedWorkspace(pwa, request) {
  const database = await pwa.call("openDatabase");
  const stores = ["meta", ...Object.values(QUEUES)];
  const transaction = database.transaction(stores, "readwrite");
  putRecords(transaction.objectStore("meta"), request);
  for (const [domain, store] of Object.entries(QUEUES)) {
    for (const item of request.workspace.local[domain] || []) transaction.objectStore(store).put(item);
  }
  await pwa.call("transactionDone", transaction);
  Object.assign(pwa.state, { user: OWNER, localOwnerId: OWNER.id, deviceId: request.allocation.deviceId,
    ready: true, authenticated: true, sessionIdentityValidated: true, selectedPhase: selectedPhase(request),
    // A resolved bootstrap clears the fail-closed gate (app-bootstrap.js).
    bootstrapBlocked: false });
  pwa.call("setDatabaseForTest", database);
  await pwa.call("reloadPersistedState", null, pwa.call("captureAccountContext"));
  return database;
}

async function readMeta(database, keys) {
  const transaction = database.transaction(["meta", "pending"], "readonly");
  const meta = transaction.objectStore("meta");
  const values = await Promise.all(keys.map((key) => syncStorage.requestResult(meta.get(key))));
  const commands = await syncStorage.requestResult(transaction.objectStore("pending").getAll());
  return { ...Object.fromEntries(keys.map((key, index) => [key, values[index]?.value ?? null])), commands };
}

// Fixed entropy makes reserved UUIDv7 suffixes match the fixtures' identities.
function fixedEntropy(bytes) {
  bytes.fill(0);
  bytes[bytes.length - 1] = 1;
  return bytes;
}

async function runCases(handlers) {
  const { leg, cases } = readInput();
  const handler = handlers[leg];
  if (!handler) throw new Error(`unknown PWA probe leg: ${leg}`);
  const results = [];
  for (const item of cases) results.push(await handler(item));
  process.stdout.write(JSON.stringify(results));
}

function main(handlers) {
  runCases(handlers).catch((error) => { console.error(error); process.exitCode = 1; });
}

module.exports = { createPwa, setClock, seedWorkspace, readMeta, fixedEntropy, main, OWNER };
