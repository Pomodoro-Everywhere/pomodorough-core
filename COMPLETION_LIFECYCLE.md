# Staged completion lifecycle v1

`workspace.completionMutation.v1` now owns the remaining bounded completion decisions across the original client transaction boundaries. [Completion mutation v1](COMPLETION_MUTATION.md) describes the existing centralized batches. This reference describes the added lifecycle stages and expanded finish profiles.

## Implemented scope

| Profile and mode | Stage | Commands in this commit |
| --- | --- | --- |
| Apple, Android, PWA centralized | `finishCommit`, `automaticFinishCommit` | Existing Finish and optional generated Start batch |
| Apple Iroh manual | `finishCommit` | Finish only, with a retained break opportunity |
| Apple Iroh manual continuation | `deferredBreakOpportunity` | Start only after the Finish commit succeeds |
| Apple, Android, Desktop Iroh expiry | `expiryObservation` | Zero commands or one Start, never a synthetic Finish |
| Desktop centralized manual or automatic | `finishCommit`, `automaticFinishCommit` | Finish only, with a SQLite-compatible pending trigger |
| Desktop centralized opportunity | `deferredBreakOpportunity` | Zero commands or one Start, with canonical or optimistic admission |

Desktop accepts a presented natural-completion snapshot only when it exactly matches Core's fresh expiry projection. PWA Iroh, Android deferred Finish, and Apple centralized deferred Finish are unsupported. Their centralized batches keep their original boundaries.

## Lifecycle request schema

Both added stages require the following fields:

[`fixtures/completion-lifecycle-request-v1.json`](fixtures/completion-lifecycle-request-v1.json) is a complete executable expiry request with raw old and new workspace state. Its expected command and consumption identities are asserted in the native suite.

```text
stage: "expiryObservation" | "deferredBreakOpportunity"
compatibility: "appleWorkspace" | "androidCoordinator" | "desktopStorage" | "desktopTerminal"
replicationMode: "centralized" | "iroh"
workspace: raw workspace.project.v1 aggregate without now
selection: {phase, generation: decimal string, explicit: boolean}
allocation: {deviceId, deviceSequence, hlc: {wallMs, counter}, lastUuid: string | null}
observation: {canonicalAnchorAt: string | null, commandTimes: {commandId: timestamp}}
clock: {occurredAt, physicalNow, observedAt}
identities: {commandUuids: UUIDv7[], timerUuid: UUIDv4 | null}
calendarIntervals: [{start, end}]
ownership: null | {timerId, deviceId}
lifecycle: {
    consumedCompletions: [{timerId, commandId: string | null, phase}],
    pendingBreaks: [{finishCommandId, timerId, finishDeviceSequence, reservedTimerUuid?}]
}
centralizedSession: {userId: string | null, authenticated: boolean}
event: one of the event objects below
```

The two lifecycle arrays default to empty inside the required `lifecycle` object. `reservedTimerUuid` is a UUIDv4 used only for Apple Iroh manual continuation. `centralizedSession.userId` is required and explicitly nullable. Omission fails before planning, including when `authenticated` is false. Unknown fields and duplicate JSON fields fail validation. Caller-generated projections, `sourceAccepted`, `generateAutoBreak`, and ownership decisions are not accepted. PWA monotonic metadata and leases are unsupported in these stages.

The workspace contains the canonical base, all five retained queues, `canonicalHead`, `neverSent`, and `timerDependencies`. Core validates every domain before projection. Centralized requests retain the safe projection rule. Iroh requests replay all validated retained domains, even when centralized delivery proof would suppress them. The returned canonical base remains raw.

An installed terminal timer can also appear in its exact history row. The shared workspace boundary validates identity, task, phase, duration, terminal time, and command provenance. Core seeds one session while retaining the history identity, terminal command, elapsed value, starter, and lifecycle intent. The lifecycle no longer clears and restores a duplicate in a separate implementation. [Workspace terminal state](WORKSPACE_TERMINAL.md) defines the same boundary used by workspace projection, read models, and intents. Public reducers and reconciliation retain their prior contracts.

The event objects are:

```json
{"kind": "observation"}
{"kind": "opportunity"}
{"kind": "canonicalInstalled", "acknowledgements": [{"commandId": "finish-id", "outcome": "applied"}], "discardedCommandIds": []}
```

Acknowledgement outcomes are `applied`, `ignored`, or `rejected`. Both event lists reject empty and duplicate identities. `observation` and `opportunity` have strict empty payloads. They reject every field except `kind`, including ACK, discard, and caller-policy metadata. `canonicalInstalled` is the only event that accepts ACK and discard evidence for the already-installed workspace.

## Expiry observation

