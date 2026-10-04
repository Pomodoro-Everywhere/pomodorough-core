# PWA cycle choices and Finish discharge

This repair addresses the three rejected behaviors in the first [PWA selection intent extension](PWA_SELECTION_INTENT.md).
The extension remains opt-in, and the complete 2141-case preservation baseline remains unchanged.
The current hosted corpus contains 2632 cases.

## Cycle-scoped choice

An admitted Start command in an opted-in PWA intent sets the returned selection's `explicit` field to false.
Core preserves the chosen phase and generation. Start does not increment the choice generation.
Core applies the reset after command admission, so rejected requests and no-op actions do not reset a choice.
The rule follows the admitted Start command rather than an adapter's proposed transition.

A phase or Skip action during that new timer still sets explicit choice and advances the generation.
That current choice survives natural expiry and the explicit Finish stage.
Focus and both break phases use the same cycle rule.
The adapter must not reset the explicit flag itself.

PWA `restart` retains its existing no-op contract. It creates no Clear or Start group and does not change generation or explicitness.
Existing supported restart groups in other profiles keep their existing command and selection policies.
Requests without `lifecycle` keep the original PWA Start selection exactly, including its explicit flag.

## Natural completion belongs to the current session

The missing-history guard now compares both timer identity and phase before consulting an installed completed base.
A completed base timer T without its own history can still deny invented completion evidence for T.
That guard does not deny a different timer U produced by retained Start commands and Core deadline projection.

The read model and natural Finish stage use the same guard.
They retain the exact raw canonical base rather than inserting U or clearing T.
An unconsumed implicit Focus completion selects its break and exposes Finish.
An explicit current choice remains selected. A consumed presentation does not cause another automatic phase change.
Counts still come from the projected history, with one row per timer identity.

## Evidence-backed Finish discharge

A non-null `consumedCompletions.commandId` does not establish discharge by itself.
PWA intent, read, and Finish boundaries require a matching retained Finish command, authoritative canonical Finish provenance,
a matching completed history row, or durable original evidence returned by Core.
Every non-null marker must have evidence, including markers for another timer or phase.
An unsupported marker fails with `consumed Finish lacks raw or durable original evidence` and an error-only envelope.
The failure occurs before an `alreadyConsumed` no-op or a read model can hide Finish.
Changing owner admission order is not the fix.

### Durable schema

The existing lifecycle gains an optional `finishEvidence` array:

```json
{
	"consumedCompletions": [],
	"pendingBreaks": [],
	"finishEvidence": []
}
```

Each evidence record has exactly these required, non-null object fields:

- `command`: the complete original Finish command returned by Core.
- `sourceTimer`: Core's validated natural timer immediately before that Finish.
- `sourceHistory`: the exact corresponding natural history row, before command provenance replacement.

Core appends an evidence record when a natural Finish request supplies the array.
Core returns the array in the same lifecycle record and transaction as the commands and consumption identities.
The adapter persists the complete result. It does not build an evidence record, sign a proof, or assert ownership.
An absent array stays absent in the result, which preserves every old envelope.
A supplied null or object in place of the array is invalid.

Core validates the natural timer and history pair, the original command's wire fields, and their timer, phase, duration, and time relationships.
It replays the original Finish through the shared timer reducer and requires matching completed command provenance.
Each record must match an existing non-null consumption identity. Duplicate evidence command IDs are invalid.
When the same command remains retained, its complete raw object must equal the durable original.
The current session's task, phase, and duration must also match the original evidence.
Corrected terminal times and replacement command provenance do not erase the original discharge.

The schema is PWA-only. Other completion profiles reject the evidence extension.
The array can accompany centralized PWA intent, read, Finish, and canonical installation requests.
When the array is present, installation also validates non-null markers against raw or durable evidence.
The older installation contract still treats legacy lifecycle records as presentation context.
An unsupported non-null legacy marker cannot authorize a later read or Finish discharge.

