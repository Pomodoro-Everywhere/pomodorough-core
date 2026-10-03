"use strict";

const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");
const Module = require("node:module");
const { spawnSync } = require("node:child_process");
const server = path.resolve(__dirname, "../../server");
const workspaceCore = require(server + "/web/workspace-core.js");
const stores = require(server + "/web/workspace-transaction.js").QUEUE_STORES;
const calls = [];
const oracle = process.env.CORE_PWA09_ORACLE || path.resolve(__dirname, "../target/debug/examples/artifact_parity_oracle");

function native(operation, input) {
  const raw = JSON.stringify(input);
  const returned = spawnSync(oracle, [], { input: JSON.stringify({ operation, input: raw }) + "\n",
    encoding: "utf8", maxBuffer: 16 * 1024 * 1024 });
  assert.equal(returned.status, 0, returned.stderr);
  const envelope = JSON.parse(returned.stdout);
  if (!envelope.ok) {
    const error = new Error(envelope.error);
    error.coreEnvelopeRaw = returned.stdout.trimEnd();
    throw error;
  }
  return envelope.value;
}

function request(results, input, action, installed = null) {
  const snapshot = installed?.snapshot ?? results.snapshot?.value;
  const local = installed?.queues ?? Object.fromEntries(workspaceCore.DOMAINS.map((name) => [name, results[name] || []]));
  const workspace = { base: workspaceCore.base(snapshot), local,
    neverSent: installed?.proof ?? results.deliveryProof?.value ?? {},
    canonicalHead: installed?.head ?? results.canonicalHead?.value ?? null,
    timerDependencies: installed?.dependencies ?? results.timerDependencies?.value ?? [],
    displayContext: { profile: "pwaStorage", projectionPending: installed
      ? installed.display ?? null : results.projectionPending?.value ?? null } };
  const raw = { profile: "pwaStorage", action, workspace, ownership: results.timerOwner?.value ?? null,
    localDeviceId: input.deviceId, localTabId: input.tabId,
    clock: { nowMs: input.nowMs, leaseDurationMs: input.leaseMs } };
  // Keep actual transaction observations beside the decode, not an inverse fixture.
  const observed = structuredClone({ results, input: { deviceId: input.deviceId, tabId: input.tabId,
    timerId: input.timerId, nowMs: input.nowMs, leaseMs: input.leaseMs }, installed });
  const inputRaw = JSON.stringify(raw);
  assert.deepEqual(JSON.parse(inputRaw), raw);
  let returned;
  try { returned = native("workspace.ownershipPlan.v1", raw); }
  catch (error) {
    calls.push({ observed, inputRaw, decoded: raw, rejectedEnvelopeRaw: error.coreEnvelopeRaw });
    throw error;
  }
  assert.deepEqual(returned.workspace, workspace);
  assert.deepEqual(returned.effectsAfterCommit, []);
  calls.push({ observed, inputRaw, decoded: raw, completeReturn: returned });
  return returned;
}

function persist(store, plan) {
  for (const write of plan.ownershipWrites) {
    switch (write.kind) {
      case "removeTimerOwner": store.delete("timerOwner"); break;
      case "recordTimerOwner": {
        const { kind, ...value } = write;
        store.put({ key: "timerOwner", value });
        break;
      }
      default: assert.fail("unknown Core owner write");
    }
  }
}

function replaceFunction(source, name, body) {
  const start = source.indexOf(`  function ${name}(`);
  assert.ok(start >= 0, `missing production function ${name}`);
  const end = source.indexOf("\n  function ", start + 1);
  assert.ok(end > start, `missing function boundary ${name}`);
  return source.slice(0, start) + body + "\n" + source.slice(end);
}

function replaceSnippet(source, before, after) {
  assert.equal(source.split(before).length, 2, "production ownership call changed");
  return source.replace(before, after);
}