`expiryObservation` requires raw `previousWorkspace`, `previousObservation`, and `event: {kind: "observation"}`. Optional `requestedTimer` represents the timer presented by the alarm or UI. Its complete intent and timer fingerprint must match the old physical projection.

Core projects the old workspace without expiry, calculates its expected expiry at `observedAt`, and compares that result with the new projection. The same running timer must become completed. Pause, replacement, changed anchor, changed intent, early observation, and prior Finish prevent consumption. The exact deadline is eligible.

A completed identity is `(timerId, commandId, phase)`. Natural expiry has a null command ID. Prior completed history and persisted `consumedCompletions` prevent repeat consumption after refresh or restart. Consumption can return `planned` without a command. Sequence, HLC, and UUID cursor remain unchanged in that case.

Core returns `nextPhase` for every consumed completion. Apple and Desktop advance selection when its phase matches the completed phase. Apple preserves explicit selection. An Apple lifecycle Start preserves the phase, generation, and explicit flag produced by completion or supplied by the deferred snapshot. The break command's phase does not replace that selection. Android's expiry orchestrator ignores the legacy selection decision unless it creates Start, so Core preserves Android selection without Start. Android advances generation when Start changes phase. Ownership controls Start, not completion consumption. Apple uses a matching local owner record, with `startedByDeviceId` fallback when the record is absent. Android requires its matching owned-timer record. Desktop Iroh uses the timer's `startedByDeviceId`.

An owned focus expiry with effective auto-start enabled produces one Start. Effective history determines cadence, and pending duration and preference operations remain authoritative. The Start has no synthetic Finish, source dependency, or provisional record. Apple returns `noop` with `startBeforeCompletion` when trusted occurrence precedes physical completion, matching its existing nil preparation. Android and Desktop retain their existing physical-observation replay without this Apple admission rule.

## Deferred break opportunity

Desktop's stage accepts no `previousWorkspace`, `previousObservation`, or `requestedTimer`. The three required trigger fields map to `pending_auto_breaks`. Array order represents SQLite row order.

Core retires a trigger after an explicit discard, rejected ACK, or applied or ignored ACK without exact canonical focus completion. Unrelated ACKs retain it. A later unrelated local timer command also retires it. A later same-timer Finish does not retire it, although changed source provenance can still block generation. Foreign-device sequences do not represent later local commands.

An authenticated centralized session with a user ID imposes the canonical barrier. The source Finish must leave the retained queue, and pending auto-start operations must clear. Core derives acceptance from the raw canonical timer and its exact history row. The current timer must still be the completed source focus. Both command and timer identities must match.

Accepted sources use canonical history for cadence even during optimistic opportunities. Pending sources use the fresh safe projection when no canonical barrier applies. A blocked head stops processing. A retired head permits the next trigger. Each call materializes at most one Start.

A pending optimistic source produces a generated dependency with `sourceDayStart` and `sourceDayEnd`, plus a provisional break record. Accepted sources produce neither. Existing `reconcile.rebase.v2` policy corrects an unsent short break after the fourth canonical focus while preserving command and timer IDs. Rejection drops unsent generated work. Frozen retained commands remain unchanged.

Apple Iroh continuation requires `previousWorkspace` and `previousObservation` from the committed Finish result, plus the current raw workspace. The trigger retains unused entropy as `reservedTimerUuid`. Changed entropy fails validation. Core derives phase and duration from the committed Finish snapshot, so later history or preference changes cannot alter the prepared break. The second commit contains only Start and has no centralized dependency.

## Result and transaction boundaries

All results retain the existing completion mutation fields and add `lifecycle`. New stages also return `source`, `sourceStatus`, `nextPhase`, and `retiredTriggerIds`. `source` is the exact completed history row. `sourceStatus` is null, `naturalExpiry`, `pending`, or `accepted`. `nextPhase` is null until Core admits completion or Start. Retired trigger IDs are source Finish IDs.

`planned` means completion consumption, trigger retirement, or a new command. `noop` preserves lifecycle state, queues, allocation, and observation. Reasons include `notExpired`, `staleTimer`, `alreadyConsumed`, `startBeforeCompletion`, `noPendingBreak`, `canonicalBarrier`, `waitingForSource`, and `triggerDropped`. Trigger retirement is a planned lifecycle write with zero commands. Errors return no partial plan.

Start uses command candidate zero and advances sequence, HLC, and UUID cursor once. Unused candidates stay unused. New supported profiles omit `deviceId` in `durableCommands` but retain it in Core queues. Desktop occurrence strings have millisecond RFC 3339 precision. Physical command observations remain separate from wire occurrence times.

Finish-only stages emit `completionRecords.pendingAutoBreak`. Desktop's first commit also emits `pendingPhaseAdvance`. A provisional Start record contains source and generated command and timer IDs, plus selection version. The existing completion-state operation still owns phase-advance reconciliation and rollback.

