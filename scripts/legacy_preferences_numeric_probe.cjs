"use strict";
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { spawnSync } = require("node:child_process");
const { isDeepStrictEqual } = require("node:util");
const { f, native, rawInput, load, apply, oldMigrations, temp } = require("./legacy_preferences_source_probe.cjs");
const evidence = [];
const checker = process.env.PWA11_CHECKER_DIR || path.join(temp, "core-pwa11-checker-1CHNfg");
const durationReceipt = JSON.parse(fs.readFileSync(path.join(checker, "independent-numeric-parity-reject.json"), "utf8"));
const retainedReceipts = JSON.parse(fs.readFileSync(path.join(checker, "independent-raw-retention-reject.json"), "utf8")).receipts;

test("exact checker receipt preserves numeric duration quantization in every phase", () => {
  const result = native(durationReceipt.inputRaw);
  assert.equal(result.envelope.ok, true, result.envelope.error);
  evidence.push({ name: "checker-duration", inputRaw: result.raw, completeNativeEnvelope: result.envelope,
    originalAfter: durationReceipt.originalAfter, originalReturns: durationReceipt.completeOriginalReturns });
  assert.deepEqual(result.envelope.value.operations.durationOperations, durationReceipt.originalAfter.pendingDurations);
});

for (const receipt of retainedReceipts) test(`exact checker raw preservation ${receipt.name}`, () => {
  const result = native(receipt.inputRaw), input = JSON.parse(receipt.inputRaw);
  assert.equal(result.envelope.ok, true, result.envelope.error);
  evidence.push({ name: receipt.name, inputRaw: result.raw, decodedInput: input,
    completeNativeEnvelope: result.envelope, rejectedPriorEnvelope: receipt.completeNativeEnvelope });
  const output = result.envelope.value;
  if (receipt.name === "string-control") assert.equal(output.operations.durationOperations[0].durationMs, 5400000);
  else if (receipt.name === "noop-unknown-preference") {
    assert.deepEqual(output.settings, input.settings);
    assert.deepEqual(output.workspace.local, input.workspace.local);
    assert.deepEqual(output.outgoing, input.outgoing);
  } else {
    for (const [domain, retained] of Object.entries(input.workspace.local)) {
      assert.deepEqual(output.workspace.local[domain].slice(0, retained.length), retained);
    }
    assert.deepEqual(output.outgoing, input.outgoing);
  }
});

for (const [name, value] of [["below", 90.49999999999999], ["exact", 90.5], ["above", 90.50000000000001]]) {
  for (const representation of ["number", "string"]) test(`frozen Git JSON ${name} ${representation} all phases`, async (t) => {
    const { client, core } = await f.fixture(t), database = client.use.database();
    const rawValue = representation === "string" ? String(value) : value;
    await f.seedMeta(database, { settings: { durations: { focus: rawValue, short_break: rawValue, long_break: rawValue },
      peerOnlySetting: { number: value, array: [value, null] } } });
    const before = await f.dump(database), input = rawInput(before, client.state.localOwnerId), result = native(input);
    assert.equal(result.envelope.ok, true, result.envelope.error);
    const originalReturn = await oldMigrations(client, core, true), originalAfter = await f.dump(database);
    evidence.push({ name: `${name}-${representation}`, before, inputRaw: result.raw, decodedInput: JSON.parse(result.raw),
      completeOriginalReturn: originalReturn, originalAfter, completeNativeEnvelope: result.envelope });
    assert.deepEqual(result.envelope.value.operations.durationOperations, originalAfter.pendingDurations);
    assert.deepEqual(result.envelope.value.settings, f.meta(originalAfter, "settings"));
    await load(database, before);
    const fullReturn = await apply(database, input, result.envelope.value);
    assert.deepEqual(fullReturn, result.envelope.value);
    assert.deepEqual(f.meta(await f.dump(database), "settings"), f.meta(originalAfter, "settings"));
  });
}

