# PWA natural-completion admission

The later [PWA selection intent contract](PWA_SELECTION_INTENT.md) closes the Core-side explicit-choice and generation blocker.
Its [adapter recipe](PWA_SELECTION_ADOPTION.md) uses the same durable selection and lifecycle record across intent, read, Finish, and installation.
The verification counts below describe the earlier natural-completion milestone. The selection reference records the earlier 2342-case selection gate.
The later [cycle and discharge repair](PWA_CYCLE_REPAIR.md) records the current 2632-case gate and original-evidence requirement.

CORE-PWA12 extends the existing `workspace.completionMutation.v1`, `workspace.readModel.v1`, and `timer.completionState.v1` contracts. The change applies to centralized PWA completion. It does not change Android's captured-send profile or add a PWA Iroh stage.

## Exact completion evidence

A natural completion has a completed canonical timer and its exact completed history row. The timer's `lastIntent.type` is `start` or `resume`. The row has no `commandId`. The shared workspace boundary checks the timer identity, task, phase, duration, elapsed value, terminal time, and intent provenance.

An installed completed timer without its history row does not grant Finish admission. A fresh running timer can supply the same evidence through Core's deadline projection. Core never clears the canonical timer to make the input acceptable. The returned `workspace.base` remains the original raw aggregate.

## Finish command admission

Both `finishCommit` and `automaticFinishCommit` accept an exact presented natural completion. Natural-completion admission requires the local owner, including the existing PWA tab lease rules. A live peer tab returns `noop`, `reason: "not_owner"`, and `retryAtMs`. The lease becomes eligible at exact equality. A foreign device remains ineligible after lease expiry. Missing ownership uses the existing origin predicate.

The explicit Finish remains necessary after the display completes. Finish records command ownership, acknowledgement provenance, and allocation state. A successful Finish replaces the natural row's command provenance in the same history identity. It does not create another session or another completed focus. The original timer identity, task, phase, and planned duration remain authoritative.

The command is admitted through the full retained ledger. Any queued Finish for the same timer blocks a second Finish, even when delivery proof or the canonical head hides that command from the display. A retained replacement or changed lifecycle intent also blocks the old natural Finish. Frozen payloads remain unchanged. A timer whose last intent is already Finish still returns the existing `staleTimer` no-op. A wrong presented timer or fingerprint also returns `staleTimer`.

Core derives the next phase through the existing completion plan. Focus completion selects a short break unless the completed-focus count in the supplied day is a positive multiple of four. Break completion selects focus and does not generate another break. Effective preferences determine the generated break duration and auto-start setting.

An explicit selection preserves its phase, generation, and explicit flag. Natural completion does not auto-start over that choice. The same selection is preserved when a prior lifecycle consumption already advanced the phase. Ownership and retained Finish evidence still govern the separate command obligation.

## Lifecycle state

The existing optional Finish request `lifecycle` contains `consumedCompletions` and `pendingBreaks`. Both arrays default to empty. The completion identity remains `{timerId, commandId, phase}`.

A null command identity records consumption of the natural completion's presentation. A non-null identity records the discharged explicit Finish obligation. A natural Finish result remembers both identities. Natural-to-explicit provenance replacement therefore does not advance the phase twice. A persisted non-null identity blocks a repeat natural Finish with `alreadyConsumed`, including after a selection-generation change or restart.

The result retains schema version 1 and every existing Finish result field. An admitted natural Finish also returns `source`, the original natural history row, and `sourceStatus: "naturalExpiry"`. Commands, ownership writes, allocation, observation, lifecycle, and selection belong to the same existing transaction. Noop returns no commands, ownership writes, or effects.

## Read model

`workspace.readModel.v1` accepts optional PWA `selection` and `lifecycle` context. `selection` contains `phase`, a canonical nonnegative decimal `generation`, and `explicit`. Its phase must equal `selectedPhase`. These added fields are rejected for other read profiles.

An exact unconsumed natural completion advances the implicit display phase and exposes `finish` in `availableIntents`. The canonical result stays completed and all counts remain unchanged. An explicit choice controls the display. A retained Finish or a persisted discharged Finish removes the extra Finish intent. Reading does not persist consumption or create a command.

## Canonical installation

The existing `pwaRejectedFinish` install profile accepts optional `lifecycle` and returns it when supplied or when Core consumes a newly installed completion. Installation uses the exact current canonical pair, not an unrelated history row or a synthetic completion. The prior history and persisted lifecycle prevent repeated consumption across corrected timestamps, provenance replacement, generation changes, and restart.

A newly installed natural completion or remote explicit completion advances only a matching implicit selection. An explicit choice remains unchanged. Pending commands or other operations retain the existing installation barrier. Local sent Finish commands retain their existing reconciliation path unless exact natural-session evidence protects the selection.

A consumed natural identity remains authoritative when the same canonical session changes its terminal command provenance. Core first validates the current canonical timer and history pair. A matching null consumption identity or prior natural history row prevents applied, ignored, and rejected acknowledgements from entering the legacy PWA rollback path. Core preserves the complete current selection, including its explicit flag and generation. Start, Resume, remote Finish, and a valid terminal pair without an intent retain the same session identity. A different timer or phase does not receive this protection.

The old PWA rollback implementation does not provide this natural-session contract. Its rejected-Finish rule can select focus after remote Finish replaces the original natural provenance. The correction is intentional new Core behavior. Source comparisons of the unchanged storage adapter do not establish parity with that old rollback policy.

## Concrete JSON representations

`src/completion_schema.rs` describes the representations consumed by the new completion boundaries. Mutation, read, and install requests use the shared `strict_json::shape` validator before typed decoding. Enum controls must be JSON strings. Records must be objects, including selections, sent commands, canonical timers, histories, allocation records, and nested intent provenance. Lifecycle parsing uses the same schema.

