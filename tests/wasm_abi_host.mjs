import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const wasm = await readFile(process.argv[2]);
const { instance } = await WebAssembly.instantiate(wasm);
const {
  memory,
  pomodorough_alloc: alloc,
  pomodorough_free: free,
  pomodorough_dispatch: dispatch,
} = instance.exports;

for (const name of [
  "memory",
  "pomodorough_alloc",
  "pomodorough_free",
  "pomodorough_dispatch",
]) {
  assert.ok(instance.exports[name], `missing export ${name}`);
}

function allocate(bytes) {
  const pointer = alloc(bytes.length) >>> 0;
  assert.notEqual(pointer, 0);
  new Uint8Array(memory.buffer, pointer, bytes.length).set(bytes);
  return pointer;
}

function release(buffer) {
  free(buffer.pointer, buffer.bytes.length);
}

function unpack(packed) {
  const pointer = Number(packed & 0xffff_ffffn) >>> 0;
  const length = Number((packed >> 32n) & 0xffff_ffffn) >>> 0;
  const bytes = new Uint8Array(memory.buffer, pointer, length).slice();
  free(pointer, length);
  return JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
}

function invoke(operationBytes, inputBytes, override = {}) {
  const operation = { bytes: operationBytes, pointer: allocate(operationBytes) };
  const input = { bytes: inputBytes, pointer: allocate(inputBytes) };
  const packed = dispatch(
    override.operationPointer ?? operation.pointer,
    override.operationLength ?? operation.bytes.length,
    override.inputPointer ?? input.pointer,
    override.inputLength ?? input.bytes.length,
  );
  const result = unpack(packed);
  release(input);
  release(operation);
  return result;
}

const encoder = new TextEncoder();
const operation = encoder.encode("core.version");
const input = encoder.encode("{}");
assert.deepEqual(invoke(operation, input), {
  ok: true,
  value: { schemaVersion: 1, coreVersion: "0.1.4" },
});
assert.deepEqual(
  invoke(
    encoder.encode("hlc.head.v1"),
    encoder.encode(JSON.stringify({
      physicalNowMs: 100,
      observed: [
        { wallMs: 101, counter: 2 },
        { wallMs: 101, counter: 7 },
        { wallMs: 99, counter: 99 },
      ],
    })),
  ),
  { ok: true, value: { wallMs: 101, counter: 7 } },
);

for (const override of [
  { operationPointer: 0, operationLength: 1 },
  { operationLength: 0 },
  { operationLength: 0xffff_ffff },
  { inputPointer: 0, inputLength: 1 },
  { inputLength: 0 },
  { inputLength: 0xffff_ffff },
]) {
  const result = invoke(operation, input, override);
  assert.equal(result.ok, false);
  assert.match(result.error, /binding (operation|input) range is invalid/);
}

for (const invalid of [
  [new Uint8Array([0xff]), input],
  [operation, new Uint8Array([0xff])],
]) {
  assert.deepEqual(invoke(...invalid), {
    ok: false,
    error: "binding input is not UTF-8",
  });
}

const live = { bytes: operation, pointer: allocate(operation) };
const liveInput = { bytes: input, pointer: allocate(input) };
free(0, 1);
free(live.pointer, 0);
free(live.pointer, 0xffff_ffff);
assert.equal(
  unpack(dispatch(live.pointer, live.bytes.length, liveInput.pointer, input.length)).ok,
  true,
);
release(liveInput);
free(live.pointer, live.bytes.length);
free(live.pointer, live.bytes.length);
assert.equal(invoke(operation, input).ok, true);
