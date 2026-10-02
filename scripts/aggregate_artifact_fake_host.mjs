// Transfer-buffer test double. It returns supplied envelopes without domain policy.
export function fakeHost(envelopes, { trap = false, grow = false } = {}) {
  const memory = { buffer: new ArrayBuffer(65536) };
  const live = new Map();
  let next = 8;
  let calls = 0;
  const alloc = (length) => {
    if (live.size === 0) next = 8;
    const pointer = next;
    next += length + 8;
    live.set(pointer, length);
    return pointer;
  };
  const freeV2 = (pointer, length) => {
    if (live.get(pointer) !== length) return 0;
    new Uint8Array(memory.buffer, pointer, length).fill(0);
    live.delete(pointer);
    return 1;
  };
  const dispatch = () => {
    if (trap) throw new Error("test dispatch trap");
    if (grow) {
      const previous = memory.buffer;
      memory.buffer = new ArrayBuffer(previous.byteLength + 65536);
      new Uint8Array(memory.buffer).set(new Uint8Array(previous));
    }
    const bytes = new TextEncoder().encode(envelopes[calls++ % envelopes.length]);
    const pointer = alloc(bytes.length);
    new Uint8Array(memory.buffer, pointer, bytes.length).set(bytes);
    return (BigInt(bytes.length) << 32n) | BigInt(pointer);
  };
  return { live, exports: { memory, pomodorough_alloc: alloc,
    pomodorough_dispatch: dispatch, pomodorough_free_v2: freeV2,
    pomodorough_free: (pointer, length) => { freeV2(pointer, length); } } };
}
