# Backlog

## Fix status (HEAD + working tree, all gates green)

- Fixed: frozen task-delete normalization (`sync_projection.rs` title `skip_serializing_if`; assertion in `retarget_and_replay_pages.rs`).
- Fixed: verifier ABI signatures (`verify_wasm_artifact.py` type-section enforcement; `scripts/test_verify_wasm_artifact.py`).
- Fixed: history `id` collision (`timer.rs` `validate_history` returns history+timer ids, overlap rejects either).
- Fixed: replay-page clamp bypass (`replay_page.rs` raw elapsed bounds check).
- Fixed: cross-device `device_sequence` (`timer_dependencies.rs` requires same `device_id`).
- Reclassified (false positive, not fixed): `completion_plan` overlap — 5 existing tests require overlap acceptance as the planning-snapshot contract.
- Fixed: release-contract unnamed steps + exact argv (`c5_release_contract.py`; tests in `test_c5_release_contract.py`).
- Fixed: bootstrap strict parse + flag matrix (`bootstrap.rs`; table cases + duplicate-field test).
- Fixed: duplicate op ids (`sync_projection.rs` post-validation uniqueness) + dangling base selection (`projection.rs`; tests in `c29_c31_review.rs`).
- Gates: `cargo fmt --check`, `cargo test --all-targets --locked` (34 binaries, 0 failures), `cargo clippy --all-targets --all-features --locked -- -D warnings`, wasm build + canonicalize + verify, `tests/*.mjs`, 64 Python tests — all pass.

## P2: Frozen task-delete payload changes during v2 reconciliation

- Location: `src/reconciliation/delivery.rs:43-44`, related serialization `src/sync_projection.rs:134-143`.
- Contract: `IMMUTABLE_RECONCILIATION.md:29,35,45` requires exact retry of frozen operations; no payload normalization without acknowledgement.
- Repro: call `reconcile.rebase.v2` with a local `taskOperations` delete operation omitting `title`, empty `sent`, and a canonical response with empty acknowledgement arrays. Use `taskId: "task"`, `deviceId: "device"`, `occurredAt: "2026-07-20T12:00:00Z"`, `hlcWallMs: 1784548800000`, `hlcCounter: 0`, plus a valid durations map and server HLC greater than the operation.
- Expected: `pendingTaskOperations[0]` preserves the original object, including omitted `title`.
- Actual: succeeds, but the returned operation gains `"title": ""` while retaining the same ID.
- Cause: `Policy::from_request` deserializes local queues and serializes them again before recording frozen payloads; `TaskOperation.title` defaults to `""` and always serializes, so the frozen comparison sees two already-normalized objects.
- Impact: a client following the prescribed persistence/retry flow replaces a possibly delivered operation with a different payload.
- Coverage gap: `tests/retarget_and_replay_pages.rs:161-195` exercises a title-less delete but checks only identity/clock fields, not complete payload equality.

## P2: Artifact verifier accepts WASM with incompatible ABI signatures

- Location: `scripts/verify_wasm_artifact.py:93-110`, especially line 102.
- Contract: `README.md:36` promises the canonical producer validates the ABI.
- Repro: build a minimal valid WASM module exporting `pomodorough_alloc`, `pomodorough_dispatch`, `pomodorough_free`, and `pomodorough_free_v2`, each typed `() -> ()`, then run `validate()` from `scripts/verify_wasm_artifact.py`.
- Expected signatures:
  - `pomodorough_alloc: (i32) -> i32`
  - `pomodorough_dispatch: (i32, i32, i32, i32) -> i64`
  - `pomodorough_free: (i32, i32) -> ()`
  - `pomodorough_free_v2: (i32, i32) -> i32`
- Expected: verifier rejects incorrect function signatures.
- Actual: verifier accepts the module and returns a digest; in Node, `WebAssembly.validate` is true while `alloc(8)` and `dispatch` return `undefined`.
- Cause: the verifier checks export names and kinds but discards function indices and never resolves function types.
- Impact: verifier reports success for an artifact unusable by host adapters. This establishes verifier failure, not demonstrated publication bypass; the release workflow runtime tests would reject this particular artifact.
- Fix direction: resolve exported function indices through import/function/type sections and enforce exact parameter/result types.

## High: Duplicate history `id` via canonical/history `id` collision accepted

- Location: `src/timer.rs:300-317`, overlap check `308-310`; history validation `344-375`; canonical history id construction `711-712`.
- Repro: call `timer.reduce.v1` with `commands: []`, canonical timer `id: "dup-id"`, status completed, and `history: [{ id: "dup-id", timerId: "other-timer", ... }]`, plus matching `now`.
- Expected: `InvalidInput: canonical timer overlaps timer history`.
- Actual: accepted; output `history` contains two entries with `"id": "dup-id"` (`timerId` `dup-id` and `other-timer`). Same exposure for an active canonical timer on the next finish.
- Cause: `validate_replay_state` checks the canonical `id` against timer IDs only; `validate_history` builds history IDs but returns only timer IDs.
- Impact: breaks history identity uniqueness and deterministic output.
- Coverage gap: `tests/production_reconciliation.rs:765-785` checks `timerId` overlap only. Same entry point is used via `src/reconciliation/validation.rs:137` and `src/projection.rs:82`.

