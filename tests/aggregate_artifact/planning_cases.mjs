import { fixture, vector } from "./cases.mjs";

function descriptor(index, domain) {
  return { id: `op-${String(index).padStart(5, "0")}`, deviceId: "device-a",
    hlcWallMs: index + 1, hlcCounter: 0, ...(domain === 0 ? { deviceSequence: index + 1 } : {}) };
}

export function batchRequest(counts, mode) {
  return { kind: "new", mode, nextDomain: "commands",
    limits: mode === "sync" ? { perDomain: 256, total: 512 } : { perDomain: 4096, total: 8192 },
    queues: Object.fromEntries(fixture("batch-plan-v1").domains.map((domain, index) =>
      [domain, Array.from({ length: counts[index] }, (_, i) => descriptor(i, index)).reverse()])),
    timerDependencies: [] };
}

export function batchCases() {
  const source = fixture("batch-plan-v1");
  const operation = "sync.batchPlan.v1";
  const cases = source.boundaries.map((item) => vector(operation, item.name, batchRequest(item.counts, item.mode)));
  for (const item of source.barriers) {
    const input = batchRequest([item.count ?? item.operations.length, 1, 1, 1, 1], "sync");
    input.timerDependencies = [{ operationId: descriptor(item.child, 0).id,
      dependsOnOperationId: descriptor(item.parent, 0).id }];
    cases.push(vector(operation, item.name, input));
  }
  for (const mask of source.mixes.masks) {
    const counts = source.domains.map((_, index) => mask & (1 << index) ? source.mixes.countPerPresentDomain : 0);
    cases.push(vector(operation, `domain-mix-${mask}`, batchRequest(counts, "sync")));
  }
  const retry = { kind: "saved", mode: "sync", limits: { perDomain: 256, total: 512 },
    queues: Object.fromEntries(source.domains.map((name) => [name, ["saved-id"]])) };
  cases.push(vector(operation, "exact-retry", retry));
  return cases;
}
