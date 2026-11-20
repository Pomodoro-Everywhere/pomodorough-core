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
  //
  // v4 supersedes v3 for R55-C05. The v3 file stays immutable. Inputs and
  // count are identical. Exactly 12 error envelopes change: a request root
  // that is not a JSON object now names the request ("completion mutation",
  // "read model", "ownership plan") instead of "invalid shared-core input:
  //  must be a JSON object" with an empty path. Those are the null, array and
  // populated-array roots of workspace.completionMutation.v1 (2),
  // workspace.readModel.v1 (3) and workspace.ownershipPlan.v1 (6) plus the
  // completionMutation structural-root case (1). Every other envelope is
  // byte-identical.
  //
  // v5 supersedes v4 for R55-C07. The v4 file stays immutable. Inputs and
  // count are identical. Exactly 9 error envelopes change: a request root
  // that is not a JSON object now gets the shared "invalid shared-core input:
  // <name> must be a JSON object" message instead of raw Serde text. Those are
  // the malformed-null and malformed-[] roots of workspace.intent.v1
  // ("workspace intent"), sync.batchPlan.v1 ("batch plan"),
  // timer.completionState.v1 ("completion state") and clock.observe.v1
  // ("clock observation"), plus the completionState structural-root case.
  // Every other envelope is byte-identical.
  //
  // v6 supersedes v5 for R55-C11. The v5 file stays immutable. Inputs and
  // count are identical. Exactly 8 error envelopes change: a stored display
  // context with another profile or an extra field now gets a Core message
  // naming its path instead of raw Serde text. Those are the
  // pwa-display-wrong-profile cases ("displayContext.profile must be a JSON
  // string enum") and the pwa-display-eligibility cases ("unknown JSON field
  // displayContext.eligible") of workspace.project.v1, workspace.readModel.v1,
  // workspace.intent.v1 and workspace.completionMutation.v1. Every other
  // envelope is byte-identical.
  assert.equal(JSON.stringify(preservationMetadata(cases, expected)), JSON.stringify(fixture("pwa-selection-preservation-v6")),
    "The complete 2151 prior inputs and envelopes must remain preserved.");
}
