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
  //
  // v3 supersedes v2 for the elapsed-time history contract. The v2 file stays
  // immutable. Terminal history rows now record `elapsedMs`, a Finish keeps the
  // real elapsed time on its completed timer, and the read model names its
  // totals `completedFocusTodayElapsedMs` and per-task `elapsedMs` under
  // `schemaVersion` 2. Of the 2141 v2 cases, 2139 keep their identity: 191
  // envelopes change only in those fields, 27 of them read models that also
  // carry the new schema version, and 26 chained inputs change because they
  // carry a prior return.
  // Two terminal rejections are replaced by three (a Finish may complete below
  // its plan, a deadline completion may not, and a recorded elapsed must match
  // the timer), three early-finish terminal cases are added, and six structural
  // rejections cover the new `elapsedMs` scalar. No error envelope of a
  // retained case changes.
  assert.equal(JSON.stringify(preservationMetadata(cases, expected)), JSON.stringify(fixture("pwa-selection-preservation-v3")),
    "The complete 2151 prior inputs and envelopes must remain preserved.");
}
