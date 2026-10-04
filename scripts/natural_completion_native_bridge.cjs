"use strict";
const assert = require("node:assert/strict");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const root = process.env.POMODOROUGH_ROOT || path.resolve(__dirname, "../..");
const { SharedCore } = require(path.join(root, "server/web/shared-core.js"));
const original = SharedCore.prototype.call;
const operations = new Set(["workspace.completionMutation.v1", "workspace.readModel.v1", "timer.completionState.v1"]);
SharedCore.prototype.call = function (operation, input) {
  if (!operations.has(operation)) return original.call(this, operation, input);
  const raw = typeof input === "string" ? input : JSON.stringify(input);
  const result = spawnSync(process.env.PWA12_NATIVE_ORACLE || path.join(root, "pomodorough-core/target/debug/examples/artifact_parity_oracle"), [], {
    input: JSON.stringify({ operation, input: raw }) + "\n", encoding: "utf8", timeout: 120000, maxBuffer: 16 * 1024 * 1024 });
  assert.equal(result.status, 0, result.stderr);
  const envelope = JSON.parse(result.stdout);
  if (!envelope.ok) throw new Error(envelope.error);
  return envelope.value;
};