`Shape::Fields` checks present fields while preserving each decoder's existing missing-field, nullable-field, and unknown-extension policy. `Shape::Scalar` rejects container encodings and leaves primitive validation to Serde. The change does not alter numeric parsing or globally round-trip input values. Genuine extension arrays, objects, omitted values, and explicit nulls remain valid where the existing wire contracts allow them.

Native metadata checks compare the schemas with the actual field names supplied by each Serde decoder. A newly added typed field without a representation guard fails the test. A separate flattened-intent construction checks the serialized provenance fields. `fixtures/pwa-completion-shapes-v1.json` inventories 426 paths. The artifact corpus rejects an invalid representation at every path. A schema-path change requires a fixture-backed explanation, and verification does not refresh the inventory.

## Verification evidence

`fixtures/pwa-natural-completion-v1.json` preserves all four original current and frozen public planner requests, complete returns, and full persisted state. It also preserves actual Go HTTP response bytes and the production installation trace. Both raw response strings decode exactly to their stored objects.

The official 0.45 artifact is `845090328b2f44056480c3930e9bb684a3874b8f3cbcbd4253ddd92f67c6f5d6`. The original four PWA tests fail with those bytes and pass through the unchanged production route with the native Core bridge. The full P222 completion suites pass 57 tests with that bridge. This is an intentional policy correction, not a claim that the old native and official natural-completion returns match the corrected result.

The source probe compares complete current and frozen storage returns for identical raw inputs. It also executes public `finishTimer`, checks durable state, and reopens the database. The live Go HTTP probe executes natural completion, public Finish, actual applied acknowledgement, response loss, database reopen, and exact request replay. The history identity and completed-focus total remain one throughout.

The baseline gate compares all 1077 old official envelopes and 20 numeric or error controls. Every old successful response and all 20 controls remain byte-identical. Of the original 1077 envelopes, 1067 remain byte-identical. Ten already-rejected malformed inputs now report the concrete representation error before typed decoding or projection. Four concern null or array request roots. Six concern malformed display-context containers and now include the full request path. `fixtures/pwa-completion-shape-errors-v1.json` preserves each exact old input, old envelope, corrected envelope, and reason. No valid result, count, ordering, or numeric token changes in this comparison.

`fixtures/pwa-natural-checker-v1.json` preserves the exact 12 independent checker failures and complete prior native returns. Two demonstrate rejected acknowledgements overwriting generation-20 selections after remote Finish changes provenance. Ten demonstrate alternative Serde encodings passing the new boundaries. The same inputs now preserve both selections or return an error envelope without a partial value. The unchanged independent checker passes all 146 cases. Its original script and prior receipts remain intact.

The expanded artifact corpus contains 1707 cases. It includes the prior natural-completion vectors, all 12 exact checker regressions, 72 provenance and acknowledgement scenarios, and 426 structural rejection paths. The artifact-gate and mutation suites pass 114 tests. Runner mutation checks reject missing lifecycle dispatches, missing structural dispatches, skipped provenance scenarios, changed consumption state, wrong timer identity, duplicate counts, and the wrong fourth-focus break. The source probe also aborts four malformed requests through the actual IndexedDB mutation transaction and confirms that every stored row remains unchanged.

Native verification uses Rust 1.97.1, format checking, warning-denying all-target Clippy, and all-target all-feature tests. Only `wasm_abi_handles_malformed_ranges_and_cleanup_in_a_real_host` and `c4_release_wasm_rejects_oversized_allocations_without_trapping` are filtered. No local WASM build or release is part of this evidence. The hosted artifact gate is extended, but a corrected published artifact has not been exercised.

The production audit has zero violations and no new size exception. Compared with the first CORE-PWA12 candidate, Core grows from 707 to 721 reported functions. Mean cyclomatic complexity changes from 3.55 to 3.52, and mean cognitive complexity changes from 2.86 to 2.83. Both p95 values remain 9. The checker fixes add declarative schemas, shared representation validation, and the consumed-natural-session decision. No metric parser changes are part of this work.

The existing PWA adapter still hardcodes implicit selection and does not persist or pass the extended lifecycle fields. Native substitution proves the corrected public command flow without changing those files. Client adoption of explicit selection and lifecycle persistence remains separately owned. The main task owns backlog updates.

The checker commands run from `pomodorough-core` with the actual pinned compiler first on `PATH`:

```sh
export PATH="$HOME/.rustup/toolchains/1.97.1-aarch64-apple-darwin/bin:$PATH"
cargo build --locked --example artifact_parity_oracle
node scripts/natural_completion_red_green.mjs
node scripts/natural_completion_checker_gate.mjs
node scripts/natural_completion_independent_recheck.mjs
node --test scripts/natural_completion_source_probe.cjs scripts/natural_completion_http_probe.cjs
node scripts/natural_completion_baseline_gate.mjs
node tests/aggregate_wasm_parity.mjs --native-only
node --experimental-vm-modules --test scripts/test_aggregate_artifact_gate.mjs scripts/test_aggregate_artifact_native.mjs
```

The temporary evidence directory contains `core-pwa12-original-four-red-green.json`, `core-pwa12-checker-red-green.json`, `core-pwa12-independent-recheck.json`, `core-pwa12-source-green.json`, `core-pwa12-http-green.json`, and `core-pwa12-official-envelope-comparison.json`. The default directory is `/var/folders/r_/_mr22dqn24d31b7460cz8z5m0000gn/T/opencode`. `PWA12_EVIDENCE_DIR` selects another existing directory. The source probe requires the frozen 0.45 PWA files, and the baseline gate requires the preserved 1077-case envelope capture. These inputs are never refreshed by the verification commands.