test("no-op raw claims and unknown numeric fields survive host JSON decoding and IndexedDB reads", async (t) => {
  const { client, core } = await f.fixture(t), database = client.use.database(), number = 90.49999999999999;
  const retained = { id: "possibly-delivered", phase: "focus", durationMs: 1800000,
    occurredAt: "1970-01-01T00:00:01Z", hlcWallMs: 0, hlcCounter: 0, extension: { number, array: [number, null] } };
  const outgoing = { ownerId: client.state.localOwnerId, sent: { durationOperations: [retained] },
    body: ' { "durationOperations": [' + JSON.stringify(retained) + '] } ', extension: { number } };
  await f.seedQueues(database, { durationOperations: [retained] });
  await f.seedMeta(database, { settings: { durationSyncBootstrapped: true, autoStartSyncBootstrapped: true,
    selectedTaskSyncBootstrapped: true, peerOnlySetting: { number } }, outgoingSync: outgoing,
    projectionPending: f.sync.emptyNeverSent(), deliveryProof: f.sync.emptyNeverSent() });
  const before = await f.dump(database), input = rawInput(before, client.state.localOwnerId);
  input.identities.operationUuids = [];
  const result = native(input); assert.equal(result.envelope.ok, true, result.envelope.error);
  const originalReturn = await oldMigrations(client, core, true), originalAfter = await f.dump(database);
  evidence.push({ name: "numeric-noop-claimed", before, inputRaw: result.raw, decodedInput: JSON.parse(result.raw),
    completeOriginalReturn: originalReturn, originalAfter, completeNativeEnvelope: result.envelope });
  assert.deepEqual(result.envelope.value.settings, input.settings);
  assert.deepEqual(result.envelope.value.workspace, input.workspace);
  assert.deepEqual(result.envelope.value.outgoing, input.outgoing);
  const fullReturn = await apply(database, input, result.envelope.value);
  assert.deepEqual(fullReturn, result.envelope.value);
  assert.deepEqual(await f.dump(database), before);
  assert.deepEqual(originalAfter, before);
});

function neighbor(value, steps) {
  const bits = new DataView(new ArrayBuffer(8));
  bits.setFloat64(0, value);
  bits.setBigUint64(0, bits.getBigUint64(0) + BigInt(steps));
  return bits.getFloat64(0);
}

function nativeMany(inputs) {
  const command = process.env.PWA11_NATIVE_ORACLE || "rustup";
  const args = process.env.PWA11_NATIVE_ORACLE ? [] : ["run", "1.97.1", "cargo", "run", "--quiet", "--locked", "--example", "artifact_parity_oracle"];
  const result = spawnSync(command, args, { cwd: path.resolve(__dirname, ".."), encoding: "utf8", timeout: 120000,
    maxBuffer: 64 * 1024 * 1024, input: inputs.map((input) => JSON.stringify({ operation: "workspace.legacyPreferences.v1", input })).join("\n") + "\n" });
  assert.equal(result.status, 0, result.stderr);
  const outputs = result.stdout.trimEnd().split("\n").map((raw) => ({ raw, envelope: JSON.parse(raw) }));
  assert.equal(outputs.length, inputs.length);
  return outputs;
}

test("3759 raw numeric boundary cases match actual frozen Git migrations", async (t) => {
  const { client, core } = await f.fixture(t), database = client.use.database();
  const seed = await f.dump(database), cases = [];
  for (let minute = 1; minute <= 179; minute += 1) for (let step = -3; step <= 3; step += 1) {
    for (const phase of ["focus", "short_break", "long_break"]) {
      const number = neighbor(minute + 0.5, step), before = structuredClone(seed);
      before.meta.find((record) => record.key === "settings").value = { durations: { [phase]: number }, peerOnlySetting: { number } };
      await load(database, before);
      const input = rawInput(before, client.state.localOwnerId);
      assert.deepEqual(JSON.parse(JSON.stringify(input)), input);
      const originalReturn = await oldMigrations(client, core, true), originalAfter = await f.dump(database);
      cases.push({ phase, minute, step, inputRaw: JSON.stringify(input), originalReturn,
        operations: originalAfter.pendingDurations, settings: f.meta(originalAfter, "settings") });
    }
  }
  assert.equal(cases.length, 3759);
  const outputs = nativeMany(cases.map((item) => item.inputRaw)), failures = [];
  let quantizationFailures = 0, preservationFailures = 0;
  for (const [index, original] of cases.entries()) {
    const output = outputs[index].envelope;
    if (!isDeepStrictEqual(output.value?.operations.durationOperations, original.operations)) quantizationFailures += 1;
    if (!isDeepStrictEqual(output.value?.settings, original.settings)) preservationFailures += 1;
    try {
      assert.equal(output.ok, true, output.error);
      assert.deepEqual(output.value.operations.durationOperations, original.operations);
      assert.deepEqual(output.value.settings, original.settings);
    } catch (error) { failures.push({ ...original, completeNativeEnvelope: output, error: error.message }); }
  }
  fs.writeFileSync(path.join(temp, process.env.PWA11_NATIVE_ORACLE ? "core-pwa11-numeric-sweep-red.json" : "core-pwa11-numeric-sweep-green.json"),
    JSON.stringify({ cases, outputs }, null, 2));
  evidence.push({ name: "numeric-boundary-sweep", total: cases.length, failures: failures.length,
    quantizationFailures, preservationFailures, receipts: failures });
  assert.equal(failures.length, 0, `${failures.length}/3759 boundary cases differ`);
});

test.after(() => fs.writeFileSync(path.join(temp, process.env.PWA11_NATIVE_ORACLE
  ? "core-pwa11-numeric-source-red.json" : "core-pwa11-numeric-source-green.json"), JSON.stringify({ evidence }, null, 2)));
