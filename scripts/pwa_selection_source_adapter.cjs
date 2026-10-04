"use strict";
const assert = require("node:assert/strict"), fs = require("node:fs"), path = require("node:path");
const Module = require("node:module"), crypto = require("node:crypto"), { spawnSync } = require("node:child_process");
const root = process.env.POMODOROUGH_ROOT || path.resolve(__dirname, "../..");
const directory = process.env.PWA_SELECTION_EVIDENCE_DIR || "/var/folders/r_/_mr22dqn24d31b7460cz8z5m0000gn/T/opencode";
const native = path.join(root, "pomodorough-core/target/debug/examples/artifact_parity_oracle");
const baseline = path.join(directory, "core-pwa-selection-baseline/debug/examples/artifact_parity_oracle");
const filename = path.join(root, "server/web/sync-storage.js");
const source = fs.readFileSync(filename, "utf8");
const frozen = fs.readFileSync(path.join(directory, "pwa-core-045/baseline/web/sync-storage.js"), "utf8");
const txFilename = path.join(root, "server/web/workspace-transaction.js");
const txSource = fs.readFileSync(txFilename, "utf8");
const receipts = [];

function dispatch(operation, input, executable = native) {
  const inputRaw = typeof input === "string" ? input : JSON.stringify(input);
  const result = spawnSync(executable, [], { input: JSON.stringify({ operation, input: inputRaw }) + "\n",
    encoding: "utf8", timeout: 120000, maxBuffer: 16 * 1024 * 1024 });
  assert.equal(result.status, 0, result.stderr);
  const envelope = JSON.parse(result.stdout);
  assert.deepEqual(Object.keys(envelope).sort(), envelope.ok ? ["ok", "value"] : ["error", "ok"]);
  if (!envelope.ok) { const error = new Error(envelope.error); error.coreEnvelopeRaw = result.stdout.trimEnd(); throw error; }
  return { inputRaw, input: JSON.parse(inputRaw), envelopeRaw: result.stdout.trimEnd(), completeReturn: envelope.value };
}

function compile(filename, source, dependencies = {}) {
  const loaded = new Module(filename, module); loaded.filename = filename;
  loaded.paths = Module._nodeModulePaths(path.dirname(filename));
  loaded.require = (name) => dependencies[name] || Module.prototype.require.call(loaded, name);
  loaded._compile(source, filename); return loaded.exports;
}

function replaceOnce(source, before, after) {
  assert.equal(source.split(before).length, 2, "The production plumbing boundary changed.");
  return source.replace(before, after);
}

function readContext(source) {
  source = replaceOnce(source, "projectionPending, dependencies, observation, settings, deviceSequence] = await Promise.all([",
    "projectionPending, dependencies, observation, settings, deviceSequence, completionState] = await Promise.all([");
  source = replaceOnce(source, 'requestResult(metaStore.get("settings")), requestResult(metaStore.get("deviceSequence"))',
    'requestResult(metaStore.get("settings")), requestResult(metaStore.get("deviceSequence")), requestResult(metaStore.get("completionState"))');
  source = replaceOnce(source, 'settings: settings?.value ?? null, deviceSequence: deviceSequence?.value ?? null',
    'settings: settings?.value ?? null, deviceSequence: deviceSequence?.value ?? null, completionState: completionState?.value ?? null');
  source = replaceOnce(source, '    const request = workspaceCore.readRequest(raw, input.selectedPhase, input.nowMs, input.monotonic ?? null);',
    '    const request = workspaceCore.readRequest(raw, input.selectedPhase, input.nowMs, input.monotonic ?? null);\n    if (input.completionState) Object.assign(request, input.completionState);');
  return replaceOnce(source, 'identities: { commandUuids: [], timerUuid: null }, calendarIntervals: []',
    'identities: { commandUuids: [], timerUuid: null }, calendarIntervals: [], ...(input.completionState || {})');
}

function adapter(core, original = source) {
  // Add raw reads and writes only. Core still decides explicitness, generation, phase, and consumption.
  let transaction = replaceOnce(txSource, '"workspaceObservation", "workspaceGroups"', '"completionState", "workspaceObservation", "workspaceGroups"');
  transaction = replaceOnce(transaction, '    put(meta, "hlc", plan.allocation.hlc);',
    '    if (plan.lifecycle) put(meta, "completionState", { selection: plan.selection, lifecycle: plan.lifecycle });\n    put(meta, "hlc", plan.allocation.hlc);');
  const tx = { ...compile(txFilename, transaction) }, reads = [];
  const run = tx.run;
  tx.run = (database, mode, change) => run(database, mode, (records, transaction) => {
    reads.push(structuredClone(records)); return change(records, transaction);
  });
  const plumbing = replaceOnce(original, "      const request = workspaceMutationRequest(records, input);",
    '      const request = workspaceMutationRequest(records, input);\n      Object.assign(request, records.completionState || { lifecycle: { consumedCompletions: [], pendingBreaks: [] } });');
  const storage = compile(filename, readContext(plumbing), { "./workspace-transaction.js": tx });
  storage.setSharedCore(core); return { storage, reads };
}

function bind(client, storage) {
  const database = client.use.database(); client.external.syncStorage = storage;
  for (const name of ["app-state.js", "app-storage.js", "app-actions.js"]) {
    const filename = path.join(root, "server/web", name);
    let source = fs.readFileSync(filename, "utf8");
    if (name === "app-state.js") source = replaceOnce(source,
      '        projectionPending: local.projectionPending, outgoing: local.outgoingSync',
      '        projectionPending: local.projectionPending, outgoing: local.outgoingSync, completionState: local.completionState');
    if (name === "app-storage.js") source = replaceOnce(source,
      '      this.state.workspaceObservation = syncState.workspaceObservation ?? null;',
      '      this.state.workspaceObservation = syncState.workspaceObservation ?? null;\n      this.state.completionState = syncState.completionState ?? null;');
    Object.assign(client.use, compile(filename, source).create({ state: client.state, external: client.external, use: client.use }));
  }
  client.use.setDatabaseForTest(database);
}

const hash = (text) => crypto.createHash("sha256").update(text).digest("hex");
module.exports = { root, directory, native, baseline, dispatch, adapter, bind, frozen, receipts,
  hashes: { currentStorage: hash(source), frozenStorage: hash(frozen), transaction: hash(txSource) } };
