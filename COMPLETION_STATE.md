# Completion state decisions

`timer.completionState.v1` implements the bounded CORE-M02 completion-consumption and selected-phase capability. It extends `completion_plan` with durable installation and provisional-selection rollback. It does not implement the full intent or read-model migration. No client calls this new operation yet.

## Input schema

The operation accepts two tagged requests. The original request fields below are required except nullable `canonicalTimer`. The Android/PWA extension adds `sentContext`, whose profile-specific requirements appear below. Unknown fields in request-owned structures are rejected. Existing timer and history wire structures retain their existing extensibility.

`kind: "install"` accepts:

- `compatibility`: `appleAp04`, `desktopD03`, `androidCapturedSend`, or `pwaRejectedFinish`. These names pin observed client behavior rather than silently selecting one policy for every client.
- `beforeHistory`, `afterHistory`: durable history before and after canonical installation. These use the existing history wire schema. Apple and Desktop completion selection permits `endedAt` when `completedAt` is absent. Android and PWA additionally enforce the production completionPlan history boundary when computing a destination, as described below. History IDs and timer IDs must be unique within each snapshot.
- `canonicalTimer`: the installed canonical timer, or null.
- `selection`: `{phase, generation, explicit}`. `generation` is canonical nonnegative decimal text, bounded by signed Int64 maximum. Text preserves Apple's full generation range through JavaScript bridges. `explicit` is a boolean.
- `pending`: `{commandIds, sendableCommandIds, otherOperationIds}` after reconciliation. IDs must be nonempty and unique within each list. Sendable command IDs must be a subset of command IDs. Other operation IDs cover pending task and settings operations.
- `advances`: ordered durable `{commandId, timerId, previousPhase, advancedPhase, generation}` records. Apple order is creation order. Desktop order is the persisted query order. Generations use the same decimal representation as selection.
- `acknowledgements`: unique `{commandId, outcome}` records. Outcomes are `applied`, `ignored`, and `rejected`. The caller supplies acknowledgements already matched to the captured request by reconciliation.
- `discardedCommandIds`: operations discarded by an explicit resolution or dependency reconciliation. Desktop uses these IDs to resolve provisional selection records even without an acknowledgement. Apple ignores them for selection rollback, whether or not an acknowledgement exists. Adapters supply the complete discarded-ID list without filtering by profile.
- `referenceTime`: an explicit RFC 3339 timestamp.
- `calendarIntervals`: `{start, end}` half-open civil-day intervals computed by the platform calendar. Intervals may be supplied out of order but must not overlap. A decision requires the source day's interval. Apple also requires the reference day's interval. Core does not guess a timezone or assume a 24-hour day.

`kind: "skip"` accepts `selection`, `sourcePhase`, `history`, `referenceTime`, and `calendarIntervals`. It computes an idle skip destination without adding a completion. Focus skips select long break after 3, 7, 11, and subsequent counts congruent to 3 modulo 4. Both break phases skip to focus. This differs from completion cadence, which selects long break after positive multiples of four.

## Output and persistence

Both requests return:

- `selection`: the complete resulting selection state.
- `source`: null or `{historyId, timerId, commandId, phase, occurredAt}`. The source retains raw identity and timestamp text. Absent command IDs remain null, not fabricated finish IDs.
- `reason`: `noNewCompletion`, `activeTimer`, `pendingCommands`, `explicitSelection`, `provisionalAdvance`, `pendingOperations`, `outsideReferenceDay`, `selectionDiffersFromSource`, `completionSelected`, or `skipSelected`.
- `advances`: unresolved durable advances in their original order.
- `retiredAdvanceIds`: resolved advance IDs, in resolution order.
- `rolledBackAdvanceIds`: the subset that actually restored selection.

The reason describes the installation selection decision, not acknowledgement acceptance. A rollback can occur before an installation is blocked. Effects report that rollback independently.