## Medium: `timer.replay.page.v1` accepts invalid `elapsedAtAnchorMs` via clamp bypass

- Location: `src/timer/replay_page.rs:106-137`, check at `124`; clamp `src/timer.rs:762-774`; raw session storage `776-793`.
- Repro: call `timer.replay.page.v1` with one running session having `plannedDurationMs: 60000` and `elapsedAtAnchorMs: 9999999` (or negative `-5`), with matching `currentTimerId` and `now`.
- Expected: `InvalidInput` for invalid canonical timer, matching `timer.reduce.v1` behavior for the same elapsed value.
- Actual: accepted; oversized elapsed returns a completed canonical timer, while `-5` persists unclamped in the session with a clamped canonical value of `0`.
- Cause: `restore_session` validates the clamped canonical projection rather than the raw session value.
- Impact: false completion, invalid session propagation, and divergence between paged and full replay.
- Coverage gap: `tests/retarget_and_replay_pages.rs:81-129,389-440` covers ordering/duplicates/missing current/omitted device, but not elapsed bounds.

## Medium: Generated-break `newer_manual_start` compares `device_sequence` cross-device

- Location: `src/reconciliation/timer_dependencies.rs:344-349`; correctly device-scoped comparison at `116-135`.
- Repro: call `reconcile.rebase.v1` with a generated break start on `device-a`, an acknowledged matching finish, a valid generated-break dependency, and an unrelated manual start on `device-b` with a large `device_sequence`.
- Expected: the unrelated device does not affect acceptance; generated start stays promoted and no break is dropped.
- Actual: without the unrelated operation, `promoted: ["command-gen"]`; with it, `promoted: []` and `droppedTimerIds: ["break-1"]`.
- Cause: the newer-manual-start check compares raw `device_sequence` values without filtering by `device_id`, although sequences are per-device.
- Coverage gap: `tests/retarget_and_replay_pages.rs:443-503` covers the single-device case only.

## Medium: `completion_plan` accepts overlapping canonical/history rejected elsewhere (RECLASSIFIED: false positive, not fixed)

- Status: reclassified during fix pass. Enforcing the overlap check in `validate_projection` breaks 5 existing tests (`tests/completion_plan.rs`: expiry + generated-break suites use `beforeTimer` id `timer-4` with history entries carrying `timerId: "timer-4"` and expect success), so overlap acceptance is the intended planning-snapshot contract, not a bug. `validate_projection` keeps separate timer/history validation.
- Original report below for reference.

- Location: `src/completion_plan.rs:337-344`, used by expiry plan `90-116` and generated-break plan `238-260`.
- Repro: call `timer.completionPlan.v1` with kind `expiry`, `beforeTimer`/`projectedTimer` id `timer-4`, and `history: [{ id: "h1", timerId: "timer-4", ... }]`.
- Expected: `InvalidInput: canonical timer overlaps timer history`, matching `timer.reduce.v1` and `projection.apply.v2`.
- Actual: accepted with `expired: true` and break selection derived from an impossible projection.
- Cause: `validate_projection` checks timer and history separately and never applies the canonical/history overlap check.
- Coverage gap: `tests/completion_plan.rs:199-225` checks id mismatch/status, but not overlap.

## High: Release-contract validator ignores unnamed workflow steps

- Location: `scripts/c5_release_contract.py:732-748` (`_workflow_steps`), `783-807` (`_require_job`).
- Repro: insert an unnamed `run:` step (for example invoking `curl`) before a known named step in `.github/workflows/ci.yml` or `release.yml`, then run the corresponding `validate_ci_workflow` / `validate_release_workflow`.
- Expected: validator rejects the workflow with unrecognized step structure.
- Actual: validator accepts it because it collects only `name:` steps and compares only those names.
- Cause: GitHub Actions executes `run:` steps without `name:`, while the parser treats `name:` as the complete step inventory.
- Impact: arbitrary CI/release execution bypass; breaks the fail-closed workflow contract.
- Coverage gap: `scripts/test_c5_release_contract.py` has no unnamed-step injection test.

## High: Release-contract validator accepts neutered required commands

- Location: `scripts/c5_release_contract.py:658-662` (`_matches_command`), `664-694` (`_require_run_commands`), expected commands `231-375`.
- Repro A: append `&& true` to the required `verify-bundle --directory dist` command in `release.yml`, then run `validate_release_workflow`.
- Repro B: replace the required `verify_wasm_artifact.py dist/pomodorough_core.wasm --sha256 "$digest"` invocation with `verify_wasm_artifact.py --help`, then run `validate_release_workflow`.
- Expected: validator rejects modified safety commands and requires exact verification/build argv.
- Actual: validator accepts both; prefix matching plus an explicit `&&` allowance permits appended operators, while `--help` exits successfully without verifying the artifact. Directory substitution has the same acceptance behavior.
- Impact: release can proceed without verification/build guarantees.
- Coverage gap: no tests for `&& true`, `--help`, or argument substitution.

## High: Bootstrap history skips recursive duplicate rejection

