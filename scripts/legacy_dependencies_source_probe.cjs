"use strict";
const assert = require("node:assert/strict"), fs = require("node:fs"), path = require("node:path"), Module = require("node:module");
const { spawnSync } = require("node:child_process"), test = require("node:test"), crypto = require("node:crypto");
const root = process.env.POMODOROUGH_ROOT || path.resolve(__dirname, "../..");
const temp = process.env.PWA10_EVIDENCE_DIR || "/var/folders/r_/_mr22dqn24d31b7460cz8z5m0000gn/T/opencode";
const f = require(path.join(root, "server/web/test/p222-completion-fixture.js"));
const current = require(path.join(root, "server/web/sync-storage.js"));
const workspace = require(path.join(root, "server/web/workspace-core.js"));
const queueStores = require(path.join(root, "server/web/workspace-transaction.js")).QUEUE_STORES;
const receipts = [], operation = "workspace.legacyDependencyPlan.v1";
const fixtureFile = path.join(__dirname, "../fixtures/legacy-dependencies-v1.json");
const capture = process.argv.includes("--capture");
const expected = capture ? null : JSON.parse(fs.readFileSync(fixtureFile, "utf8"));
const names = ["complete", "all-five-raw", "missing-parent", "sibling-proven", "sibling-frozen", "canonical-source-applied", "canonical-source-ignored", "canonical-source-rejected"];

function frozen(core) {
  const filename = path.join(root, "server/web/sync-storage.js"), module = new Module(filename);
  module.filename = filename; module.paths = Module._nodeModulePaths(path.dirname(filename));
  const source = fs.readFileSync(path.join(temp, "pwa-core-045/baseline/web/sync-storage.js"), "utf8");
  module._compile(source, filename); module.exports.setSharedCore(core);
  return { storage: module.exports, sha256: crypto.createHash("sha256").update(source).digest("hex") };
}

function native(input) {
  const inputRaw = JSON.stringify(input);
  assert.deepEqual(JSON.parse(inputRaw), input);
  const result = spawnSync(path.join(root, "pomodorough-core/target/debug/examples/artifact_parity_oracle"), [], {
    input: JSON.stringify({ operation, input: inputRaw }) + "\n", encoding: "utf8", timeout: 120000, maxBuffer: 16 * 1024 * 1024 });
  assert.equal(result.status, 0, result.stderr);
  const envelope = JSON.parse(result.stdout);
  assert.equal(envelope.ok, true, envelope.error);
  return { inputRaw, completeNativeEnvelope: result.stdout.trimEnd(), completeNativeReturn: envelope.value };
}

function rawInput(before, ownerId, acknowledgements = []) {
  const input = { profile: "pwaStorage", deviceId: f.meta(before, "deviceId"),
    ownership: { ownerId, expectedOwnerId: ownerId, timerOwner: f.meta(before, "timerOwner") ?? null },
    workspace: { base: workspace.base(f.meta(before, "snapshot")),
      local: Object.fromEntries(Object.entries(queueStores).map(([domain, store]) => [domain, before[store]])),
      canonicalHead: f.meta(before, "canonicalHead") ?? null, neverSent: f.meta(before, "deliveryProof") ?? {},
      timerDependencies: f.meta(before, "timerDependencies") ?? null,
      displayContext: { profile: "pwaStorage", projectionPending: f.meta(before, "projectionPending") ?? null },
      now: new Date(f.nowMs).toISOString() }, outgoing: f.meta(before, "outgoingSync") ?? null,
    calendarIntervals: workspace.calendarIntervals([f.nowMs]), sourceAcknowledgements: acknowledgements };
  for (const [domain, store] of Object.entries(queueStores)) assert.deepEqual(input.workspace.local[domain], before[store]);
  for (const [field, key] of [["canonicalHead", "canonicalHead"], ["neverSent", "deliveryProof"], ["timerDependencies", "timerDependencies"]]) {
    assert.deepEqual(input.workspace[field], f.meta(before, key) ?? (field === "neverSent" ? {} : null));
  }
  assert.deepEqual(input.outgoing, f.meta(before, "outgoingSync") ?? null);
  return input;
}