The caller persists selection, unresolved advances, and the canonical snapshot in one transaction. The durable `afterHistory` becomes `beforeHistory` on the next call. Desktop therefore consumes an installed completion even when pending work blocks selection, matching D03. Retrying the same before/after request is deterministic. Restarting with the installed snapshot does not reconsume it. The output does not return rewritten commands or history rows.

## Preserved client policies and divergences

The implementation reads these client decisions as the parity baseline:

- Apple `TimerSessionController.derivedNextPhase` chooses the latest completion, breaking equal-time ties by greatest timer ID. It requires the reference civil day. `AccountSynchronization.mergeSyncedSnapshot` protects explicit selection and blocks on any pending timer command. It derives from the latest history on repeated installation, rather than consuming a new identity.
- Desktop `storage_canonical_installation._latest_completed_source` chooses the smallest timer ID among newly consumed completions at the newest millisecond timestamp. Apple retains submillisecond ordering. Its consumed identity is timer ID, phase, and command ID with timer-ID fallback. A changed timestamp or history row ID does not make that identity new. Older backfill does not displace an already-consumed latest completion. D03 uses the completion's day even when the reference day differs. It protects a selection different from the source phase, not an explicit-choice flag. It blocks on unresolved phase advances or sendable commands and other pending operations, but not blocked-only commands.
- Apple rollback rejects an acknowledged advance when the outcome is rejected or exact canonical finish evidence is absent. Only an invalid acknowledgement starts suffix invalidation. An unacknowledged advance remains unresolved unless an invalid acknowledged predecessor invalidates it, even when its ID appears in `discardedCommandIds`. Apple invalidates the remaining ordered advance suffix and unwinds in reverse. It restores the previous generation, wrapping zero to Int64 maximum. History evidence matches timer and command, without checking phase. Timer evidence additionally requires a finish intent.
- Desktop rollback restores only non-applied or discarded advances without exact canonical evidence. Applied acknowledgements need no canonical evidence. Rejected acknowledgements with exact evidence do not roll back. Evidence checks the source phase. Desktop restores phase without decrementing generation and does not invalidate an ordered suffix.
- Both rollback policies require matching current phase and generation. A later explicit choice, including choosing the same phase again, protects selection through its changed generation.
- Android `CentralizedSyncCoordinator.reconciledSelectedPhase` protects the send-time selection and generation, folds sent finishes in sequence/ID order, and can restore a canonical timer phase. The `androidCapturedSend` profile supplies the raw context described below.
- PWA `app-actions.js` rolls rejected finishes backward by device sequence, tests phase equality against a recomputed destination, and has no generation fence in that function. The `pwaRejectedFinish` profile explicitly preserves that behavior. It does not weaken the Apple or Desktop profiles.

## Completion dependencies and remaining scope

Command eligibility, explicit ownership, natural expiry, and generated-break ownership remain in `timer.completionPlan.v1`. Its `expiry`, `commandRequest`, `finishApplied`, and `generatedBreak` contracts remain unchanged. The shared focus-count helper now also supports skip cadence.

Generated-command graph validation, rejection cascades, promotion, and immutable delivery remain in `reconcile.rebase.v2`, `reconciliation/timer_dependencies.rs`, and M01 workspace projection. This capability consumes the reconciled pending IDs and discarded IDs. It does not mutate graph edges, command payloads, reserved identities, or delivery proofs. Apple's ordered selection suffix is a separate dependency rule from the generated-command graph.

Still open for the main backlog owner:

- Client adapters and transaction integration, including ownership and graph composition at each caller.
- A unified product policy for tie order, explicit same-phase choices, old-day installation, and acknowledgement evidence. The compatibility profiles expose the differences without resolving them by accident.
- Android A04 remote-completion correction and PWA generation migration. The captured-send and rejected-finish parity decisions are implemented below; neither is a product-policy correction.
- Recording provisional advances from local intents, advancing selection generations for explicit choices, and allocating generated commands. The current module resolves existing durable records; it does not replace those intent paths.
- Caller retention of enough durable history to establish consumption. Truncating `beforeHistory` can intentionally make a historical identity appear new, as in a fresh resolution installation.
- Exotic non-UUID identities and timestamp precision beyond client clock precision. Core compares raw UTF-8 IDs without Unicode normalization and parses RFC 3339 fractions with Chrono. Fixtures cover raw Unicode retention, not every Swift string-collation or floating-point Date edge.