The evidence-enabled Finish stage also accepts a running timer presented before the deadline when Core's current projection proves natural expiry.
Core compares the presented running fingerprint with the non-expiring current session before allocating Finish.
This supports the actual public PWA action, whose persisted timer can still be running when the read model reports completion.
It does not rewrite that timer into the canonical base.

`fixtures/pwa-finish-evidence-schema-v1.json` declares the required wrapper fields and references the frozen shared command, timer, and history schemas.
Native metadata tests compare those fields with the actual decoder and every additive nested representation path.
The original 426 completion shape paths remain checked against their unchanged fixture.
The new corpus includes structural, duplicate, missing-field, mismatched-identity, raw enum, null, and original-command rejection controls.

### Trust boundary

Core proves consistency and deterministic discharge from the supplied original records. It cannot authenticate the origin of local storage.
The guarded host must read and persist the actual account-bound records and protect their transaction boundary.
An attacker who can replace the command, source timer, source history, consumption records, and canonical context with a coherent invented history
can satisfy deterministic replay. This API contains no cryptographic attestation and makes no claim that it can detect that complete replacement.

The bounded correction rejects an invented command identity when no matching raw command, canonical provenance, history, or durable original exists.
The owner observation does not make that marker valid.
The durable array preserves genuine local Finish evidence after the raw command is retired and remote Finish changes terminal provenance.
Legacy markers whose original evidence has already been lost require recovery from actual records. The adapter must not fabricate replacement evidence.

## Public source proof

`scripts/pwa_cycle_source_probe.cjs` executes the registered public chooser, Start, Restart observation, read model, Finish, and database reopen.
Only in-memory context plumbing is added to the PWA source. Client files remain unchanged on disk.

The six phase scenarios cover Focus, Short Break, and Long Break, with and without a new choice during the timer.
Their starting snapshot contains the actual old completed timer from the preserved natural-completion receipt.
The new Start retains the choice generation but clears its explicit flag.
At deadline, the public read selects the expected next phase, retains one completion for the new timer, and exposes Finish.
Database reopen produces the same result. A choice during the timer remains selected after both expiry and public Finish.

A separate scenario executes two successive public Focus cycles from an idle base.
Their deadline counts are one and two. The second read selects a short break and still exposes Finish.
Both explicit Finish commands preserve the two history identities without creating a third completion.
The complete canonical snapshot remains equal as a decoded record throughout both local cycles.

Four source scenarios insert the unsupported `fabricated-finish` marker with local, foreign, peer-tab, and absent ownership.
The actual storage transaction rejects the marker, public Finish returns false, and the public read throws the evidence error.
All persisted rows remain equal before and after the failed transactions.

`fixtures/pwa-cycle-public-v1.json` preserves the complete public planner requests, envelopes, returns, and persisted states.
Native tests dispatch those exact raw inputs and compare the complete envelopes.
The hosted corpus includes all 32 positive public calls and the 12 actual failed planner calls.
The source capture is create-only and is not refreshed by verification.

`scripts/pwa_cycle_red_green.mjs` compares the exact requests with the rejected pre-repair native executable.
It omits only the new evidence array for the six Start and read comparisons, so the old decoder accepts the same request representation.
All three Start inputs previously retained explicit choice. All three deadline reads previously hid Finish.
Four unchanged fabricated-marker inputs previously returned `alreadyConsumed`, including the foreign-owner input.
The repaired authority resets the old choice, exposes the new natural Finish, or returns an error-only envelope as required.

## Verification and retained diagnostics

Rust 1.97.1 format checking, warning-denying all-target Clippy, and all-target, all-feature native tests pass.
Exactly these two tests remain filtered:

- `wasm_abi_handles_malformed_ranges_and_cleanup_in_a_real_host`
- `c4_release_wasm_rejects_oversized_allocations_without_trapping`

The hosted corpus has 2632 passing native cases. All 2141 prior raw inputs and envelopes remain byte-identical.
The unchanged digest fixture still guards the old inputs and results during every hosted replay.
The artifact-gate and mutation suites pass 180 tests. The new public cycle source suite passes 11 tests.
The earlier selection source suite still passes its seven tests, and all eight dependency source comparisons pass.

