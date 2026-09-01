import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const wasm = await readFile(process.argv[2]);
const { instance } = await WebAssembly.instantiate(wasm);
const encoder = new TextEncoder();
const decoder = new TextDecoder("utf-8", { fatal: true });

function allocate(bytes) {
  const pointer = Number(instance.exports.pomodorough_alloc(bytes.length));
  new Uint8Array(instance.exports.memory.buffer, pointer, bytes.length).set(bytes);
  return pointer;
}

function invoke(operation, input) {
  const operationBytes = encoder.encode(operation);
  const inputBytes = encoder.encode(input);
  const operationPointer = allocate(operationBytes);
  const inputPointer = allocate(inputBytes);
  const packed = instance.exports.pomodorough_dispatch(
    operationPointer,
    operationBytes.length,
    inputPointer,
    inputBytes.length,
  );
  const outputPointer = Number(packed & 0xffff_ffffn);
  const outputLength = Number((packed >> 32n) & 0xffff_ffffn);
  const output = new Uint8Array(
    instance.exports.memory.buffer,
    outputPointer,
    outputLength,
  ).slice();
  instance.exports.pomodorough_free_v2(operationPointer, operationBytes.length);
  instance.exports.pomodorough_free_v2(inputPointer, inputBytes.length);
  instance.exports.pomodorough_free_v2(outputPointer, outputLength);
  return JSON.parse(decoder.decode(output));
}

const durations = { focus: 1_500_000, short_break: 300_000, long_break: 900_000 };
const queues = {
  commands: [],
  taskOperations: [],
  durationOperations: [],
  autoStartOperations: [],
  selectedTaskOperations: [],
};
const response = {
  acknowledgements: [],
  taskAcknowledgements: [],
  durationAcknowledgements: [],
  autoStartAcknowledgements: [],
  selectedTaskAcknowledgements: [],
  revision: 1,
  canonicalTimer: null,
  history: [],
  tasks: [],
  durationsMs: durations,
  autoStartBreaks: false,
  selectedTaskId: null,
  serverTime: "2026-07-20T12:00:10Z",
  serverHlcWallMs: 1_784_548_810_000,
  serverHlcCounter: 0,
};

const duplicate = invoke(
  "projection.apply.v2",
  `{"base":{"durationsMs":{"focus":1500000,"short_break":300000,"long_break":900000},"autoStartBreaks":false},"pending":{"durationOperations":[{"metadata":0,"metadata":1}]},"now":"2026-08-22T12:00:00Z"}`,
);
assert.equal(duplicate.ok, false);
assert.match(duplicate.error, /duplicate field `metadata`/);

const wrongContainer = invoke(
  "reconcile.rebase.v1",
  JSON.stringify({ local: [], sent: queues, response }),
);
assert.equal(wrongContainer.ok, false);
assert.match(wrongContainer.error, /local must be a JSON object/);

assert.equal(
  invoke(
    "projection.apply.v2",
    JSON.stringify({
      base: { durationsMs: durations, autoStartBreaks: false },
      pending: {},
      now: "2026-08-22T12:00:00Z",
    }),
  ).ok,
  true,
);
assert.equal(
  invoke("reconcile.rebase.v1", JSON.stringify({ pending: {}, sent: {}, response })).ok,
  true,
);