`fixtures/completion-state-v1.json` records AP04/D03 selection parity and explicit divergence cases. `tests/completion_state.rs` runs those cases in both history orders and adds restart, backfill, cadence, rollback-chain, generation, raw-identity, calendar, and malformed-input coverage. These are native shared fixtures, not client test executions.

## Verification receipt

Native verification uses Rust 1.97.1 for both Cargo and rustc. The host's Homebrew rustc otherwise overrides Cargo's compiler selection, so the verification environment explicitly sets `RUSTC` and puts the pinned toolchain's bin directory first in `PATH`.

- `cargo fmt --all -- --check` passes.
- `cargo clippy --all-targets --all-features --locked -- -D warnings` passes.
- `cargo test --all-targets --all-features --locked -- --skip c4_release_wasm_rejects_oversized_allocations_without_trapping --skip wasm_abi_handles_malformed_ranges_and_cleanup_in_a_real_host` passes 266 tests, including 25 completion-state tests. Exactly the two WASM-building tests are filtered out.
- The suite size audit reports zero violations. The new production code needs no size exceptions.
- The complexity report gives Core cyclomatic mean 2.80, cognitive mean 2.01, and p95 values 8 and 8. The new exact-evidence predicate has cyclomatic complexity 12 because it preserves both clients' different history and timer evidence rules. The profile branches are intentional and covered by divergence tests. Completion counting is extracted for reuse. The ACK-only correction reduces the reported cyclomatic mean from 2.81 to 2.80.
- `git diff --check` passes.

The patch changes `src/completion_plan.rs` and adds one dispatcher arm to `src/lib.rs`. New files are this reference, `src/completion_plan/state.rs`, `src/completion_plan/state/rollback.rs`, the fixture, and the test file. Existing C01, C02, C03, and M01 changes remain in place and their native tests pass. Client code, backlog files, WASM assets, lockfiles, and release metadata are not changed by this patch.

## ACK-only parity correction

The checker found that the first implementation treated Apple discard-only records as invalid. The regression `apple_discard_only_retains_unacknowledged_advance` reproduced the defect before the fix: selection `short_break`, generation `"1"`, became `focus`, generation `"0"`, without an acknowledgement.

`rollback::resolution` now returns Apple's decision directly from the matching acknowledgement. With no acknowledgement it returns unresolved. The existing suffix resolver still invalidates descendants of a rejected or evidence-missing acknowledged predecessor. Desktop's discard predicate is unchanged.

Seven additional tests cover discard-only retention, acknowledged discard invariance, rejected-parent suffixes, canonical-timer-only evidence, the Desktop discard/acknowledgement matrix, an unresolved prefix with an acknowledged child, and Desktop multi-record ordering. The wrap test now uses a rejected acknowledgement instead of a discard-only record. Canonical timer cases distinguish ID, status, phase, missing intent, intent type, and command identity according to each production predicate.

## Android and PWA parity extension

This extension adds two compatibility values and one optional install field, `sentContext`. The `skip` request and output schema are unchanged. Existing Apple and Desktop inputs omit `sentContext` or supply null. Those profiles reject a non-null context. Android and PWA require a matching context variant and an empty `advances` list. Their clients have no corresponding durable advance records to retire. Existing pending and discarded fields remain accepted but do not gate these two profiles.

Android supplies:

```json
{
  "kind": "android",
  "commands": [],
  "selectionAtSend": {"phase": "short_break", "generation": "2"},
  "acknowledgementHistory": [],
  "acknowledgementTimer": null,
  "nextProjectionTimer": null
}
```

