"use strict";

// Execute unchanged PWA storage against IndexedDB and the existing bundled Core.
const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");
const web = path.resolve(__dirname, "../../server/web");
const { indexedDB } = require(path.resolve(web, "../node_modules/fake-indexeddb"));
const storage = require(path.join(web, "sync-storage.js"));
const { SharedCore } = require(path.join(web, "shared-core.js"));
globalThis.crypto ||= crypto.webcrypto;

async function database(name) {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(name, 1);
    request.onupgradeneeded = () => {
      for (const store of ["meta", "pending", "pendingTasks", "pendingDurations", "pendingAutoStarts", "pendingSelectedTasks"]) {
        request.result.createObjectStore(store, { keyPath: store === "meta" ? "key" : "id" });
      }
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

async function runCase(input, name, core) {
  const connection = await database(`completion-probe-${name}`);
  try {
    const transaction = connection.transaction(["meta", "pending"], "readwrite");
    const meta = transaction.objectStore("meta");
    const base = input.workspace.base;
    meta.put({ key: "snapshot", value: { ...base, user: { id: "user-1" }, revision: 0,
      serverTime: "2026-07-20T12:00:00Z" } });
    meta.put({ key: "deviceSequence", value: input.allocation.deviceSequence });
    meta.put({ key: "hlc", value: input.allocation.hlc });
    meta.put({ key: "settings", value: { selectedPhase: input.selection.phase } });
    if (input.ownership) meta.put({ key: "timerOwner", value: input.ownership });
    for (const command of input.workspace.local.commands) transaction.objectStore("pending").put(command);
    await storage.transactionDone(transaction);
    const automatic = input.stage === "automaticFinishCommit";
    const outcome = await storage.finishTimer(connection, {
      expectedUserId: "user-1", timerId: input.requestedTimer.id,
      phase: input.requestedTimer.phase, requestedTimer: input.requestedTimer,
      deviceId: input.allocation.deviceId, tabId: input.localTabId || "tab-local",
      nowMs: Date.parse(input.clock.occurredAt), localNowMs: input.leaseNowMs,
       leaseMs: input.leaseDurationMs ?? 30_000, manual: !automatic, requireOwner: automatic,
       observedElapsedMs: 60_000, withUuidV7: true,
       breakTimerId: input.identities.timerUuid,
      entropy: (bytes) => { bytes.fill(0); bytes[bytes.length - 1] = 1; return bytes; },
      settings: { selectedPhase: input.selection.phase }, sharedCore: core
    });
    const after = connection.transaction(["meta", "pending"], "readonly");
    const [owner, commands, sequence, hlc, uuidV7] = await Promise.all([
      storage.requestResult(after.objectStore("meta").get("timerOwner")),
      storage.requestResult(after.objectStore("pending").getAll()),
      storage.requestResult(after.objectStore("meta").get("deviceSequence")),
      storage.requestResult(after.objectStore("meta").get("hlc")),
      storage.requestResult(after.objectStore("meta").get("uuidV7"))
    ]);
    return { transitioned: outcome.transitioned, reason: outcome.reason,
      ...(Object.hasOwn(outcome, "retryAtMs") ? { retryAtMs: outcome.retryAtMs } : {}),
      commands: outcome.commands, selectedPhase: outcome.selectedPhase ?? null,
       finishCount: commands.filter((command) => command.type === "finish").length,
       startCount: commands.filter((command) => command.type === "start").length,
      ownerAfter: owner?.value ?? null, sequence: sequence?.value,
      hlc: hlc?.value, uuidV7: uuidV7?.value ?? null };
  } finally {
    connection.close();
  }
}

async function main() {
  const inputs = JSON.parse(fs.readFileSync(0, "utf8"));
  const core = await SharedCore.fromBytes(fs.readFileSync(path.join(web, "pomodorough_core.wasm")));
  storage.setSharedCore(core);
  const results = [];
  for (const { name, input } of inputs) results.push({ name, ...await runCase(input, name, core) });
  process.stdout.write(JSON.stringify(results));
}

main().catch((error) => { console.error(error); process.exitCode = 1; });
