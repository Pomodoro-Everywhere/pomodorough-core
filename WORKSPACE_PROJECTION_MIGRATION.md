# Migrate adapters to safe workspace projection

This guide applies after the official Core artifact exposes
`workspace.project.v1`. The request and result schemas are defined in
[Safe workspace projection v1](WORKSPACE_PROJECTION.md). The current change adds
the Core capability and shared fixtures only. The main backlog owner coordinates
the client changes and their independent verification.

## Persist canonical inputs separately

1. Add durable canonical fields for all six `base` members. Keep canonical
   `durationsMs`, `autoStartBreaks`, and `selectedTaskId` separate from rendered
   settings and selection. Keep canonical timer and history separate from local
   display projection.
2. Persist the canonical covering head with that base. Keep the existing account
   or room ownership and revision alongside the snapshot. Install or replace
   the base and head in the same transaction.
3. Keep all five complete retained queues, exact raw retry payloads, never-sent
   proof, and timer dependencies in the same workspace transaction boundary.
   Persist new payloads and their proof together. Retire proof before delivery.
4. On reconciliation, atomically install `baseTimer`, `baseHistory`, `baseTasks`,
   `baseDurationsMs`, `baseAutoStartBreaks`, and `baseSelectedTaskId`. Map
   `baseTimer` to the workspace request's `base.canonicalTimer`. Install the
   response's `serverHlcWallMs` and `serverHlcCounter` as `canonicalHead`.
5. In that transaction, install all five returned pending queues and
   `pendingTimerDependencies`, and prune proof to the retained identities. Keep
   raw retry records intact. Recompute projection after the transaction.

For an existing installation without a trustworthy head, persist explicit
`canonicalHead: null`. Core then displays only the canonical base, with time
progression from `now`. Do not claim a zero head to recover optimistic visibility.
For a genuinely empty authoritative base, a trustworthy zero covering head is
valid. Establish that fact through the existing bootstrap flow.

If the installation has only optimistic durations or an ambiguous merged
timer/history snapshot, do not copy that state into new canonical columns. Fetch
or reconstruct canonical state through an authoritative existing flow first.
Core cannot recover an overwritten 25-minute duration from a valid 30-minute
base. Keep queued work and saved claims while recovery runs. Missing canonical
inputs are a migration blocker, not permission to discard queues or weaken
validation.

## Replace projection calls

1. Read the canonical base, covering head, complete retained queues, proof, and
   dependencies from one consistent workspace snapshot.
2. Supply the replay time as `now`. Keep canonical timestamps and immutable
   operation timestamps in their shared wire time coordinate system. Convert
   timestamps for native rendering outside the operation payloads.
3. Call `workspace.project.v1` after local mutations, proof retirement, canonical
   installation, restoration after failure, and process restart. Also call it
   when time advancement requires a new timer projection.
4. Install `workspace` as the combined display result. Use its timer, history,
   tasks, durations, auto-start setting, selection, outcomes, and winner IDs.
    Remove local eligibility selection and terminal timer reconstruction.
   The new workspace decoder must allow optional `lastIntent.deviceId`. Older
   projection and reconciliation decoders keep their existing field contracts.
5. Retain complete queues for delivery. Do not replace them with
   `projectionPending`. Treat `projectionPending` as a derived result that becomes
   stale whenever any request input changes.
6. On an invalid-input result, retain durable state and report recovery through
   the platform adapter. Do not retry through `projection.apply.v2` or infer a
   safer-looking queue locally. Do not weaken C01 raw-payload preservation, C02
   identity collision checks, or C03 batch selection.

`workspace` must never overwrite the authoritative `base` after optimistic replay.
The workspace boundary accepts a persisted terminal timer alongside its exact
history row. Pass both raw objects. Remove the adapter's timer-clearing step for
this call and render Core's returned timer directly. Public `timer.reduce.v1`,
`projection.apply.v2`, and reconciliation still have their existing strict input
contracts. Their adapters require separate migration.