`commands` is the captured request's complete command list. Core filters finishes and orders them by ascending device sequence, then ID. `selectionAtSend` contains the captured raw phase and generation, not a caller-computed race flag. Null or omitted capture models bootstrap resolution, whose production call passes two nulls. `acknowledgementHistory` and `acknowledgementTimer` come from the response that acknowledged the commands. The existing install `afterHistory` and `canonicalTimer` come from the response being installed. They can differ during bootstrap resolution. `nextProjectionTimer` is the actual pending projection result. The two nullable timer fields must be present. Request-owned unknown fields fail closed.

PWA supplies:

```json
{"kind": "pwa", "commands": [], "rollbackHistory": []}
```

Here `commands` means the current pending command list, not only the captured send list. `rollbackHistory` is the current optimistic history before installation. This distinction comes from `app-sync.js:120-122`, which passes `state.pending` and `state.history`. Replacing it with the incoming canonical history changes the cadence calculation. Core filters rejected finishes and folds descending device sequence; equal sequences preserve input order.

Both command lists carry raw `id`, `timerId`, `type`, `phase`, `deviceSequence`, `occurredAt`, and optional nullable `physicalOccurredAt`. Unused wire extensions are accepted, with duplicate JSON keys still rejected recursively. IDs must be nonempty and command IDs unique. Phases and occurrence timestamps must satisfy the existing Core wire contract. Sequences are nonnegative JavaScript-safe integers. Generation remains canonical nonnegative Int64 decimal text. A malformed Android physical timestamp falls back to `occurredAt`, matching production. Acknowledgements use the existing unique, lowercase protocol outcomes. This bounded operation does not emulate PWA's coercion of malformed JavaScript inputs, missing sequence defaults, arbitrary outcome casing, or legacy non-wire history. No adapter may silently normalize those into new decisions; such inputs need a separate boundary contract if still supported.

### Decisions and result meaning

- Android first compares the current phase and generation against the capture. A mismatch returns `selectionChangedSinceSend`. Both values must match; returning to the same phase after a different choice still fences an old response.
- An Android finish without an acknowledgement does nothing. Evidence comes only from the acknowledgement response: any completed row or timer with the finish's timer ID. It intentionally does not require the command ID or phase to match.
- A non-applied Android acknowledgement without that evidence restores the installed canonical timer's phase if its ID matches, regardless of status. Otherwise it restores the finish's source phase.
- With acknowledgement-response evidence, Core derives the destination from installed history. It synthesizes a completed canonical timer row only when no completed history row already covers that timer. The reference uses the first matching completed history row with matching or absent command ID, then the matching completed timer anchor, then physical occurrence, then command occurrence. The existing completion cadence owns phase calculation.
- Applied Android acknowledgements without evidence use the projected timer phase only when its last intent names the finish. They otherwise retain selection.
- PWA considers only rejected acknowledgements. It recomputes the destination using `rollbackHistory` and the finish occurrence's civil day. It restores the source phase only when selection equals that destination. Applied and ignored outcomes do nothing. Canonical evidence, generation, and explicit-choice flags do not protect a matching phase in this profile.

New reasons are `noAcknowledgedFinish`, `selectionChangedSinceSend`, and `sentFinishesReconciled`. The last reason means at least one acknowledged finish was evaluated, not that selection necessarily changed. `source` remains null for these profiles because multiple commands can contribute to the result. Advance arrays remain empty. Generation and explicit-choice metadata pass through unchanged. Repeating an identical request is deterministic; an adapter must not replay an already-consumed callback with a different selection. Restart fixtures remove resolved commands and acknowledgements and confirm that installed history alone does not reconsume a finish.

`androidCapturedSend` names the observed algorithm rather than the open A04 bug. `pwaRejectedFinish` names the narrower acknowledgement rollback behavior, rather than implying remote-completion selection or generation protection.

### A04 and local intent scope remain open

