# PWA selection intent contract

The later [PWA cycle and discharge repair](PWA_CYCLE_REPAIR.md) supersedes the cycle lifetime and unverified discharge behavior in this first candidate.
It records the current schema, 2632-case hosted corpus, and the three old checker assumptions that are now rejected.
The verification counts below describe the earlier selection milestone.

`workspace.intent.v1` now owns explicit PWA phase choices and their selection generation.
The opt-in request carries the same `selection` and `lifecycle` fields as the existing PWA `completionState` record.
This closes the Core-side natural-completion adapter blocker before the next Core publication.

## Request and result

The existing request gains an optional top-level `lifecycle` object.
Its presence enables the new policy for `compatibility: "pwaStorage"` and `replicationMode: "centralized"`.
The existing required `selection` contains the raw persisted phase, generation, and explicit flag.
`lifecycle` uses the existing completion schema. Both arrays default to empty.

The context has this form:

```json
{
	"selection": {"phase": "focus", "generation": "7", "explicit": false},
	"lifecycle": {"consumedCompletions": [], "pendingBreaks": []}
}
```

The user action remains either `{"kind":"selectPhase","phase":"focus"}` or `{"kind":"skip"}`.
Core derives the resulting phase, sets `explicit: true`, and increments the persisted generation once for each accepted action.
Choosing the same phase is an action. Core returns `outcome: "planned"` even when the phase already matches.
The returned generation distinguishes that choice from an older automatic selection.

The generation is a canonical nonnegative decimal string in the signed 64-bit range.
The maximum accepted successor is `"9223372036854775807"`.
A choice at that value fails with `selection generation exhausted`. Core never wraps or resets the value.
Other eligible no-op actions can retain the maximum generation without allocating a command.

PWA Skip uses the existing Core Skip rule and the supplied calendar intervals.
Focus selects a long break when the completed-focus count modulo four equals three.
Other focus counts select a short break.
Either break selects focus. Repeated Skip actions read the prior committed selection.
The platform supplies day boundaries. The adapter does not count history or calculate a phase.

A selection-only result has no commands, atomic command IDs, ownership writes, or effects.
Core leaves the HLC, device sequence, and last UUID unchanged.
Unused candidate identities remain unused. An empty command identity array and a null timer identity are sufficient.
The complete raw canonical base and retained workspace records remain unchanged.

If the projected session is an exact natural completion, Core records its null-command consumption identity in `lifecycle`.
The choice consumes the automatic presentation, not the explicit Finish obligation.
The existing natural Finish stage still allocates its command and records the non-null command identity.
An explicit choice suppresses the generated automatic break for that natural Finish.
Late applied, ignored, and rejected acknowledgements preserve an explicit choice even after the source timer is cleared or replaced.
Core still validates the current canonical pair before preserving the choice. An explicit flag cannot hide conflicting evidence.

Every opted-in result returns `lifecycle` with the existing schema version and result fields.
Other intents retain their command policy. In particular, terminal Clear still creates its existing Clear command.
When `lifecycle` is absent, the complete legacy request and response contracts stay unchanged.
Legacy PWA same-phase selection remains a no-op, and legacy PWA generation remains unchanged.
Other clients keep their existing selection and generation rules.

## Structural admission

`src/completion_schema/intent.rs` uses the shared `strict_json::shape` validator for the opt-in request.
The schema checks required fields, nullable records, tagged action objects, and raw string enums before typed decoding.
Selection requires all three fields. Null lifecycle and tuple encodings are rejected.
Unknown caller policy flags are rejected rather than ignored.

`fixtures/pwa-selection-contract-v1.json` records the exact presence, nullable, enum, and action metadata.
Native tests compare field names with Serde's decoder metadata.
They also compare required and nullable behavior with the actual decoder and phase variants with the actual enum metadata.
The hosted corpus includes 82 new raw rejection cases. The prior 426 completion representation cases remain unchanged.

## Verification evidence

`fixtures/pwa-selection-public-v1.json` preserves two actual public chooser flows through `issuePhaseSelection`.
The registered current PWA adapter produces the original implicit, generation-zero request.
Its same-phase action returns false because the old planner returns a no-op.
The different-phase action retains implicit selection and generation zero.
Those complete old returns match the saved pre-extension branch executable for the identical raw inputs.
The saved branch rejects the added lifecycle field. It cannot supply the new choice contract.

The source probe adds only context reads, request fields, result writes, and state routing to in-memory copies of PWA code.
It exercises both current and frozen storage code through the registered public action.
It reads `completionState` inside the actual IndexedDB transaction.
No test adapter sets explicitness, increments generation, counts history, or chooses a phase.
Current and frozen storage receive byte-identical planner inputs and return equal complete plans and persisted records.
The public same-phase action now returns true, persists generation 8, and retains focus after natural expiry.
The different-phase action persists the same generation and preserves the chosen long break.
The actual public `getWorkspaceReadModel` preserves both choices before and after database reopen.