The caller persists the result under its existing account gate and transaction lock. A Start returns `launchSync` and `scheduleAlarm` effects for execution after successful commit. Apple Iroh manual continuation also returns source `cancelAlarm` before `scheduleAlarm`. Its first Finish commit emits no provisional phase record and does not cancel the source alarm while the Start commit can still fail. Completion presentation, OS alarm admission, account gates, transport capture, and UI scheduling remain client responsibilities. Apple never calls its second stage when the first Finish commit fails.

## Evidence and migration blockers

`tests/completion_lifecycle.rs` covers Iroh profiles, stale and terminal snapshots, owner denial, explicit selection, deadline plus and minus one millisecond, repeat, restart, queue retirement, canonical barriers, ACK rejection and discard, exact provenance, frozen queues, reserved UUIDs, and optimistic fourth-focus reconciliation. Existing centralized completion tests remain active.

`scripts/completion_lifecycle_source_probe.py` compiles unchanged Swift expiry policy, command creation, phase advancement, and Apple's two-commit orchestrator, including failed Finish and Start persistence. Complete returned selection objects are compared without input substitution. Swift state serialization and restoration also cover restart selection. It compiles Android expiry orchestration. It executes Desktop expiry policy and deferred queue methods against an in-memory SQLite transaction. Legacy policy calls use native `timer.completionPlan.v1`, independently of the new stage. Deterministic adapters supply clocks, entropy, projection, and persistence dependencies. This evidence does not establish packaged WASM, alarms, transport, or deployment parity.

Adoption still requires the following client work:

- Iroh adapters pass raw old and new domain state and persist consumption identities at their existing boundaries.
- Apple retains the committed Finish snapshot and reserved entropy across the second call. Failed first persistence prevents the second call.
- Desktop maps raw trigger rows, authentication state, and ACK or discard evidence, then persists retirement and generated dependency metadata in its existing SQLite transaction.
- Adapters restore device identity on retained commands and retain physical observations separately. Generic reconciliation still requires existing terminal-overlap normalization.
- PWA and Android follow-up edges still diverge from Core's direct-parent contract, as documented in `COMPLETION_MUTATION.md`.
- No client invokes the added stages yet. Release, packaged ABI checks, client migration, and the main backlog remain independently owned.

Native verification uses Rust 1.97.1. Only `wasm_abi_handles_malformed_ranges_and_cleanup_in_a_real_host` and `c4_release_wasm_rejects_oversized_allocations_without_trapping` are filtered from all-target tests. No local WASM artifact is built. Production functions remain within 50 physical lines without a new size exception.

The production audit grows from 456 to 503 functions. Mean cyclomatic complexity changes from 3.44 to 3.51, and mean cognitive complexity changes from 2.75 to 2.82. Both p95 values remain 9. The added decisions enforce stage admission, exact provenance, trigger order, frozen entropy, ownership, and separate transaction boundaries. The maximum scores remain 19 cyclomatic and 28 cognitive. These increases are intentional capability and validation costs, not a parser or metric change.

## Independent checker regression evidence

The eight `checker_` cases were added before the three production fixes. The unchanged baseline produced seven failures and one passing null-or-present session control. The failures were:

- Expiry Start changed `{phase: "focus", generation: "5", explicit: true}` to `{phase: "short_break", generation: "5", explicit: false}`.
- Manual deferred Start and persisted restart changed the explicit `long_break` selection to `short_break` and cleared its flag.
- Both empty events accepted `sourceAccepted`. An `opportunity` with a rejected ACK produced Start while the same evidence under `canonicalInstalled` retired the trigger.
- Missing `centralizedSession.userId` admitted optimistic Start. Explicit null and present identities already retained the expected barrier behavior.

The corrected compiled-source comparisons also failed before the production fix. Apple expiry case 24 returned `focus/5/true` from production and `short_break/5/false` from Core. Manual selection step 1 returned `long_break/5/true` from production and `short_break/5/false` from Core. Neither comparison substitutes request fields for Core results.

After the fixes, the same eight regression tests pass. The lifecycle source probe passes 122 cases, including 28 complete Apple expiry selections and 12 manual Finish, Start, and restart snapshots. The prior centralized mutation and generated-break suites remain unchanged.

The recheck commands use the actual pinned compiler and Cargo binaries:

```sh
export PATH="$HOME/.rustup/toolchains/1.97.1-aarch64-apple-darwin/bin:$PATH"
cargo test --locked --test completion_lifecycle checker_ -- --nocapture
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --all-features --locked -- --exact --skip wasm_abi_handles_malformed_ranges_and_cleanup_in_a_real_host --skip c4_release_wasm_rejects_oversized_allocations_without_trapping
cargo build --locked --example completion_policy_probe
python3 scripts/completion_lifecycle_source_probe.py
```