The fixture `android-a04-remote-completion-still-open` records both current parity, `focus`, and desired A04 selection, `short_break`, when remote focus completion arrives without a locally sent finish. The parity profile deliberately produces the former. This is evidence of the remaining bug, not its final fix. A04 needs a separately reviewed remote-completion consumption policy with explicit user-choice protection and restart semantics. No client uses this extension yet, so this patch cannot fix A04 in production.

Local intent planning remains separate. `TimerMutationCoordinator.finish` computes the completion destination and provisional generated commands, but does not own generation increments. `TimerRepository.selectPhase:1351-1363` increments only for a different accepted phase; choosing the same phase returns early. `installTimerMutation:2503-2510` increments only when mutation settings change phase. The repository initializes the counter to zero in memory. These are not Apple's durable same-phase generation rules. Importing a generic explicit-choice or provisional-advance constructor here would change semantics without owning mutation acceptance, reserved identities, generated dependencies, and transaction commit. A future intent planner must return those effects together and decide whether generation becomes durable. This patch neither allocates commands nor creates advance records.

### Production-source differential evidence

Run the native example, then the probe:

```sh
cargo build --locked --example completion_policy_probe
python3 scripts/completion_source_probe.py
```

The probe reads current sibling source files each run. It extracts Android methods from `reconciledSelectedPhase` through `completionReference` and compiles those bodies with Kotlin. It extracts the actual PWA `CompletionPlanPolicy` class and executes it with Node. Test shims provide wire models and transport to the existing native `timer.completionPlan.v1` dispatcher. They contain no copied selection or rollback algorithms and do not call `completionState` to implement the baseline. Both production outputs and `completionState` must equal each fixture's expected phase. Compiler/runtime failures fail the probe rather than skipping evidence.

The probe uses UTC fixtures and defaults to Android Studio's bundled Kotlin compiler and JBR. `KOTLINC` and `JAVA_HOME` override those executable locations. Temporary Kotlin/JavaScript artifacts stay outside client trees. This is isolated production-method execution, not an Android application build, browser integration test, or WASM test.

The shared `completion-sent-v1.json` matrix currently passes 17 Android and 9 PWA production-source cases. It covers missing/applied/ignored/rejected acknowledgements, distinct response snapshots, broad evidence, projected intent, canonical phase restoration, newer choices, uncaptured resolution, ordered finishes, stable ties, fourth-focus cadence, optimistic-versus-installed history, invalid physical time fallback, and the explicit A04 gap.

Source SHA-256 values from the successful probe:

- `CentralizedSyncCoordinator.kt`: `bb54d1090d08bcd31db115d4c915d9d82aaeea7ba3b72fd9d1a14816424766ba`
- `app-actions.js`: `f7f974f650aa1359fbf495aab939f021ac4e583ef2f78fc04c91f23cc9c2e0bc`

The new native tests are `shared_android_pwa_captured_send_fixtures`, `restart_without_sent_commands_does_not_reconsume_response`, `context_profile_and_raw_identity_validation`, and `ordered_ties_distinguish_android_id_order_from_pwa_stable_order`. The existing 25 Apple/Desktop completion tests remain unchanged.

### Checker handoff

This extension changes only `src/completion_plan/state.rs`, its rollback module's sibling visibility and profile match, and this reference among the preexisting patches. It adds `state/sent.rs`, `tests/completion_sent.rs`, `fixtures/completion-sent-v1.json`, the native example, and three production-source probe files. It does not edit clients, backlog, completionPlan, dispatcher wiring, C01/C02/C03/M01 files, release metadata, lockfiles, or WASM assets.

Native gates use `RUSTC=$HOME/.rustup/toolchains/1.97.1-aarch64-apple-darwin/bin/rustc` and prepend that toolchain's bin directory to `PATH`. The full native test command retains both explicit WASM-building skips from the earlier receipt. The verified suite passes 270 tests with exactly two filtered tests. Formatting and all-target/all-feature locked Clippy pass. The size audit reports zero violations and seven preexisting documented exceptions; this extension adds no exception. Core complexity means change from the earlier recorded 2.80/2.01 to 2.85/2.06 because the requested independent captured-send and reverse-rollback branches are now represented in Core. The p95 values are 7/8. Fixture-backed profile differences justify that increase; no existing reducer was made more complex to absorb these policies.