async function load(database, records) {
  const tx = database.transaction(f.stores, "readwrite");
  for (const [store, rows] of Object.entries(records)) { tx.objectStore(store).clear(); rows.forEach((row) => tx.objectStore(store).put(row)); }
  await current.transactionDone(tx);
}

async function apply(database, input, plan, fail = false) {
  const tx = database.transaction(f.stores, "readwrite");
  for (const write of plan.metadataWrites) {
    assert.equal(write.kind, "recordTimerDependencies");
    tx.objectStore("meta").put({ key: "timerDependencies", value: write.value });
  }
  if (fail) tx.abort();
  await current.transactionDone(tx);
  return plan;
}

async function legacy(t) {
  const oldReceipts = JSON.parse(fs.readFileSync(path.join(temp, "pwa-core-045/ownership-receipts.json"), "utf8"));
  const source = f.meta(oldReceipts.receipts[0].before, "snapshot").canonicalTimer;
  const { client, core, open } = await f.fixture(t, { canonicalTimer: { ...source, id: "legacy-focus", startedByDeviceId: "p222-device" }, autoStartBreaks: true });
  const old = frozen(core);
  const finishInput = f.completionInput(client, false, { finishCommandId: "legacy-finish", breakCommandId: "legacy-break-start" });
  const before = await f.dump(client.use.database());
  const completeOldFinishReturn = await old.storage.finishTimer(client.use.database(), finishInput);
  assert.equal(completeOldFinishReturn.transitioned, true);
  const after = await f.dump(client.use.database());
  const completeOldReadReturn = await old.storage.readSyncState(client.use.database());
  return { client, core, open, old, before, after, finishInput, completeOldFinishReturn, completeOldReadReturn,
    originalReceiptProvenance: oldReceipts.provenance };
}

