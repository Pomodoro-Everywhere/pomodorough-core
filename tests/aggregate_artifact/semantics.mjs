import assert from "node:assert/strict";
import { vector } from "./cases.mjs";

export const requiredHits = {
  queued: 12, generated: 6, childPause: 3, childFinish: 3, barrierRebase: 3, barrier: 3, rejection: 6,
  desktopDeferred: 4, appleExplicit: 4, fractionalRead: 3, missingReading: 1,
  taskTotals: 3, savedOrder: 1, oversizedSaved: 1, cursorRotation: 10, batchDrain: 4,
  terminalRebase: 19, terminalPartial: 1, terminalBarrier: 1, terminalRejection: 1,
  terminalPromotion: 1, terminalComposition: 2, terminalNormalization: 1,
  terminalMissingHistory: 6, workspaceMissingHistory: 6,
  pwaDisplay: 4, pwaLifecycle: 16, pwaRebase: 4,
  pwaMatrix: 18, pwaParity: 8,
};

export function branch(operation, name, input, hit, checks) {
  return { ...vector(operation, name, input), hit, checks };
}

export function at(value, path) {
  const [key, ...rest] = path.split(".");
  if (key === "*") return Object.values(value).map((item) => rest.length ? at(item, rest.join(".")) : item);
  const selected = value?.[key];
  return rest.length ? at(selected, rest.join(".")) : selected;
}

export function assertSemantics(item, value) {
  if (!item.hit) return;
  const label = `${item.name} semantic`;
  assert.ok(Object.keys(item.checks ?? {}).length > 0, `${label} checks missing`);
  for (const [path, expected] of Object.entries(item.checks.equals ?? {})) {
    assert.deepEqual(at(value, path), expected, `${label} ${path}`);
  }
  for (const [path, count] of Object.entries(item.checks.lengths ?? {})) {
    assert.equal(at(value, path)?.length, count, `${label} ${path} count`);
  }
  for (const [path, count] of Object.entries(item.checks.totalLengths ?? {})) {
    const total = at(value, path).reduce((sum, queue) => sum + queue.length, 0);
    assert.equal(total, count, `${label} ${path} total count`);
  }
  for (const [path, excluded] of Object.entries(item.checks.notIncludes ?? {})) {
    assert.ok(!at(value, path)?.includes(excluded), `${label} ${path} restored claimed proof`);
  }
  const input = JSON.parse(item.input);
  for (const [path, source] of Object.entries(item.checks.same ?? {})) {
    assert.deepEqual(at(value, path), at(input, source), `${label} ${path} retained`);
  }
  for (const [path, source] of Object.entries(item.checks.prefixes ?? {})) {
    const original = at(input, source);
    assert.deepEqual(at(value, path).slice(0, original.length), original, `${label} ${path} prefix`);
  }
}

export function assertCoverage(cases) {
  assert.equal(new Set(cases.map((item) => `${item.operation}/${item.name}`)).size,
    cases.length, "duplicate corpus identity");
  const hits = Object.fromEntries(Object.keys(requiredHits).map((key) => [key, 0]));
  for (const item of cases.filter((item) => item.hit)) {
    assert.ok(Object.hasOwn(hits, item.hit), `unknown branch hit ${item.hit}`);
    assert.ok(item.ok && Object.keys(item.checks ?? {}).length, `missing semantic checks ${item.name}`);
    hits[item.hit] += 1;
  }
  assert.deepEqual(hits, requiredHits, "required semantic branch hit counts");
  return hits;
}