Remaining integration blockers belong to the main backlog owner: client transaction adapters and callback consumption, A04's desired remote-completion correction, PWA generation protection, local intent planning and generation persistence, non-wire legacy input policy, and eventual WASM/release verification. This receipt is ready for independent review of the bounded parity extension, not closure of all CORE-M02 work.

## Checker correction: required timers and completion history

The checker identified two defects in the first Android/PWA extension. Both now have regressions that failed against the preceding implementation.

### Required nullable timer fields

Serde accepted omitted `acknowledgementTimer` and `nextProjectionTimer` even though their types were `Box<Option<CanonicalTimer>>`. Each field now uses `deserialize_with = "required_timer"`. The custom deserializer accepts an explicit null or a timer object, but Serde rejects an omitted field before invoking it. The JSON schema has not expanded; the implementation now enforces the documented required presence.

Separate tests, `android_requires_acknowledgement_timer_presence` and `android_requires_next_projection_timer_presence`, first verify that explicit null succeeds, then remove only the named field and require rejection. Both omission assertions failed before the fix and pass afterward.

### Profile-specific history boundary

The shared completionState input validator retains its existing endedAt fallback. Apple and Desktop still use that fallback for completion selection, and `skip` is unchanged. The sent profiles now call the same `timer::validate_history` used by `timer.completionPlan.v1` immediately before computing a destination:

- Android validates the history returned by `android_history`, including any synthesized canonical completion. This occurs only when acknowledgement-response evidence leads production to call `finishApplied`.
- PWA validates `rollbackHistory` only for a rejected finish, when production calls `finishPlan`.

A completed row at these dispatch boundaries must have a valid `completedAt`. An absent or null `completedAt` with only `endedAt` produces `CoreError::InvalidInput("invalid timer history")`. Core does not normalize the row before this check. Branches that never call completionPlan retain their prior behavior, including Android's newer-choice fence, missing acknowledgements, and PWA applied acknowledgements. `sent_history_validation_follows_production_dispatch_boundary` protects these distinctions.

The shared fixtures add `android-ended-only-completion-rejected` and `pwa-ended-only-completion-rejected`. The native example now serializes `InvalidInput` errors for the probe. Kotlin and JavaScript transport shims propagate these errors through the actual extracted production methods. Unexpected failures still fail verification. No shim implements history validation or rollback policy.

Baseline differential results before the production fix:

```text
android-ended-only-completion-rejected: production=error:invalid timer history core=short_break
pwa-ended-only-completion-rejected: production=error:invalid timer history core=focus
```

After the fix, both production and completionState return `error:invalid timer history`. All 28 source cases pass: 18 Android and 10 PWA, including the original 26 positive cases. The source hashes above remain unchanged.

The pinned Rust 1.97.1 checks pass: formatting, all-target/all-feature locked Clippy, and the full native suite with the same two explicit WASM-building skips. The suite now passes 274 tests, including eight sent-profile tests and all 25 unchanged Apple/Desktop tests. The relevant two test binaries also passed separately. The size audit reports zero violations with no new exceptions. Core complexity means are 2.84 cyclomatic and 2.06 cognitive, with p95 values 7 and 8. These corrections do not add decision branches to the phase algorithms. The new deserializer adds one straight-line function.

Correction files are `src/completion_plan/state/sent.rs`, `tests/completion_sent.rs`, `fixtures/completion-sent-v1.json`, `examples/completion_policy_probe.rs`, the three `scripts/completion_*` probe files, and this reference. The Apple/Desktop implementation, C01/C02/C03/M01 patches, clients, backlog, and release assets remain untouched. A04 and the other integration blockers above remain open.