- Location: `src/bootstrap.rs:16,18` (`local_history`/`remote_history: Vec<Value>`), `src/bootstrap.rs:41` (`serde_json::from_str`, not `strict_json::parse` at `src/strict_json.rs:8`).
- Repro: call `bootstrap.plan.v1` with `{"localHistory":[{"id":"a","id":"b","status":"completed","timerId":"t"}],"remoteHistory":[]}` via `dispatch_json`.
- Expected: `Err` containing `duplicate field 'id'`, matching `projection.apply.v2` with same inner duplicate shape.
- Actual: `Ok({"mode":"auto","strategy":"replace_remote","reason":"local_only"})`; inner duplicate silently last-wins and item counted.
- Cause: history items typed as `Value` and parsed with plain `serde_json`, bypassing strict duplicate-field rejection used by reconciliation/projection/sync paths.
- Impact: duplicate identity inside history flips `completed_history_count` / choose-vs-auto decision. Breaks duplicate-field contract; native + WASM share path.
- Coverage gap: `tests/c2_round5_recursive_json_validation.rs` covers projection/rebase/standalone adapters, never bootstrap history items.

## Medium: Bootstrap state-flag matrix drops remote non-history state

- Location: `src/bootstrap.rs:70-96`, fallthrough past choose/local_only/remote_only to merge/empty on flags alone.
- Repro A: call `bootstrap.plan.v1` with `{"localHistory":[],"remoteHistory":[],"hasLocalState":true,"hasRemoteState":true}`.
- Expected: reason reflecting both sides' state; cannot be `local_state_only` when `hasRemoteState:true`.
- Actual: `{"mode":"auto","strategy":"merge","reason":"local_state_only"}` while remote state present.
- Repro B: call with `{"localHistory":[],"remoteHistory":[],"hasLocalState":false,"hasRemoteState":true}`.
- Expected: cannot be `empty` when remote state exists; compare `remote_only` branch at `89-91` which requires completed history, so flag-only remote state has no branch.
- Actual: `{"mode":"auto","strategy":"keep_remote","reason":"empty"}` while remote state present.
- Impact: wrong strategy persisted; merge discards remote-state signal, empty masks remote-only state. Client bootstrap can push wrong direction.
- Coverage gap: `tests/production_reconciliation.rs:1064-1093` + `tests/unit_positive.rs:37-60` cover flag+completed-history combos, never both-flags-true with empty histories nor remote-flag-only.

## High: Duplicate operation IDs accepted in projection/standalone, rejected in rebase

- Location: `src/projection.rs:200-231` validate_task/duration/autoStart/selected operations, no uniqueness check; `src/sync_projection.rs:169,286,354,426` replay_tasks/replay_durations/replay_auto_start/replay_selected_task, no uniqueness check. Reference correct: `src/reconciliation/validation.rs:156-191` local_queue_ids rejects, `src/timer.rs:268-278` check_unique_command_ids rejects.
- Repro `task.reduce.v1`: two upserts with same `id: "dup-op"`, different `taskId`/title Alpha/Beta.
- Expected: err like rebase `invalid local taskOperations identities`.
- Actual: ok with both tasks projected and same op ID in `winningOperationIds` for two tasks. Same class confirmed for `duration.reduce.v1` (dup `dup-dur`), `autoStart.reduce.v1` (dup `dup-auto` silently drops true), `selectedTask.reduce.v1` (dup `dup-sel` silently picks max clock), and `projection.apply.v2` with dup ops.
- Cause: projection/standalone validators never check ID uniqueness; rebase does.
- Impact: breaks ack semantics (one ack ID, two payloads), deterministic ordering unsafe, rebase/projection roundtrip divergence. Frozen neverSent/delivery proof cannot distinguish payloads.
- Coverage gap: `tests/production_reconciliation.rs:788` covers rebase rejects duplicates. No projection/standalone duplicate-ID negative case; `tests/c2_round5_recursive_json_validation.rs:105` only checks duplicate JSON keys, not duplicate ID values.

## Medium: Dangling base selectedTaskId silently cleared in projection, rejected in rebase

- Location: `src/projection.rs:188-198` validate_base_selected_task only rejects empty, no membership check. Reference correct: `src/reconciliation/validation.rs:145-153` canonical_response rejects empty or not-in-tasks.
- Repro `projection.apply.v2`: base with `selectedTaskId: "task-missing"`, `tasks: []`, empty pending queues.
- Expected: err like `invalid base selected task identity`, matching rebase `invalid canonical response selectedTaskId`.
- Actual: ok with `selectedTaskId: null`; forged base accepted, scrubbed.
- Cause: base selection validation lacks task-membership check applied to canonical responses.
- Impact: forged base passes optimistic projection but fails canonical validation; roundtrip mismatch, persists invalid snapshot, hides client bug. Violates missing/null/value + base validation parity (base tasks/durations strictly validated, selection not).
- Coverage gap: `tests/projection_validation.rs:48-55` checks empty base selection rejects, no dangling test; `tests/c3_reconciliation_projection_roundtrip.rs:364` covers forged base tasks, not forged base selectedTaskId.