The probe verifies two independent instances sharing the database.
Their serial transactions read generations 11 and 12 and commit generations 12 and 13.
Database reopen retains the complete final state.
Four malformed phase, generation, lifecycle, and timer cases abort the real mutation transaction without changing any stored row.
All five retained queues, canonical snapshot, allocation state, ownership, and outgoing metadata remain exact for selection-only actions.

The hosted corpus grows from 2141 to 2342 cases.
All 2141 prior inputs and complete raw envelopes remain unchanged.
`fixtures/pwa-selection-preservation-v1.json` fixes their count and SHA256 digests in the hosted gate.
The gate checks those digests during native capture and every artifact replay, so a changed native result cannot replace an old expectation.
The 201 added cases cover exact public requests, same-phase and different-phase choices, six timer states, generation boundaries,
repeated Skip, choice before and after expiry, natural Finish, restart, and late applied, ignored, and rejected acknowledgements.
Mutation tests reject missing choice dispatches, skipped structural rejections, implicit selections, stale generations,
unwanted commands, allocation changes, canonical clearing, duplicate generated breaks, and acknowledgement rollback.
The artifact-gate and mutation suites pass 170 tests.

The accepted natural-completion checker still passes 146 of 146 checks.
The 12 original natural checker regressions, nine dependency checker inputs, and six dependency residual inputs remain green.
The original official comparison still preserves 1077 envelopes and 20 controls, including its ten documented shape-error corrections.
PWA10, PWA11, PWA12, and lease-release behavior are included in the unchanged 2141-case corpus.

Rust verification uses the actual Rust 1.97.1 compiler first on `PATH`.
Format checking, all-target and all-feature Clippy with denied warnings, and the native test suite pass.
Only these two tests are filtered:

- `wasm_abi_handles_malformed_ranges_and_cleanup_in_a_real_host`
- `c4_release_wasm_rejects_oversized_allocations_without_trapping`

The production size audit reports zero violations and no new exception.
Core has no production size exceptions.
Reported Core entities increase from 773 to 786. Mean cyclomatic complexity changes from 3.63 to 3.61.
Mean cognitive complexity changes from 3.01 to 2.98. Cyclomatic p95 changes from 10 to 9, and cognitive p95 stays 10.
The added parsing, schema, and lifecycle decisions explain the new functions. No metric parser changes are part of this work.
The existing selection decision adds the opt-in choice branch, and Skip adds the PWA context guard.
Those local complexity increases implement the requested policy. The aggregate means and p95 values do not increase.

## Checker commands

These commands run from `pomodorough-core`:

```sh
export PATH="$HOME/.rustup/toolchains/1.97.1-aarch64-apple-darwin/bin:$PATH"
rustc --version
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --all-features --locked -- \
	--skip wasm_abi_handles_malformed_ranges_and_cleanup_in_a_real_host \
	--skip c4_release_wasm_rejects_oversized_allocations_without_trapping
cargo build --locked --example artifact_parity_oracle
node --test scripts/pwa_selection_source_probe.cjs
node scripts/pwa_selection_preservation.mjs
node tests/aggregate_wasm_parity.mjs --native-only
node --experimental-vm-modules --test \
	scripts/test_aggregate_artifact_gate.mjs scripts/test_aggregate_artifact_native.mjs
node scripts/natural_completion_checker_gate.mjs
node scripts/natural_completion_independent_recheck.mjs
node scripts/legacy_dependencies_checker_gate.mjs
node scripts/legacy_dependencies_residual_gate.mjs
node scripts/natural_completion_baseline_gate.mjs
```

`PWA_SELECTION_EVIDENCE_DIR` selects an existing evidence directory.
The default is `/var/folders/r_/_mr22dqn24d31b7460cz8z5m0000gn/T/opencode`.
The preserved inputs are `core-pwa-selection-prior-2141.json`, the pre-extension executable under `core-pwa-selection-baseline`,
and the frozen PWA source under `pwa-core-045/baseline/web`.
Verification never replaces those inputs or `fixtures/pwa-selection-public-v1.json`.
Fresh source receipts are written to `core-pwa-selection-source.json`.

## Ownership and publication status

The Core-side policy blocker is closed. The integrated adapter recipe is in [Persist PWA choices through Core](PWA_SELECTION_ADOPTION.md).
The main task owns the suite backlog update and client adoption.
The PWA source files remain unchanged on disk, so the shipped adapter still needs the documented context plumbing.
No commit, release, nested agent, or local WASM build is part of this verification.
The next official Core artifact must pass the expanded hosted gate before an adapter adopts the extension.
Native evidence and the unchanged old artifact comparison do not verify the future published WASM bytes.
