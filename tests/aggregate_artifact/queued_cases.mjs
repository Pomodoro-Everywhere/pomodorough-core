import { fixture } from "./cases.mjs";
import { branch } from "./semantics.mjs";

const task = fixture("workspace-intent-desktop-known-tasks-v1").knownTasks[0];
const domains = fixture("batch-plan-v1").domains;
const intents = [
  [{ kind: "upsertTask", title: "Artifact task" }, [0, 1, 0, 0, 0]],
  [{ kind: "selectTask", taskId: task.id }, [1, 0, 0, 0, 1]],
  [{ kind: "deleteTask", taskId: task.id }, [1, 1, 0, 0, 1]],
  [{ kind: "setDuration", phase: "focus", minutes: 3 }, [0, 0, 1, 0, 0]],
  [{ kind: "setAutoStart", enabled: true }, [0, 0, 0, 1, 0]],
  [{ kind: "addAndSelectTask", title: "Artifact task" }, [1, 1, 0, 0, 1]],
];

function request(intent, nullHead) {
  const input = fixture("workspace-intent-v1").request;
  input.compatibility = "desktopStorage";
  input.intent = intent;
  input.ownership = { ownerId: null, expectedOwnerId: null };
  input.durability = { outgoingDurationOperationIds: [] };
  input.identities.commandUuids = [1, 2, 3].map((n) => `019f7f65-dd10-7000-8000-${String(n).padStart(12, "0")}`);
  input.workspace.base.tasks = [task];
  input.workspace.base.selectedTaskId = intent.kind === "deleteTask" ? task.id : null;
  input.workspace.base.canonicalTimer = fixture("workspace-intent-v1").timer;
  if (nullHead) input.workspace.canonicalHead = null;
  else installClaims(input);
  return input;
}

function installClaims(input) {
  const queues = fixture("workspace-projection-v1").request.local;
  for (const domain of domains) {
    const operation = structuredClone(queues[domain][0]);
    Object.assign(operation, { id: `claimed-${domain}`, deviceId: "device-local",
      hlcCounter: 0, hlcWallMs: 1784548800000, extension: { keep: null } });
    if (domain === "commands") Object.assign(operation, { deviceSequence: 7,
      timerId: "existing-timer", type: "retarget", taskId: null });
    if (domain === "taskOperations") Object.assign(operation, { type: "upsert", taskId: task.id, title: task.title });
    if (domain === "autoStartOperations") operation.enabled = false;
    input.workspace.local[domain] = [operation];
  }
  input.durability.outgoingDurationOperationIds = ["claimed-durationOperations"];
}

export function queuedCases() {
  return intents.flatMap(([intent, counts]) => [false, true].map((nullHead) => {
    const input = request(intent, nullHead);
    const lengths = Object.fromEntries(domains.map((domain, i) => [`operations.${domain}`, counts[i]]));
    const equals = { outcome: "planned" };
    for (const [i, domain] of domains.entries()) {
      equals[`groupOutcomes.${domain}.*.outcome`] = Array(counts[i]).fill("queued");
    }
    if (counts[0]) equals["commands.*.type"] = ["retarget"];
    const prefixes = Object.fromEntries(domains.map((domain) =>
      [`workspace.local.${domain}`, `workspace.local.${domain}`]));
    const notIncludes = Object.fromEntries(domains.map((domain) =>
      [`workspace.neverSent.${domain}`, `claimed-${domain}`]));
    return branch("workspace.intent.v1", `queued-${intent.kind}-${nullHead ? "null-head" : "claimed"}`,
      input, "queued", { lengths, equals, prefixes, notIncludes, same: { "workspace.base": "workspace.base" } });
  }));
}
