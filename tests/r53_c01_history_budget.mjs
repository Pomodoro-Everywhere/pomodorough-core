import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { invoke } from "./aggregate_artifact/abi_host.mjs";
import { nativeResponses, assertParity } from "./aggregate_artifact/native_oracle.mjs";

const artifact = process.argv[2];
assert.ok(artifact, "usage: node tests/r53_c01_history_budget.mjs <exact-release.wasm>");
const bytes = readFileSync(artifact);
const fixture = JSON.parse(readFileSync(new URL("../fixtures/read-model-v1.json", import.meta.url)));

// Reuse the R53 volume probe's unique, completed 60-second history rows.
function request(suppressed) {
  const input = structuredClone(fixture.request);
  input.profile = "pwaStorage";
  input.observedAt = "2026-07-20T12:00:00.000Z";
  input.calendarIntervals = [{ start: "2026-07-20T00:00:00Z", end: "2026-07-21T00:00:00Z" }];
  input.monotonic = null;
  const workspace = input.source.value;
  workspace.base.history = Array.from({ length: 100000 }, (_, index) => ({
    id: `h-${index}`, timerId: `t-${index}`, phase: "focus", status: "completed",
    plannedDurationMs: 60000, completedAt: "2026-07-20T11:00:00Z",
  }));
  workspace.canonicalHead = { wallMs: 1784548800000, counter: 0 };
  workspace.local.autoStartOperations = suppressed ? [{
    id: "claimed", deviceId: "d", occurredAt: "2026-07-20T12:00:00Z",
    hlcWallMs: 1784548800001, hlcCounter: 0, enabled: true,
  }] : [];
  return input;
}

function cases() {
  const control = request(false);
  const suppressed = request(true);
  const invalid = request(true);
  // Clock/identity are valid. The hidden setting payload itself is malformed.
  invalid.source.value.local.autoStartOperations[0].enabled = "true";
  const invalidDuration = request(true);
  const { enabled, ...clock } = invalidDuration.source.value.local.autoStartOperations.pop();
  invalidDuration.source.value.local.durationOperations = [{ ...clock, phase: "focus", durationMs: 1 }];
  return [
    ["100k-control", control, true], ["100k-suppressed", suppressed, true],
    ["100k-hidden-invalid", invalid, false],
    ["100k-hidden-invalid-duration", invalidDuration, false],
  ].map(([name, input, ok]) => ({ name, operation: "workspace.readModel.v1", input: JSON.stringify(input), ok }));
}

async function check(item, expected) {
  assert.ok(Buffer.byteLength(item.input) < 16 * 1024 * 1024, "input exceeds ABI cap");
  const { instance } = await WebAssembly.instantiate(bytes);
  const actual = invoke(instance.exports, item);
  assertParity(item, expected, actual);
  const result = JSON.parse(actual);
  assert.equal(result.ok, item.ok);
  if (item.ok) {
    assert.equal(result.value.cadence.completedFocusTotal, 100000);
    assert.equal(result.value.cadence.completedFocusToday, 100000);
    assert.equal(result.value.cadence.completedFocusTodayPlannedDurationMs, 6000000000);
  } else {
    assert.match(result.error, /boolean|invalid duration operation/);
  }
  const memoryBytes = instance.exports.memory.buffer.byteLength;
  assert.ok(memoryBytes <= 256 * 1024 * 1024, "linear memory exceeds fixed ABI budget");
  console.log(JSON.stringify({ name: item.name, inputBytes: Buffer.byteLength(item.input),
    outputBytes: Buffer.byteLength(actual), memoryBytes, rawParity: true }));
  return actual;
}

const items = cases();
const expected = nativeResponses(items);
const actual = [];
for (const [index, item] of items.entries()) actual.push(await check(item, expected[index]));
assert.equal(actual[0], actual[1], "suppressed setting changed the complete read result");