for (const name of names) {
  test(`actual old/current storage and native dependency plan ${name}`, async (t) => {
    const scenario = await legacy(t), { client, core, open, old } = scenario, database = client.use.database();
    const seeded = structuredClone(scenario.after), source = seeded.pending.find((row) => row.type === "finish");
    const start = seeded.pending.find((row) => row.generatedBreak), acknowledgements = [];
    const put = (key, value) => { seeded.meta = seeded.meta.filter((row) => row.key !== key).concat([{ key, value }]); };
    if (name === "all-five-raw") {
      const original = JSON.parse(fs.readFileSync(path.join(temp, "pwa-core-045/source-parity.json"), "utf8"));
      for (const [domain, store] of Object.entries(queueStores).slice(1)) {
        const record = original.receipts.find((receipt) => receipt.persistedAfter?.[store]?.length);
        assert.ok(record, `production receipt for ${domain}`);
        seeded[store] = structuredClone(record.persistedAfter[store]);
      }
      put("canonicalHead", { wallMs: f.nowMs, counter: 2 });
      put("projectionPending", Object.fromEntries(Object.entries(queueStores).map(([domain, store]) => [domain, seeded[store]])));
    }
    if (name.startsWith("sibling")) {
      seeded.pending.push({ ...start, id: "legacy-break-pause", type: "pause", generatedBreak: false,
        deviceSequence: start.deviceSequence + 1, hlcCounter: start.hlcCounter + 1, observedElapsedMs: 1234,
        extension: { number: 90.49999999999999, flags: [null, false] } });
      if (name === "sibling-proven") put("deliveryProof", { ...f.meta(seeded, "deliveryProof"), commands: [...f.meta(seeded, "deliveryProof").commands, "legacy-break-pause"] });
    }
    if (name === "missing-parent" || name.startsWith("canonical-source")) {
      seeded.pending = seeded.pending.filter((row) => row.id !== source.id);
      put("deliveryProof", { ...f.meta(seeded, "deliveryProof"), commands: f.meta(seeded, "deliveryProof").commands.filter((id) => id !== source.id) });
    }
    if (name.startsWith("canonical-source")) {
      const history = core.call("workspace.project.v1", { ...rawInput(scenario.after, client.state.localOwnerId).workspace, timerDependencies: [] }).workspace.history;
      put("snapshot", { ...f.meta(seeded, "snapshot"), canonicalTimer: null, history });
      const sent = f.sync.buildSyncBatch({ commands: [source] });
      put("outgoingSync", { ownerId: client.state.localOwnerId, requestId: "exact-saved-source", sent,
        body: " {\n\"deviceId\":\"p222-device\",\"requestId\":\"exact-saved-source\", " + JSON.stringify(sent).slice(1, -1) + "\n} ", metadata: { number: 90.49999999999999 } });
      acknowledgements.push({ commandId: source.id, outcome: name.split("-").at(-1), reason: "" });
    }
    await load(database, seeded);
    const before = await f.dump(database), input = rawInput(before, client.state.localOwnerId, acknowledgements);
    const result = native(input), plan = result.completeNativeReturn;
    const blocked = ["missing-parent", "sibling-frozen", "canonical-source-rejected"].includes(name);
    assert.equal(plan.outcome === "blocked", blocked);
    assert.deepEqual(plan.workspace.local, input.workspace.local); assert.deepEqual(plan.workspace.base, input.workspace.base);
    assert.deepEqual(plan.outgoing, input.outgoing); assert.deepEqual(plan.ownership, input.ownership);
    if (!blocked) {
      await assert.rejects(apply(database, input, plan, true)); assert.deepEqual(await f.dump(database), before);
    }
    assert.deepEqual(await apply(database, input, plan), plan);
    const after = await f.dump(database), reopened = await open();
    assert.deepEqual(await f.dump(reopened.use.database()), after);
    const restart = native(rawInput(after, client.state.localOwnerId, acknowledgements));
    assert.equal(restart.completeNativeReturn.outcome, blocked ? "blocked" : "noop");
    const completeCurrentReadReturn = await current.readSyncState(reopened.use.database());
    assert.deepEqual(completeCurrentReadReturn.commands, before.pending);
    await load(database, before);
    let originalError = null, completeOldReadReturn = null;
    try { completeOldReadReturn = await old.storage.readSyncState(database); } catch (error) { originalError = error.message; }
    assert.deepEqual(await f.dump(database), before);
    if (name === "missing-parent") {
      assert.equal(originalError, null); // readSyncState exposes raw records; projectState is the live reconstruction path.
      assert.throws(() => current.projectState({ snapshot: f.meta(before, "snapshot"), queues: input.workspace.local,
        deviceId: input.deviceId, nowMs: f.nowMs, sharedCore: core }), /Timer dependency has an invalid source time/);
      originalError = "Timer dependency has an invalid source time.";
    }
    receipts.push({ name, productionSourceSha256: old.sha256, originalReceiptProvenance: scenario.originalReceiptProvenance,
      legacyCreation: { before: scenario.before, input: JSON.parse(JSON.stringify(scenario.finishInput)), completeReturn: scenario.completeOldFinishReturn,
        after: scenario.after, completeReadReturn: scenario.completeOldReadReturn },
      persistedBefore: before, input, ...result, persistedAfter: after, completeCurrentReadReturn, completeOldReadReturn,
      originalError, restartInputRaw: restart.inputRaw, completeRestartReturn: restart.completeNativeReturn });
  });
}

test.after(() => {
  const output = { operation, receipts };
  fs.writeFileSync(path.join(temp, "core-pwa10-source-parity.json"), JSON.stringify(output, null, 2));
  if (capture) {
    assert.equal(receipts.length, names.length, "all production scenarios must pass before capture");
    assert.equal(fs.existsSync(fixtureFile), false, "raw fixture cannot be refreshed");
    fs.writeFileSync(fixtureFile, JSON.stringify(output, null, 2) + "\n");
  } else assert.deepEqual(output, expected, "complete production/native receipt drift");
});
