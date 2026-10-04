"use strict";

const assert = require("node:assert/strict"), fs = require("node:fs"), path = require("node:path");
const crypto = require("node:crypto"), Module = require("node:module"), { spawnSync } = require("node:child_process");
const a = require("./pwa_ownership_source_adapter.cjs");
const filename = path.join(a.server, "web/sync-storage.js");
const currentSource = fs.readFileSync(filename, "utf8");
const frozenPath = process.env.CORE_PAGEHIDE_FROZEN_STORAGE || "/var/folders/r_/_mr22dqn24d31b7460cz8z5m0000gn/T/opencode/pwa-core-045/baseline/web/sync-storage.js";
const frozenSource = fs.readFileSync(frozenPath, "utf8");
const original = spawnSync("git", ["show", "50c86a2:web/sync-storage.js"], { cwd: a.server, encoding: "utf8", maxBuffer: 1024 * 1024 });
assert.equal(original.status, 0, original.stderr);
const calls = [];

function method(source) {
  const start = source.indexOf("  function releaseTimerOwnership(");
  const end = source.indexOf("\n  function ", start + 1);
  assert.ok(start >= 0 && end > start, "production release boundary changed");
  return source.slice(start, end);
}

const originalMethod = method(original.stdout), currentMethod = method(currentSource);
assert.equal(currentMethod, originalMethod, "release policy changed since the production oracle");
assert.equal(method(frozenSource), originalMethod, "accepted frozen production release policy changed");

function dispatch(input, results, hostInput) {
  const inputRaw = JSON.stringify(input);
  assert.deepEqual(JSON.parse(inputRaw), input);
  const observed = structuredClone({ results, deviceId: hostInput.deviceId, tabId: hostInput.tabId, nowMs: hostInput.nowMs });
  try {
    const completeReturn = a.native("workspace.ownershipPlan.v1", input);
    calls.push({ observed, inputRaw, decoded: input, completeReturn });
    return completeReturn;
  } catch (error) {
    calls.push({ observed, inputRaw, decoded: input, rejectedEnvelopeRaw: error.coreEnvelopeRaw });
    throw error;
  }
}

function compile(source, core) {
  const loaded = new Module(filename, module);
  loaded.filename = filename; loaded.paths = Module._nodeModulePaths(path.dirname(filename));
  loaded.releaseProbe = { dispatch };
  loaded._compile("const releaseProbe = module.releaseProbe;\n" + source, filename);
  loaded.exports.setSharedCore(core);
  return loaded.exports;
}

function adapters(core) {
  const replacement = `  function releaseTimerOwnership(database, input) {
    return guardedMutation(database, [], (transaction, _outcome, _abort, results) => {
      input.assertCurrent?.();
      const plan = releaseProbe.dispatch({ profile: "pwaStorage", action: { kind: "release" },
        workspace: workspaceRecords({ ...transactionWorkspace(results), deviceId: input.deviceId }),
        ownership: results.timerOwner?.value ?? null, localDeviceId: input.deviceId, localTabId: input.tabId,
        clock: { nowMs: input.nowMs } }, results, input);
      input.assertCurrent?.();
      workspaceTransaction.writeOwnership(transaction.objectStore(META_STORE), plan.ownershipWrites);
    }, { ...input, allowBootstrap: true });
  }
`;
  return { original: compile(original.stdout, core), frozen: compile(frozenSource, core), current: compile(currentSource, core),
    migrated: compile(currentSource.replace(currentMethod, replacement), core) };
}

function ownerWrites(database) {
  const transaction = database.transaction, writes = [];
  database.transaction = function (...args) {
    const tx = transaction.apply(this, args);
    if (args[1] !== "readwrite") return tx;
    const store = tx.objectStore("meta"), put = store.put.bind(store), remove = store.delete.bind(store);
    store.put = (row) => { if (row.key === "timerOwner") writes.push({ kind: "recordTimerOwner", ...structuredClone(row.value) }); return put(row); };
    store.delete = (key) => { if (key === "timerOwner") writes.push({ kind: "removeTimerOwner" }); return remove(key); };
    return tx;
  };
  return { writes, restore() { database.transaction = transaction; } };
}

const hash = (source) => crypto.createHash("sha256").update(source).digest("hex");
module.exports = { ...a, calls, adapters, ownerWrites, sources: { originalCommit: "50c86a2",
  originalSha256: hash(original.stdout), frozenSha256: hash(frozenSource), currentSha256: hash(currentSource), releaseSha256: hash(currentMethod) } };
