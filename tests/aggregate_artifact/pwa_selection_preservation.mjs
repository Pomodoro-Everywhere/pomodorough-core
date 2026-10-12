import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { fixture } from "./cases.mjs";

const added = new Set(["pwaChoice", "pwaChoiceFlow", "pwaChoiceShape", "pwaCycleRepair", "pwaCycleSource", "pwaDischargeRepair", "pwaDischargeShape"]);
const digest = (rows) => createHash("sha256").update(rows.join("\n") + "\n").digest("hex");

export function preservationMetadata(cases, expected) {
  const old = cases.map((item, index) => ({ item, envelope: expected[index] }))
    .filter(({ item }) => !added.has(item.hit) && !added.has(item.rejectionHit));
  return { count: old.length,
    inputSha256: digest(old.map(({ item }) => JSON.stringify([item.operation, item.name, item.input]))),
    envelopeSha256: digest(old.map(({ envelope }) => envelope)) };
}

export function assertSelectionPreservation(cases, expected) {
  // v2 supersedes v1. The v1 file stays immutable. v2 keeps the identical
  // input digest (no prior input changed) and differs in exactly one
  // envelope: workspace.readModel.v1/fractional-live prints progress
  // 0.12000033333333333 instead of ...332. That is the serde_json
  // float_roundtrip parser correction (R48-C03): the old bytes were an
  // imprecisely parsed quotient, the new bytes are the correctly rounded
  // f64. Every previously rejected input keeps its byte-identical error.
  assert.equal(JSON.stringify(preservationMetadata(cases, expected)), JSON.stringify(fixture("pwa-selection-preservation-v2")),
    "The complete 2141 prior inputs and envelopes must remain preserved.");
}
