import assert from "node:assert/strict";
import { fixture } from "./cases.mjs";
import { branch } from "./semantics.mjs";
import { batchRequest } from "./planning_cases.mjs";

const domains = fixture("batch-plan-v1").domains;

export function savedCases() {
  const queues = Object.fromEntries(domains.map((domain) => [domain, ["z-last", "a-first", "m-middle"]]));
  const input = { kind: "saved", mode: "sync", limits: { perDomain: 256, total: 512 }, queues };
  const oversized = { ...input, limits: { perDomain: 2, total: 10 } };
  return [branch("sync.batchPlan.v1", "non-sorted-saved-order", input, "savedOrder", {
    equals: { status: "replay_saved", nextDomain: null }, same: { selected: "queues" } }),
  branch("sync.batchPlan.v1", "oversized-saved-atomic", oversized, "oversizedSaved", {
    equals: { status: "oversized_saved", nextDomain: null },
    lengths: Object.fromEntries(domains.map((domain) => [`selected.${domain}`, 0])) })];
}

function drain(call, counts, totals, hit, prefix) {
  let input = batchRequest(counts, "sync");
  if (hit === "cursorRotation") input.limits = { perDomain: 1, total: 1 };
  const original = new Set(domains.flatMap((domain) => input.queues[domain].map(({ id }) => `${domain}/${id}`)));
  const consumed = new Set();
  for (const [round, count] of totals.entries()) {
    const lengths = hit === "cursorRotation" ? { [`selected.${domains[round % 5]}`]: 1 } : {};
    const equals = { status: "planned" };
    if (hit === "cursorRotation") {
      equals.nextDomain = domains[(round + 1) % 5];
      equals[`selected.${domains[round % 5]}`] = [`op-0000${Math.floor(round / 5)}`];
    }
    const output = call(branch("sync.batchPlan.v1", `${prefix}-${round}`, input, hit,
      { lengths, equals, totalLengths: { "selected.*": count } }));
    const selected = domains.flatMap((domain) => output.selected[domain].map((id) => `${domain}/${id}`));
    assert.equal(selected.length, count, `${prefix} round ${round} selected count`);
    for (const id of selected) {
      assert.ok(original.has(id) && !consumed.has(id), `${prefix} duplicate or invented ${id}`);
      consumed.add(id);
    }
    input = structuredClone(input);
    for (const domain of domains) input.queues[domain] = input.queues[domain].filter(({ id }) => !output.selected[domain].includes(id));
    input.nextDomain = output.nextDomain;
  }
  assert.deepEqual(consumed, original, `${prefix} incomplete drain`);
  return input;
}

export function batchScenarios(call) {
  drain(call, [2, 2, 2, 2, 2], Array(10).fill(1), "cursorRotation", "one-slot-cursor");
  const empty = drain(call, [257, 257, 257, 257, 257], [512, 512, 261], "batchDrain", "actual-drain");
  call(branch("sync.batchPlan.v1", "actual-drain-empty", empty, "batchDrain", {
    equals: { total: 0 }, lengths: Object.fromEntries(domains.map((domain) => [`selected.${domain}`, 0])) }));
}