A history-only base cannot distinguish completion display from an explicit clear.
Preserve or recover the authoritative terminal timer or explicit null. Do not add
a latest-history heuristic. [Workspace terminal state](WORKSPACE_TERMINAL.md)
defines accepted pairs, rejected conflicts, and the source-parity evidence.

The same raw workspace is accepted by the Core read model, intent planner, and
staged completion lifecycle. Native alarms and adapter adoption remain separate
backlog items.
Pass saved `canonicalAnchorAt` and complete `commandTimes` to the planners through
their existing observation fields. Do not replace wire timestamps before the call.
Persist returned `workspace` and `observation` separately. The planners' physical
display `projection` is not a canonical wire snapshot to install as `base`.

## Adapter-specific changes

- Apple: replace the `safeProjection*` methods in
  `Sources/ImmutableReconciliationPolicy.swift` and the filtered call in
  `TimerSessionController.project`. Render the returned terminal timer rather
  than `restoreTerminalCanonicalTimer`. Audit `projectionBase` because it reads
  `state.settings.durationsMs`, and verify that stored preferences are canonical.
  Remove domain-dependent replay-time selection from the adapter only through
  its separately verified clock/time migration. Do not modify retained command
  timestamps through `localProjection` before sending them to the new interface.
- Desktop: replace `storage.py::_safe_projection_state` and
  `_safe_domain_queue`. Replace the optimistic settings input used by
  `_projection_input` with persisted canonical durations. R43-D04 remains open
   until claim/reopen and unrelated-mutation tests use those canonical columns.
  For workspace calls, remove the overlap clearing in `_projection_input` and
  the retained-timer fallback in `ui_controller.py::presented_timer`.
- Web: replace `app-state.js::projectionQueuesForDisplay`,
  `hasImmutableProof`, and `isNewerThanHead` with a complete request. Do not merge
  fresh commands into cached `projectionPending`, and do not omit fresh task or
  preference operations. Keep the existing `base*` fields and head atomic.
- Android: replace the all-retained replay in
  `TimerRepository.projectSynchronizedState`. Persist the covering head and
  canonical preferences independently of `installCoreProjection`, which updates
  rendered settings. `currentProjectionBase` currently reads those settings and
  local selection. Preserve the distinction between complete retry queues and
  Core's display queues from `CentralizedSyncCoordinator.reconcile`. R43-A03
   remains open until Room reopen and unrelated mutations call the new operation.
  Workspace input must bypass `CoreProjectionDispatcher.projectionInput`'s
  `takeUnless` overlap clearing.

## Verify adoption

1. Publish and verify the official Core artifact before changing consumers.
   Follow each consumer's artifact policy. Apple continues to fetch the latest
   official Core at build time and must not gain a hardcoded Core pin.
2. Run `fixtures/workspace-projection-v1.json` through each actual packaged-Core
   adapter. Also run `fixtures/workspace-terminal-v1.json`. Compare complete
   workspace results, raw safe queue objects, and exact conflict errors.
3. Add storage tests that persist, close, reopen, and rebuild after a delivery
   claim. Include all five domains, partial proof, stale siblings, newer local
   mutations, and a server head covering acknowledged operations.
4. Verify canonical 25 minutes remains 25 after an uncertain 30-minute mutation,
   a later unrelated mutation, and restart. Also verify that a fresh, proven,
   above-head duration displays 30 before its proof is retired.
5. Verify finish, cancel, clear, automatic completion, task deletion with
   selection cleanup, and generated-break dependencies through real application
   entrypoints. Compare equivalent-time results with `reconcile.rebase.v2`.
6. Verify failed projection does not commit partial display or durable changes.
   Keep C01, C02, and C03 regression suites enabled.

Client database migrations, packaged-WASM tests, official publication, and any
missing canonical or terminal source state remain blockers for integration signoff.
The Core implementation does not update the main backlog or declare those gates
complete.
