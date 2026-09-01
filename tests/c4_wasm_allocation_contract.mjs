import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const MAX_OPERATION_BYTES = 256;
const MAX_BUFFER_BYTES = 16 * 1024 * 1024;
const MAX_MEMORY_BYTES = 256 * 1024 * 1024;
const MEMORY_GROWTH_SLACK = 2 * 1024 * 1024;
const encoder = new TextEncoder();
const decoder = new TextDecoder("utf-8", { fatal: true });

const wasm = await readFile(process.argv[2]);
const { instance } = await WebAssembly.instantiate(wasm);
const {
  memory,
  pomodorough_alloc: alloc,
  pomodorough_free: free,
  pomodorough_free_v2: freeV2,
  pomodorough_dispatch: dispatch,
} = instance.exports;

for (const name of [
  "memory",
  "pomodorough_alloc",
  "pomodorough_free",
  "pomodorough_free_v2",
  "pomodorough_dispatch",
]) {
  assert.ok(instance.exports[name], `missing export ${name}`);
}

function allocateLength(length) {
  return alloc(length) >>> 0;
}

function allocateBytes(bytes) {
  const pointer = allocateLength(bytes.length);
  assert.notEqual(pointer, 0, `allocation failed for ${bytes.length} bytes`);
  new Uint8Array(memory.buffer, pointer, bytes.length).set(bytes);
  return { pointer, length: bytes.length };
}

function releaseBuffer(buffer) {
  assert.equal(freeV2(buffer.pointer, buffer.length), 1);
}

function unpack(packed) {
  assert.notEqual(packed, 0n, "dispatch result allocation failed");
  const pointer = Number(packed & 0xffff_ffffn) >>> 0;
  const length = Number((packed >> 32n) & 0xffff_ffffn) >>> 0;
  assert.notEqual(pointer, 0);
  assert.ok(length > 0 && length <= MAX_BUFFER_BYTES);
  const bytes = new Uint8Array(memory.buffer, pointer, length).slice();
  assert.equal(freeV2(pointer, length), 1);
  return JSON.parse(decoder.decode(bytes));
}

function invoke(operation, input = "{}", override = {}) {
  const operationBuffer = allocateBytes(encoder.encode(operation));
  const inputBuffer = allocateBytes(encoder.encode(input));
  const packed = dispatch(
    override.operationPointer ?? operationBuffer.pointer,
    override.operationLength ?? operationBuffer.length,
    override.inputPointer ?? inputBuffer.pointer,
    override.inputLength ?? inputBuffer.length,
  );
  const result = unpack(packed);
  releaseBuffer(inputBuffer);
  releaseBuffer(operationBuffer);
  return result;
}

function assertControlledRangeFailure(result) {
  assert.equal(result.ok, false);
  assert.match(result.error, /binding (operation|input) range is invalid/);
}

function assertNonOverlapping(buffers) {
  for (let left = 0; left < buffers.length; left += 1) {
    for (let right = left + 1; right < buffers.length; right += 1) {
      const first = buffers[left];
      const second = buffers[right];
      const separated = first.pointer + first.length <= second.pointer
        || second.pointer + second.length <= first.pointer;
      assert.ok(separated, `allocations ${left} and ${right} overlap`);
    }
  }
}

const initialMemory = memory.buffer.byteLength;
for (let repetition = 0; repetition < 8; repetition += 1) {
  for (const rejected of [0, MAX_BUFFER_BYTES + 1, 0xffff_ffff]) {
    assert.equal(allocateLength(rejected), 0, `length ${rejected} must be rejected`);
  }
}
assert.equal(memory.buffer.byteLength, initialMemory, "rejected allocations grew memory");

for (const accepted of [MAX_BUFFER_BYTES - 1, MAX_BUFFER_BYTES]) {
  const pointer = allocateLength(accepted);
  assert.notEqual(pointer, 0, `boundary allocation ${accepted} failed`);
  assert.ok(pointer + accepted <= memory.buffer.byteLength);
  assert.equal(freeV2(pointer, accepted), 1);
}
const boundaryMemory = memory.buffer.byteLength;
assert.ok(boundaryMemory <= initialMemory + MAX_BUFFER_BYTES + MEMORY_GROWTH_SLACK);
assert.ok(boundaryMemory <= MAX_MEMORY_BYTES);

