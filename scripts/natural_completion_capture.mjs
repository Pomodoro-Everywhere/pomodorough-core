import assert from "node:assert/strict";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { invoke } from "../tests/aggregate_artifact/abi_host.mjs";

const temp = process.env.PWA12_EVIDENCE_DIR || "/var/folders/r_/_mr22dqn24d31b7460cz8z5m0000gn/T/opencode";
const destination = new URL("../fixtures/pwa-natural-completion-v1.json", import.meta.url);
assert.equal(process.argv[2], "--capture");
assert.equal(existsSync(destination), false, "raw observations cannot be refreshed");
const evidence = JSON.parse(readFileSync(`${temp}/pwa045-checker-evidence.json`, "utf8"));
const bytes = readFileSync(`${temp}/pwa-core-045/pomodorough_core.wasm`);
const sha256 = createHash("sha256").update(bytes).digest("hex");
assert.equal(sha256, "845090328b2f44056480c3930e9bb684a3874b8f3cbcbd4253ddd92f67c6f5d6");
const { instance } = await WebAssembly.instantiate(bytes);
const receipts = evidence.receipts.filter((row) => row.case.startsWith("independent natural expiry current/frozen"));
assert.equal(receipts.length, 4);
for (const receipt of receipts) {
  const request = receipt.request;
  assert.deepEqual(JSON.parse(request.inputRaw), request.input);
  const actual = JSON.parse(invoke(instance.exports, { operation: request.operation, input: request.inputRaw }));
  assert.deepEqual(actual.value, request.completeCoreReturn);
  assert.deepEqual(actual.value, receipt.completeProductionReturn);
  assert.deepEqual(actual.value, receipt.completeFrozenReturn);
  assert.equal(actual.value.reason, "staleTimer");
}
const http = JSON.parse(readFileSync(`${temp}/pwa045-natural-http.json`, "utf8"));
assert.deepEqual(JSON.parse(http.naturalResponseRaw), http.naturalResponse);
assert.deepEqual(JSON.parse(http.finishResponseRaw), http.finishResponse);
assert.equal(http.finishResponse.acknowledgements[0].outcome, "applied");
const installed = evidence.receipts.find((row) => row.http?.naturalResponseRaw === http.naturalResponseRaw);
assert.ok(installed, "real HTTP production installation trace missing");
writeFileSync(destination, JSON.stringify({ officialVersion: "0.45.0", officialSha256: sha256, receipts,
  http, installation: installed }, null, 2) + "\n");
console.log("Captured four complete current/frozen returns and raw Go HTTP installation evidence.");