function staleInstallSource(source) {
  source = replaceSnippet(source, `      const ownerClaim = plannedMissingTimerOwner(
        owner, storedSnapshot, results.commands || [], input.timerOwnerClaim
      );`, `      const ownerPlan = ownershipProbe.request(results, input.timerOwnerClaim, { kind: "install" });
      const ownerClaim = ownershipProbe.ownerPlan(ownerPlan).value;`);
  source = replaceSnippet(source, "kind: \"stale\", clockOffset, ownerClaim,", "kind: \"stale\", clockOffset, ownerClaim, ownerPlan,");
  source = replaceSnippet(source, `      if (plan.ownerClaim) metaStore.put({ key: TIMER_OWNER_KEY, value: plan.ownerClaim });`,
    "      ownershipProbe.persist(metaStore, plan.ownerPlan);");
  return replaceSnippet(source, `    claimMissingTimerOwner(
      metaStore,
      results.timerOwner?.value || null,
      storedSnapshot,
      results.commands || [],
      canonical.timerOwnerClaim
    );`, `    ownershipProbe.persist(metaStore,
      ownershipProbe.request(results, canonical.timerOwnerClaim, { kind: "install" }));`);
}

function compileStorage() {
  const filename = server + "/web/sync-storage.js";
  const original = fs.readFileSync(filename, "utf8");
  let source = replaceFunction(original, "renewTimerOwnership", `  function renewTimerOwnership(database, input) {
    return guardedMutation(database, [PENDING_STORE], (transaction, outcome, abort) => {
      const results = {};
      collectTransactionRequests(mutationContextRequests(transaction), results, () => {
        input.assertCurrent?.();
        const plan = ownershipProbe.request(results, input, { kind: "renew", timerId: input.timerId });
        ownershipProbe.persist(transaction.objectStore(META_STORE), plan);
        outcome.value = plan.renewed;
      }, abort);
    }, { ...input, allowBootstrap: true });
  }`);
  source = replaceFunction(source, "plannedTimerOwner", `  function plannedTimerOwner(input, results, rebased) {
    const commands = retainedCommands(results.commands,
      [...(input.queueIds.commands || []), ...rebased.droppedCommandIds],
      rebased.queues?.commands || input.promoteCommands);
    const queues = rebased.queues || Object.fromEntries(workspaceCore.DOMAINS.map((name) => [name,
      name === "commands" ? commands : (results[name] || []).filter((row) => !(input.queueIds[name] || []).includes(row.id))]));
    return ownershipProbe.ownerPlan(ownershipProbe.request(results, input.timerOwnerClaim, { kind: "install" }, {
      snapshot: input.snapshot, queues, display: rebased.projectionPending,
      head: input.serverHlc || results.canonicalHead?.value || null,
      proof: neverSentFromProof(results.deliveryProof?.value, queues, input.reconciliation?.sent),
      dependencies: rebased.timerDependencies || results.timerDependencies?.value || [] }));
  }`);
  source = replaceFunction(source, "applyResolutionTimerOwner", `  function applyResolutionTimerOwner(metaStore, results, canonical, queueIds, rebased) {
    const commands = canonical.reconciliation ? rebased.queues.commands : retainedCommands(results.commands,
      [...(queueIds.commands || []), ...rebased.droppedCommandIds], canonical.promoteCommands);
    const queues = rebased.queues || Object.fromEntries(workspaceCore.DOMAINS.map((name) => [name,
      name === "commands" ? commands : (results[name] || []).filter((row) => !(queueIds[name] || []).includes(row.id))]));
    const plan = ownershipProbe.request(results, canonical.timerOwnerClaim, { kind: "install" }, {
      snapshot: canonical.snapshot, queues, display: rebased.projectionPending,
      head: canonical.serverHlc || results.canonicalHead?.value || null,
      proof: neverSentFromProof(results.deliveryProof?.value, queues),
      dependencies: rebased.timerDependencies || [] });
    ownershipProbe.persist(metaStore, plan);
  }`);
  source = staleInstallSource(source);
  const loaded = new Module(filename, module);
  loaded.filename = filename;
  loaded.paths = Module._nodeModulePaths(path.dirname(filename));
  loaded.ownershipProbe = { request, persist, ownerPlan: (plan) => ({
    remove: plan.ownershipWrites.some((write) => write.kind === "removeTimerOwner"),
    value: plan.ownershipWrites.find((write) => write.kind === "recordTimerOwner") ? plan.ownership : null }) };
  loaded._compile("const ownershipProbe = module.ownershipProbe;\n" + source, filename);
  return { storage: loaded.exports, sourceSha256: crypto.createHash("sha256").update(original).digest("hex") };
}

module.exports = { native, request, persist, compileStorage, calls, stores, server };
