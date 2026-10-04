import assert from "node:assert/strict";
import test from "node:test";
import { nativeCorpus, runCorpus } from "../tests/aggregate_artifact/corpus.mjs";
import { assertCoverage, assertSemantics, requiredHits, requiredRejections } from "../tests/aggregate_artifact/semantics.mjs";
import { changed } from "../tests/aggregate_artifact/cases.mjs";
import { assertSelectionPreservation } from "../tests/aggregate_artifact/pwa_selection_preservation.mjs";

const corpus = nativeCorpus();
test("immutable pre-extension digest rejects input drift and complete envelope drift", () => {
  assertSelectionPreservation(corpus.cases, corpus.expected);
  const cases = structuredClone(corpus.cases), expected = [...corpus.expected];
  cases[0].input += " ";
  assert.throws(() => assertSelectionPreservation(cases, expected), /2141 prior inputs/);
  cases[0] = corpus.cases[0]; expected[0] += " ";
  assert.throws(() => assertSelectionPreservation(cases, expected), /2141 prior inputs/);
});
const mutations = [
  ["cycle-start-focus", "selection.explicit", true],
  ["cycle-start-short_break", "selection.generation", "9"],
  ["cycle-deadline-focus", "display.phase", "focus"],
  ["cycle-deadline-focus", "availableIntents", ["start", "selectPhase"]],
  ["cycle-finish-focus", "outcome", "noop"],
  ["cycle-finish-focus", "lifecycle.finishEvidence", []],
  ["cycle-current-protected-finish-long_break", "selection.explicit", false],
  ["discharge-install-focus-cleared", "selection.phase", "focus"],
  ["choice-completed-focus", "selection.explicit", false],
  ["choice-completed-focus", "selection.generation", "7"],
  ["choice-completed-focus", "outcome", "noop"],
  ["choice-completed-focus", "allocation.deviceSequence", 9],
  ["choice-completed-focus", "commands", [{ type: "clear" }]],
  ["choice-completed-focus", "workspace.base.canonicalTimer", null],
  ["choice-ack-completed-focus-rejected", "selection.phase", "short_break"],
  ["choice-ack-completed-short_break-rejected-missing", "selection.phase", "focus"],
  ["choice-ack-running-short_break-rejected-replacement", "selection.phase", "focus"],
  ["choice-expired-display-running-focus", "display.phase", "short_break"],
  ["choice-finish-completed-focus", "commands", [{ type: "finish" }, { type: "start" }]],
  ["choice-skip-1", "selection.generation", "8"],
  ["pwa-release-missing", "ownershipWrites", [{ kind: "recordTimerOwner" }]],
  ["pwa-release-own-live", "ownership.leaseExpiresAtMs", 1784548831000],
  ["pwa-release-own-expired", "ownershipWrites", []],
  ["pwa-release-own-missing-expiry", "ownershipWrites", []],
  ["pwa-release-peer-expired", "ownership.tabId", "tab-local"],
  ["pwa-release-foreign-expired", "ownershipWrites", [{ kind: "removeTimerOwner" }]],
  ["pwa-release-stale-owner-timer", "ownership.timerId", "ownership-timer"],
  ["pwa-release-no-timer", "workspace.local.commands", [{ id: "claim" }]],
  ["pwa-release-zero-clock", "ownership.leaseExpiresAtMs", 1],
  ["pwa-release-boundary-true-0", "ownershipWrites", [{ kind: "recordTimerOwner" }]],
  ["pwa-release-takeover-0", "renewed", false],
  ["pwa-release-terminal-finish", "ownershipWrites", [{ kind: "removeTimerOwner" }]],
  ["dependency-residual-body-no-device-head-null", "metadataWrites", [{ kind: "recordTimerDependencies", value: [] }]],
  ["dependency-residual-descendant-phase-finished-true-proof-false", "outcome", "planned"],
  ["dependency-residual-descendant-plannedDurationMs-finished-true-proof-false", "recovery.blocksSync", false],
  ["dependency-residual-descendant-observedElapsedMs-finished-true-proof-false", "recovery.blocksMutations", false],
  ["dependency-checker-retained-source-ack-rejected", "outcome", "planned"],
  ["dependency-checker-saved-body-incomplete-{}", "recovery.blocksSync", false],
  ["dependency-checker-pending-frozen-duration-60000", "metadataWrites", [{ kind: "rewriteCommand" }]],
  ["dependency-raw-missing-parent", "recovery.unresolved", []],
  ["dependency-raw-missing-parent", "recovery.blocksSync", false],
  ["dependency-raw-missing-parent", "workspace.local.commands", []],
  ["dependency-raw-sibling-proven", "workspace.local.commands.0.dependsOnCommandId", "legacy-break-start"],
  ["dependency-raw-sibling-proven", "classifications.0.dependency.dependsOnOperationId", "legacy-finish"],
  ["dependency-raw-canonical-source-applied", "outgoing.body", "reconstructed"],
  ["dependency-raw-canonical-source-applied", "timerDependencies", [{ operationId: "phantom" }]],
  ["dependency-raw-canonical-source-rejected", "outcome", "planned"],
  ["dependency-direct-future", "classifications.0.dependency.dependsOnOperationId", "legacy-finish"],
  ["dependency-rebase-applied", "pendingTimerDependencies", []],
  ["dependency-rebase-rejected", "droppedTimerOperationIds", []],
  ["dependency-physical-source", "classifications.0.sourceCompletedAt", "2026-08-31T12:00:00Z"],
  ["dependency-causal-count", "classifications.0.sourcePhaseAfter", "long_break"],
  ["dependency-canonical-only", "workspace.base.history", [{ id: "phantom" }]],
  ["dependency-frozen-payload", "metadataWrites", [{ kind: "rewriteCommand" }]],
  ["dependency-sibling-saved-ack", "timerDependencies", []],
  ["checker-late-ack-after-provenance-replacement-rejected-explicittrue", "selection.phase", "focus"],
  ["consumed-finish-rejected-false-20", "lifecycle.consumedCompletions", []],
  ["natural-raw-public-0", "commands.0.timerId", "wrong-timer"],
  ["natural-raw-public-0", "projection.history.0.id", "duplicate-completion"],
  ["natural-display-finish", "cadence.completedFocusTotal", 2],
  ["natural-display-finish", "availableIntents", ["start"]],
  ["natural-lease-true-1", "allocation.deviceSequence", 9],
  ["natural-fourth-long", "commands.1.phase", "short_break"],
  ["natural-fourth-counts", "cadence.completedFocusToday", 5],
  ["natural-ack-rejected", "selection.phase", "focus"],
  ["legacy-json-below-number", "operations.durationOperations.0.durationMs", 5460000],
  ["legacy-json-below-number", "operations.durationOperations.1.durationMs", 5460000],
  ["legacy-json-below-number", "operations.durationOperations.2.durationMs", 5460000],
  ["legacy-json-markers-true", "settings.peerOnlySetting.number", 90.5],
  ["legacy-json-markers-false", "settings.peerOnlySetting.array.0", 90.5],
  ["legacy-json-claimed-false", "workspace.local.durationOperations.0.extension.number", 90.5],
  ["legacy-json-claimed-false", "outgoing.sent.durationOperations.0.extension.number", 90.5],
  ["legacy-json-claimed-true", "outgoing.extension.number", 90.5],
  ["legacy-remote-35", "operations.durationOperations.0.hlcWallMs", 1788177600000],
  ["legacy-remote-35", "projection.durationsMs.focus", 1800000],
  ["legacy-remote-35", "operations.autoStartOperations.0.enabled", true],
  ["legacy-remote-35", "operations.selectedTaskOperations.0.taskId", "wrong-task"],
  ["legacy-number-half", "operations.durationOperations", [{ id: "default" }]],
  ["legacy-number-invalid", "operations.durationOperations.0.durationMs", 1500000],
  ["legacy-saved-exact", "outgoing.body", "reconstructed"],
  ["legacy-saved-exact", "workspace.local.durationOperations.0.occurredAt", "1970-01-01T00:00:00.000Z"],
  ["legacy-restart-noop", "writeSettings", true],
  ["queued-upsertTask-claimed", "groupOutcomes.taskOperations.0.outcome", "applied"],
  ["appleWorkspace-generated-short_break", "commands.1.phase", "long_break"],
  ["appleWorkspace-generated-pause", "workspace.timerDependencies", []],
  ["appleWorkspace-generated-child-finish", "commands", []],
  ["appleWorkspace-generated-focus-ack", "promotedTimerOperationIds", []],
  ["appleWorkspace-generated-child-barrier", "selected.commands", []],
  ["appleWorkspace-generated-start-rejection", "droppedTimerOperationIds", []],
  ["desktop-deferred-barrier", "reason", ""],
  ["apple-explicit-start", "selection.phase", "short_break"],
  ["fractional-live", "canonical.elapsedMs", 180000],
  ["pwa-missing-reading", "observation.monotonicAnchor", null],
  ["populated-task-totals", "cadence.completedFocusToday", 0],
  ["non-sorted-saved-order", "selected.commands", ["a-first", "m-middle", "z-last"]],
  ["oversized-saved-atomic", "selected.commands", ["z-last"]],
  ["one-slot-cursor-0", "nextDomain", "commands"],
  ["actual-drain-0", "selected.commands", []],
  ["actual-http-200-finish", "baseTimer", null],
  ["partial-proof-raw-retained", "pendingTaskOperations", []],
  ["terminal-focus-ack", "promotedTimerOperationIds", []],
  ["terminal-start-rejection", "droppedTimerOperationIds", []],
  ["terminal-start-applied", "promotedTimerOperationIds", []],
  ["terminal-raw-pwa-install", "selection.phase", "focus"],
  ["raw-native-metadata", "timer.lastIntent.deviceId", null],
  ["terminal-generated-normalization", "pending.0.extension", null],
  ["missing-history-frozen-extensions", "pending", []],
  ["missing-history-ack-ignored-finish-fields", "workspace.history.0.phase", "long_break"],
  ["pwa-raw-project-0", "projectionPending.commands", ["forged-proof"]],
  ["pwa-raw-read-0", "canonical.status", "idle"],
  ["pwa-null-start-false", "commandOutcomes.0.outcome", "queued"],
  ["pwa-covered-pause-false", "projection.canonicalTimer.status", "running"],
  ["pwa-covered-resume-false", "projection.canonicalTimer.status", "paused"],
  ["pwa-covered-finish-true", "commands.1.type", "finish"],
  ["pwa-display-focus-ack", "displayContext.projectionPending.commands", []],
  ["pwa-actual-http-ack-trim", "displayContext.projectionPending.commands", ["stale"]],
  ["pwa-start-removed-no-synthetic-timer", "timer", { id: "synthetic" }],
  ["pwa-display-foreign-owner", "outcome", "planned"],
  ["pwa-display-normalized-context", "displayContext.projectionPending.commands.0.phase", "long_break"],
  ["pwa-duration-context-retired", "workspace.displayContext.projectionPending.durationOperations", []],
  ["pwa-duration-context-claimed", "groupOutcomes.durationOperations.0.outcome", "queued"],
  ["pwa-duration-hidden-claimed", "groupOutcomes.durationOperations.0.outcome", "applied"],
  ["pwa-admit-duration-short_break", "projection.durationsMs.short_break", 300000],
  ["pwa-admit-finish-long_break", "commands.1.plannedDurationMs", 900000],
  ["pwa-admit-peer-no-start", "commands.0.type", "start"],
  ["pwa-admit-task-delete", "projection.canonicalTimer.taskId", "stale-task"],
  ["pwa-admit-new-barrier-durationOperations-claim", "displayContext.projectionPending.durationOperations", []],
  ["pwa-owner-missing-retained", "ownershipWrites", []],
  ["pwa-owner-foreign-expired", "renewed", true],
  ["pwa-owner-install-valid", "ownership", null],
  ["pwa-owner-removed", "ownershipWrites", []],
  ["pwa-owner-boundary--1", "renewed", true],
  ["pwa-owner-boundary--1", "retryAtMs", null],
  ["pwa-owner-boundary-0", "renewed", false],
  ["pwa-owner-boundary-1", "ownership.tabId", "tab-local"],
  ["pwa-owner-origin-device-foreign", "ownershipWrites", [{ kind: "recordTimerOwner" }]],
  ["pwa-owner-terminal-finish", "ownership", { timerId: "ownership-timer" }],
];

