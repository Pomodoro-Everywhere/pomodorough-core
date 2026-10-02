import assert from "node:assert/strict";
import test from "node:test";
import { aggregateCases } from "../tests/aggregate_artifact/catalog.mjs";
import { operations } from "../tests/aggregate_artifact/cases.mjs";
import { assertParity, validateEnvelopes } from "../tests/aggregate_artifact/native_oracle.mjs";
import { exerciseArtifact, invoke } from "../tests/aggregate_artifact/abi_host.mjs";
import { fakeHost } from "./aggregate_artifact_fake_host.mjs";
import "./test_aggregate_runner_mutants.mjs";

const cases = aggregateCases();
const success = '{"ok":true,"value":{"retained":[null,"",{}]}}';
const failure = '{"error":"invalid shared-core input: rejected","ok":false}';
const synthetic = operations.flatMap((operation) => [
  { operation, name: "success", input: "{}", ok: true },
  { operation, name: "failure", input: "{", ok: false },
]);
const envelopes = synthetic.map((item) => item.ok ? success : failure);

test("catalog is deterministic, uniquely named, and covers all new operations", () => {
  assert.deepEqual(aggregateCases(), cases);
  assert.equal(new Set(cases.map((item) => `${item.operation}/${item.name}`)).size, cases.length);
  assert.deepEqual(new Set(cases.map((item) => item.operation)), new Set(operations));
  for (const operation of operations) {
    const selected = cases.filter((item) => item.operation === operation);
    assert.ok(selected.some((item) => item.ok), operation);
    assert.ok(selected.some((item) => !item.ok), operation);
    for (const name of ["duplicate-root", "duplicate-nested", "unknown-control", "unsafe-integer", "integer-overflow"]) {
      assert.equal(selected.find((item) => item.name === name)?.ok, false, `${operation}/${name}`);
    }
  }
});

test("raw inputs preserve duplicate keys and fractional integer tokens", () => {
  for (const item of cases.filter((item) => item.name.startsWith("duplicate-"))) {
    assert.equal(item.input.match(/"artifactDuplicate":/g).length, 2);
  }
  const fractional = cases.find((item) => item.name === "android-rounded-fraction");
  assert.ok(fractional.input.includes("20000.000000000001"));
  const overflow = cases.filter((item) => item.name === "integer-overflow");
  assert.equal(overflow.length, operations.length);
  assert.ok(overflow.every((item) => item.input.includes("18446744073709551616")));
});

test("native responses require complete envelopes and both outcomes per operation", () => {
  assert.deepEqual(validateEnvelopes(synthetic, envelopes.join("\n") + "\n"), envelopes);
  assert.throws(() => validateEnvelopes(synthetic, envelopes.slice(1).join("\n")), /response count/);
  const missing = synthetic.slice(2);
  assert.throws(() => validateEnvelopes(missing, envelopes.slice(2).join("\n")), /needs success and failure/);
  const partial = [...envelopes];
  partial[0] = '{"ok":true}';
  assert.throws(() => validateEnvelopes(synthetic, partial.join("\n")));
});

test("native oracle cannot bless unsupported dispatch or wrong outcomes", () => {
  const unsupported = [...envelopes];
  unsupported[1] = '{"error":"unsupported shared-core operation: missing","ok":false}';
  assert.throws(() => validateEnvelopes(synthetic, unsupported.join("\n")), /unsupported/);
  const wrong = [...envelopes];
  wrong[0] = failure;
  assert.throws(() => validateEnvelopes(synthetic, wrong.join("\n")), /success/);
});

test("parity rejects missing, extra, changed fields and changed errors", () => {
  assertParity(synthetic[0], success, success);
  for (const actual of [
    '{"ok":true,"value":{}}',
    '{"ok":true,"value":{"retained":[null,"",{}],"extra":null}}',
    '{"ok":true,"value":{"retained":["",null,{}]}}',
    failure,
  ]) assert.throws(() => assertParity(synthetic[0], success, actual));
  assert.throws(() => assertParity(synthetic[1], failure, failure.replace("rejected", "other")));
});

test("raw comparison detects numbers collapsed by JavaScript parsing", () => {
  const native = '{"ok":true,"value":9007199254740991.1}';
  const rounded = '{"ok":true,"value":9007199254740991}';
  assert.deepEqual(JSON.parse(native), JSON.parse(rounded));
  assert.throws(() => assertParity(synthetic[0], native, rounded), /raw envelope/);
});

test("ABI copies output before checked frees and cleans both outcomes", () => {
  const host = fakeHost([success, failure]);
  assert.equal(invoke(host.exports, synthetic[0]), success);
  assert.equal(host.live.size, 0);
  assert.equal(invoke(host.exports, synthetic[1]), failure);
  assert.equal(host.live.size, 0);
});

test("ABI cleans input allocations after dispatch trap", () => {
  const host = fakeHost([success], { trap: true });
  assert.throws(() => invoke(host.exports, synthetic[0]), /test dispatch trap/);
  assert.equal(host.live.size, 0);
});

test("ABI rejects null result and incorrect free contract", () => {
  const host = fakeHost([success]);
  host.exports.pomodorough_dispatch = () => 0n;
  assert.throws(() => invoke(host.exports, synthetic[0]), /dispatch allocation failed/);
  assert.equal(host.live.size, 0);
  const broken = fakeHost([success]);
  broken.exports.pomodorough_free_v2 = () => 1;
  assert.throws(() => invoke(broken.exports, synthetic[0]), /wrong-length free accepted/);
});

test("artifact loop enforces stable memory after success/failure warmup", () => {
  const host = fakeHost(envelopes);
  exerciseArtifact(host.exports, synthetic, envelopes);
  assert.equal(host.live.size, 0);
  const growing = fakeHost(envelopes, { grow: true });
  assert.throws(() => exerciseArtifact(growing.exports, synthetic, envelopes), /leaked memory/);
});
