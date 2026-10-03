import { readFileSync } from "node:fs";

export const operations = [
  "workspace.project.v1", "workspace.readModel.v1", "workspace.intent.v1",
  "workspace.completionMutation.v1", "bootstrap.workspacePlan.v1",
  "sync.batchPlan.v1", "timer.completionState.v1", "clock.observe.v1",
  "reconcile.rebase.v3",
  "workspace.ownershipPlan.v1",
];

export function fixture(name) {
  return JSON.parse(readFileSync(new URL(`../../fixtures/${name}.json`, import.meta.url), "utf8"));
}

export function vector(operation, name, input, ok = true, error = null) {
  return { operation, name, input: typeof input === "string" ? input : JSON.stringify(input), ok, error };
}

export function changed(input, path, value) {
  const result = structuredClone(input);
  const keys = path.split(".");
  const parent = keys.slice(0, -1).reduce((object, key) => object[key], result);
  parent[keys.at(-1)] = value;
  return result;
}

export function patchInput(target, patch) {
  for (const [key, value] of Object.entries(patch)) {
    if (value !== null && typeof value === "object" && !Array.isArray(value)) {
      target[key] ??= {};
      patchInput(target[key], value);
    } else {
      target[key] = structuredClone(value);
    }
  }
  return target;
}

export function invalidCases(seed, overflowPath) {
  const { operation, input } = seed;
  const parsed = JSON.parse(input);
  const nested = input.indexOf(":{") + 2;
  if (nested < 2) throw new Error(`missing nested object in ${operation}`);
  const duplicate = '"artifactDuplicate":null,"artifactDuplicate":false,';
  return [
    ...["{", "null", "[]"].map((raw) => vector(operation, `malformed-${raw}`, raw, false)),
    vector(operation, "duplicate-root", `{${duplicate}${input.slice(1)}`, false, "duplicate field"),
    vector(operation, "duplicate-nested", input.slice(0, nested) + duplicate + input.slice(nested), false, "duplicate field"),
    vector(operation, "unknown-control", { ...parsed, artifactUnknown: null }, false),
    vector(operation, "unsafe-integer", changed(parsed, overflowPath, 9007199254740992), false),
    vector(operation, "integer-overflow", JSON.stringify(changed(parsed, overflowPath, "RAW_OVERFLOW"))
      .replace('"RAW_OVERFLOW"', "18446744073709551616"), false),
  ];
}