test("native authority reaches every required semantic branch", () => {
  assert.deepEqual(corpus.hits, requiredHits);
  console.log(`Native semantic hits: ${JSON.stringify(corpus.hits)}`);
});

test("dropping any required branch vector fails coverage even with correct envelopes", () => {
  for (const hit of Object.keys(requiredHits)) {
    const index = corpus.cases.findIndex((item) => item.hit === hit);
    assert.throws(() => assertCoverage(corpus.cases.filter((_, i) => i !== index)), /semantic branch hit counts/);
  }
});

test("dropping any required raw rejection vector fails strict coverage", () => {
  for (const hit of Object.keys(requiredRejections)) {
    const index = corpus.cases.findIndex((item) => item.rejectionHit === hit);
    assert.ok(index >= 0);
    assert.throws(() => assertCoverage(corpus.cases.filter((_, i) => i !== index)), /required raw rejection hit counts/);
  }
});

for (const [name, path, replacement] of mutations) {
  test(`native semantic assertion kills ${name} mutant`, () => {
    const index = corpus.cases.findIndex((item) => item.name === name);
    assert.ok(index >= 0, `required mutant vector missing: ${name}`);
    const envelope = JSON.parse(corpus.expected[index]);
    assertSemantics(corpus.cases[index], envelope.value);
    const mutant = changed(envelope.value, path, replacement);
    assert.throws(() => assertSemantics(corpus.cases[index], mutant), /semantic/);
  });
}

test("stateful replay consumes actual previous returns and drains all 1285 IDs", () => {
  let cursor = 0;
  const replayed = runCorpus(corpus.staticCases, (item) => {
    assert.deepEqual(item, corpus.cases[cursor], `stateful input ${cursor}`);
    return corpus.expected[cursor++];
  });
  assert.equal(cursor, corpus.cases.length);
  assert.deepEqual(replayed.cases, corpus.cases);
});