PWA11 numeric source tests pass 27 tests, including all 3759 numeric boundary cases.
The legacy dependency independent checker passes 70 cases.
Its residual checker passes 100 checks and 79 restarts, followed by 20 actual reconciliation executions.
The nine exact dependency checker regressions and six residual inputs remain green.
The preserved 1707-case pre-PWA10 comparison remains byte-identical.
The original official comparison preserves 1077 envelopes and 20 controls with its ten documented shape-error corrections.

The unchanged older natural checker now reports 143 of 146 checks.
Its three failing success assertions use `durable-finish` or `prior-finish` markers with no original evidence.
Those assumptions conflict with this repair's explicit rejection requirement.
`scripts/pwa_cycle_independent_gate.mjs` preserves the checker source and original receipt and verifies exactly those three error-only outcomes.
The other 143 checks still pass. No failed assertion is deleted or relabeled as old parity.

The main task corrected the lease-release source probe after the independently accepted CORE-PWA13 host callback repair.
All 47 source cases now pass. The delayed success, failure, and account-mismatch cases require the replacement account's complete
persisted state and in-memory state to remain unchanged, with no effects.
The earlier assertion expected the old defect, a false validated-session flag in the replacement account.
The release includes this intentional test correction in `scripts/pwa_release_source_probe.cjs:240-254`.
Current and migrated release adapters return equal plans, writes, persisted states, and host states.
This correction does not change lease-release Core policy or the host callback.

The production size audit reports zero violations and no new exception. Core has no production size exceptions.
Reported Core entities increase from 786 to 800. Mean cyclomatic complexity changes from 3.61 to 3.63.
Mean cognitive complexity changes from 2.98 to 3.00. Both p95 values remain 9 and 10 respectively.
The added admission branches and original-evidence replay checks explain those small mean increases.
They reject unsupported discharge and retain genuine original provenance across canonical replacement.
No parser or metric formula changes are part of this repair.

## Checker commands

Run from `pomodorough-core` with the actual pinned compiler first on `PATH`:

```sh
export PATH="$HOME/.rustup/toolchains/1.97.1-aarch64-apple-darwin/bin:$PATH"
rustc --version
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --all-features --locked -- \
	--skip wasm_abi_handles_malformed_ranges_and_cleanup_in_a_real_host \
	--skip c4_release_wasm_rejects_oversized_allocations_without_trapping
cargo build --locked --example artifact_parity_oracle
node --test scripts/pwa_cycle_source_probe.cjs scripts/pwa_selection_source_probe.cjs
node scripts/pwa_cycle_red_green.mjs
node scripts/pwa_cycle_independent_gate.mjs
node scripts/pwa_selection_preservation.mjs
node tests/aggregate_wasm_parity.mjs --native-only
node --experimental-vm-modules --test \
	scripts/test_aggregate_artifact_gate.mjs scripts/test_aggregate_artifact_native.mjs
node scripts/legacy_dependencies_checker_gate.mjs
node scripts/legacy_dependencies_residual_gate.mjs
node scripts/legacy_dependencies_independent_recheck.cjs
node scripts/legacy_dependencies_residual_recheck.cjs
node --test scripts/legacy_preferences_numeric_probe.cjs
```

The temporary evidence directory contains `core-pwa-cycle-source.json`, `core-pwa-cycle-red-green.json`,
`core-pwa-cycle-independent.json`, and the before and after complexity reports.
`PWA_SELECTION_EVIDENCE_DIR` selects an existing directory.
The default is `/var/folders/r_/_mr22dqn24d31b7460cz8z5m0000gn/T/opencode`.
The baseline executable is under `core-pwa-cycle-rejected`. The original 2141-case baseline and its digest remain unchanged.

No client or root backlog edits, commits, releases, nested agents, or local WASM builds are part of this work.
The next official artifact still needs the expanded hosted gate before client adoption.
The main task owns that publication, adapter wiring, and backlog status.
