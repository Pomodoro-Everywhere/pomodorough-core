import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { operations } from "./cases.mjs";
import { assertSemantics } from "./semantics.mjs";

export function validateResponses(cases, output) {
  const envelopes = output.trimEnd().split("\n");
  assert.equal(envelopes.length, cases.length, "native oracle response count");
  for (const [index, item] of cases.entries()) {
    const envelope = JSON.parse(envelopes[index]);
    const label = `${item.operation}/${item.name}`;
    assert.equal(envelope.ok, item.ok, `${label}: ${envelope.error ?? "unexpected success"}`);
    assert.deepEqual(Object.keys(envelope).sort(), item.ok ? ["ok", "value"] : ["error", "ok"], label);
    if (!item.ok) {
      assert.equal(typeof envelope.error, "string", label);
      assert.ok(envelope.error.length > 0, label);
      assert.doesNotMatch(envelope.error, /unsupported shared-core operation/, `${label}: ${envelope.error}`);
      if (item.error) assert.ok(envelope.error.includes(item.error), label);
    }
    if (envelope.ok) assertSemantics(item, envelope.value);
  }
  return envelopes;
}

export function validateEnvelopes(cases, output) {
  const envelopes = validateResponses(cases, output);
  const coverage = new Map();
  for (const item of cases) {
    const covered = coverage.get(item.operation) ?? new Set();
    covered.add(item.ok);
    coverage.set(item.operation, covered);
  }
  for (const operation of operations) {
    assert.deepEqual(coverage.get(operation), new Set([true, false]), `${operation} needs success and failure`);
  }
  return envelopes;
}

export function nativeResponses(cases) {
  const input = cases.map(({ operation, input }) => JSON.stringify({ operation, input })).join("\n") + "\n";
  const result = spawnSync("rustup", ["run", "1.97.1", "cargo", "run", "--quiet", "--locked", "--example", "artifact_parity_oracle"], {
    cwd: fileURLToPath(new URL("../../", import.meta.url)), input, encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024, timeout: 120000,
  });
  assert.ifError(result.error && new Error(`${result.error.message}: ${result.stderr}`));
  assert.equal(result.status, 0, `native oracle failed: ${result.stderr}`);
  return validateResponses(cases, result.stdout);
}

export function nativeEnvelopes(cases) {
  const envelopes = nativeResponses(cases);
  return validateEnvelopes(cases, envelopes.join("\n"));
}

export function assertParity(item, expected, actual) {
  const label = `${item.operation}/${item.name}`;
  assert.deepEqual(JSON.parse(actual), JSON.parse(expected), label);
  // Also compare raw envelopes: JS parsing must not hide numeric token drift.
  assert.equal(actual, expected, `${label} raw envelope`);
}
