import { fixture, vector } from "./cases.mjs";
import { branch } from "./semantics.mjs";
import { naturalRequest, naturalRead, naturalInstall } from "./pwa_natural_cases.mjs";

export function checkerCases() {
  return fixture("pwa-natural-checker-v1").cases.map((item) => {
    const input = JSON.parse(item.inputRaw);
    if (item.name.startsWith("late-ack-")) return branch(item.operation, `checker-${item.name}`, item.inputRaw, "pwaChecker", {
      equals: { selection: input.selection, lifecycle: input.lifecycle } });
    return { ...vector(item.operation, `checker-${item.name}`, item.inputRaw, false, "JSON"), rejectionHit: "pwaChecker" };
  });
}

function insert(root, keys, value) {
  if (!keys.length) return value;
  let current = root;
  for (const [index, key] of keys.slice(0, -1).entries()) {
    const next = keys[index + 1];
    if (current[key] == null || typeof current[key] !== "object") current[key] = next === "0" ? [] : {};
    current = current[key];
  }
  current[keys.at(-1)] = value;
  return root;
}

export function shapeCases() {
  const builders = {
    "workspace.completionMutation.v1": naturalRequest,
    "workspace.readModel.v1": () => naturalRead(naturalRequest()),
    "timer.completionState.v1": () => naturalInstall(naturalRequest()),
  };
  return Object.entries(fixture("pwa-completion-shapes-v1").fields).map(([path, shape]) => {
    const [operation, ...keys] = path.split("/");
    const bad = shape === "array" ? {} : shape.endsWith("string") ? { pwaStorage: null } : [];
    const input = insert(builders[operation](), keys, bad);
    return { ...vector(operation, `structural-${keys.join("-") || "root"}`, input, false, "JSON"), rejectionHit: "pwaStructure" };
  });
}

export function provenanceCases() {
  const seed = JSON.parse(fixture("pwa-natural-checker-v1").cases.find((item) => item.name.startsWith("late-ack-")).inputRaw);
  const cases = [];
  for (const intent of ["start", "resume", "finish", "absent"]) for (const outcome of ["applied", "ignored", "rejected"]) {
    for (const explicit of [false, true]) for (const generation of ["0", "20", "9223372036854775807"]) {
      const input = structuredClone(seed);
      input.selection = { phase: "short_break", generation, explicit };
      input.lifecycle.consumedCompletions = input.lifecycle.consumedCompletions.slice(0, 1);
      input.acknowledgements[0].outcome = outcome;
      if (intent === "absent") delete input.canonicalTimer.lastIntent;
      else input.canonicalTimer.lastIntent = { type: intent, commandId: "other-provenance", occurredAt: input.canonicalTimer.anchorAt };
      if (intent === "finish") input.afterHistory[0].commandId = "other-provenance";
      else delete input.afterHistory[0].commandId;
      cases.push(branch("timer.completionState.v1", `consumed-${intent}-${outcome}-${explicit}-${generation}`, input, "pwaProvenance", {
        equals: { selection: input.selection, lifecycle: input.lifecycle, source: null, reason: "sentFinishesReconciled" } }));
    }
  }
  return cases;
}
