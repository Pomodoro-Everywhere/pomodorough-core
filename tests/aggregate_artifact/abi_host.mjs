import assert from "node:assert/strict";
import { assertParity } from "./native_oracle.mjs";
import { runCorpus } from "./corpus.mjs";

const encoder = new TextEncoder();
const decoder = new TextDecoder("utf-8", { fatal: true });
const MAX_BUFFER_BYTES = 16 * 1024 * 1024;
const MAX_MEMORY_BYTES = 256 * 1024 * 1024;

function allocate(exports, text) {
  const bytes = encoder.encode(text);
  const pointer = exports.pomodorough_alloc(bytes.length) >>> 0;
  assert.notEqual(pointer, 0, "allocation failed");
  assert.ok(pointer + bytes.length <= exports.memory.buffer.byteLength, "allocation out of bounds");
  new Uint8Array(exports.memory.buffer, pointer, bytes.length).set(bytes);
  return { pointer, length: bytes.length };
}

function release(exports, buffer) {
  assert.equal(exports.pomodorough_free_v2(buffer.pointer, buffer.length + 1), 0, "wrong-length free accepted");
  assert.equal(exports.pomodorough_free_v2(buffer.pointer, buffer.length), 1, "free failed");
  assert.equal(exports.pomodorough_free_v2(buffer.pointer, buffer.length), 0, "double free accepted");
}

function resultBuffer(packed, memory) {
  assert.notEqual(packed, 0n, "dispatch allocation failed");
  const pointer = Number(packed & 0xffff_ffffn);
  const length = Number(packed >> 32n);
  assert.ok(pointer > 0 && length > 0 && length <= MAX_BUFFER_BYTES, "invalid result buffer");
  assert.ok(pointer + length <= memory.buffer.byteLength, "result out of bounds");
  return { pointer, length };
}

export function invoke(exports, item) {
  const operation = allocate(exports, item.operation);
  let input;
  let output;
  try {
    input = allocate(exports, item.input);
    output = resultBuffer(exports.pomodorough_dispatch(
      operation.pointer, operation.length, input.pointer, input.length), exports.memory);
    return decoder.decode(new Uint8Array(exports.memory.buffer, output.pointer, output.length).slice());
  } finally {
    if (output) release(exports, output);
    if (input) release(exports, input);
    release(exports, operation);
  }
}

export function replayCorpus(exports, corpus) {
  let cursor = 0;
  const result = runCorpus(corpus.staticCases, (item) => {
    const expectedCase = corpus.cases[cursor];
    assert.deepEqual(item, expectedCase, `stateful request ${cursor} differs from native trace`);
    const actual = invoke(exports, item);
    assertParity(item, corpus.expected[cursor++], actual);
    return actual;
  });
  assert.equal(cursor, corpus.cases.length, "stateful trace skipped dispatch");
  return result;
}

export function exerciseArtifact(exports, cases, expected, corpus = null) {
  for (const name of ["memory", "pomodorough_alloc", "pomodorough_dispatch", "pomodorough_free", "pomodorough_free_v2"]) {
    assert.ok(exports[name], `missing ABI export ${name}`);
  }
  const run = () => corpus ? replayCorpus(exports, corpus)
    : cases.forEach((item, index) => assertParity(item, expected[index], invoke(exports, item)));
  run();
  run();
  const warmed = exports.memory.buffer.byteLength;
  for (let repetition = 0; repetition < 3; repetition += 1) {
    run();
    assert.equal(exports.memory.buffer.byteLength, warmed, "aggregate success/failure dispatch leaked memory");
  }
  assert.ok(warmed <= MAX_MEMORY_BYTES, "memory exceeds ABI cap");
  const legacy = allocate(exports, "legacy free");
  exports.pomodorough_free(legacy.pointer, legacy.length);
  assert.equal(exports.pomodorough_free_v2(legacy.pointer, legacy.length), 0, "legacy free retained allocation");
}
