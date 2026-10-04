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
  assert.equal(JSON.stringify(preservationMetadata(cases, expected)), JSON.stringify(fixture("pwa-selection-preservation-v1")),
    "The complete 2141 prior inputs and envelopes must remain preserved.");
}