for (let repetition = 0; repetition < 32; repetition += 1) {
  const pointer = allocateLength(4096);
  assert.notEqual(pointer, 0);
  assert.equal(freeV2(pointer, 4096), 1);
}
assert.equal(memory.buffer.byteLength, boundaryMemory, "reused allocations grew memory");

const liveBuffers = [1, 2, 3, 8, 16, 32, 64, 257, 4096].map((length) => ({
  pointer: allocateLength(length),
  length,
}));
for (const buffer of liveBuffers) {
  assert.notEqual(buffer.pointer, 0);
  assert.equal(Number.isInteger(buffer.pointer), true);
  assert.ok(buffer.pointer + buffer.length <= memory.buffer.byteLength);
}
assert.equal(new Set(liveBuffers.map(({ pointer }) => pointer)).size, liveBuffers.length);
assertNonOverlapping(liveBuffers);
for (const buffer of liveBuffers.reverse()) {
  releaseBuffer(buffer);
}

const wrongLength = { pointer: allocateLength(64), length: 64 };
assert.equal(freeV2(wrongLength.pointer, 63), 0);
assert.equal(freeV2(wrongLength.pointer, wrongLength.length), 1);
assert.equal(freeV2(wrongLength.pointer, wrongLength.length), 0);
free(0, 1);

const operation = "core.version";
const malformedLengths = [
  { operationPointer: 0, operationLength: 1 },
  { operationLength: 0 },
  { operationLength: MAX_OPERATION_BYTES + 1 },
  { operationPointer: 0xffff_ffff, operationLength: 1 },
  { operationLength: 0xffff_ffff },
  { inputPointer: 0, inputLength: 1 },
  { inputLength: 0 },
  { inputLength: MAX_BUFFER_BYTES + 1 },
  { inputPointer: 0xffff_ffff, inputLength: 1 },
  { inputLength: 0xffff_ffff },
];
for (const override of malformedLengths) {
  assertControlledRangeFailure(invoke(operation, "{}", override));
}

const maximumOperation = allocateBytes(encoder.encode("x".repeat(MAX_OPERATION_BYTES)));
const maximumOperationInput = allocateBytes(encoder.encode("{}"));
assert.match(unpack(dispatch(
  maximumOperation.pointer,
  maximumOperation.length,
  maximumOperationInput.pointer,
  maximumOperationInput.length,
)).error, /unsupported .*core operation/);
releaseBuffer(maximumOperationInput);
releaseBuffer(maximumOperation);

const capOperation = allocateBytes(encoder.encode(operation));
const capInput = { pointer: allocateLength(MAX_BUFFER_BYTES), length: MAX_BUFFER_BYTES };
assert.notEqual(capInput.pointer, 0);
assert.deepEqual(unpack(dispatch(
  capOperation.pointer,
  capOperation.length,
  capInput.pointer,
  capInput.length,
)), { ok: true, value: { schemaVersion: 1, coreVersion: "0.10.0" } });
releaseBuffer(capInput);
releaseBuffer(capOperation);

const operations = [
  "core.version",
  "timer.reduce",
  "timer.reduce.v1",
  "projection.reduce",
  "projection.apply.v2",
  "task.reduce.v1",
  "task.identity.v1",
  "duration.reduce.v1",
  "autoStart.reduce.v1",
  "selectedTask.reduce",
  "selectedTask.reduce.v1",
  "selectedTask.classify",
  "reconcile.rebase.v1",
  "bootstrap.plan.v1",
  "timer.completionPlan.v1",
  "hlc.head.v1",
  "hlc.tick.v1",
  "uuidv7.fromParts.v1",
];
for (const exportedOperation of operations) {
  const result = invoke(exportedOperation);
  assert.equal(typeof result.ok, "boolean", `${exportedOperation} lacked an envelope`);
  if (!result.ok) {
    assert.equal(typeof result.error, "string");
    assert.ok(result.error.length > 0);
    assert.doesNotMatch(result.error, /unsupported .*core operation/);
  }
}
assert.match(invoke("c4.unsupported").error, /unsupported .*core operation/);
assert.equal(invoke(operation).ok, true, "valid dispatch failed after rejected requests");
assert.ok(memory.buffer.byteLength <= MAX_MEMORY_BYTES);
