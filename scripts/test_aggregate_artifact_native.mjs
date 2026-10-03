import assert from "node:assert/strict";
import test from "node:test";
import { nativeCorpus, runCorpus } from "../tests/aggregate_artifact/corpus.mjs";
import { assertCoverage, assertSemantics, requiredHits, requiredRejections } from "../tests/aggregate_artifact/semantics.mjs";
import { changed } from "../tests/aggregate_artifact/cases.mjs";

const corpus = nativeCorpus();
const mutations = [
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
