# Core 0.46.0 release

This release includes the independently accepted Core implementations for CORE-PWA10 legacy dependencies,
CORE-PWA11 preference precedence, CORE-PWA12 natural completion, cycle-scoped selection with original Finish evidence,
and pagehide lease release. Client adapter adoption remains separate.

## Published scope

- `workspace.legacyDependencyPlan.v1` validates raw dependency provenance and returns metadata or blocked recovery without rewriting queues or saved requests.
- `workspace.legacyPreferences.v1` retains epoch timestamps and HLC `0/0`, explicit false and null preferences, and correctly rounded raw numeric records.
- Natural completion retains history identity, counts, numeric records, explicit current choices, and the explicit Finish obligation.
- Opted-in selection resets the previous explicit flag only after an admitted Start. Durable `finishEvidence` retains the original Finish command and natural timer/history pair.
- The `release` action of `workspace.ownershipPlan.v1` stamps exact `nowMs` only for the stored local device and tab. It grants no claim, renewal, timer mutation, or effect.

## Preservation and older checker assumptions

`fixtures/pwa-selection-preservation-v1.json` fixes the digests of all 2141 prior raw inputs and complete envelopes.
The expanded artifact corpus contains 2632 cases. Native capture and every hosted artifact replay check the unchanged digests.

The original immutable Core 0.45.0 release belongs to commit `c37350cc2f755320388033687eda3649d3971398`.
Its WASM SHA-256 is `845090328b2f44056480c3930e9bb684a3874b8f3cbcbd4253ddd92f67c6f5d6`.
This release does not replace that tag or either asset.
The frozen 1077-envelope comparison preserves every successful return and all 20 numeric/error controls.
Exactly ten already-rejected malformed inputs have new concrete shape-error envelopes, recorded individually in
`fixtures/pwa-completion-shape-errors-v1.json`. The other 1067 envelopes remain byte-identical.

The unchanged older natural checker reports 143 of 146 checks.
Its three success assertions use unsupported `durable-finish` or `prior-finish` consumption markers without original evidence.
`scripts/pwa_cycle_independent_gate.mjs` checks exactly those three error-only outcomes and preserves the checker source and original assertions.
These expected old-oracle disagreements do not waive any repository-native or hosted artifact gate.
Earlier documents retain their historical 146-of-146 milestone counts.

The release also includes the main task's intentional correction to `scripts/pwa_release_source_probe.cjs` after CORE-PWA13.
All 47 cases now require delayed callbacks to preserve replacement-account state with no effects.
The prior assertion expected the old host defect. The host repair itself belongs to the PWA worktree.

## Verification contract

Local verification uses Rust 1.97.1 formatting, all-target/all-feature Clippy with denied warnings, and all-target/all-feature tests.
Only `wasm_abi_handles_malformed_ranges_and_cleanup_in_a_real_host` and
`c4_release_wasm_rejects_oversized_allocations_without_trapping` are filtered locally because they build WASM.
The hosted workflow runs both tests, builds and canonicalizes WASM, validates the ABI, and executes every top-level `tests/*.mjs` host.
The expanded aggregate host requires five complete 2632-case passes, exact native envelopes, preservation digests, and stable memory/free behavior.

The C5 workflow binds the source, workflow, and tag to the same commit. It seals the exact tested WASM and checksum manifest,
attests both files, compares downloaded draft assets with those bytes, and publishes the verified immutable release.
Publication is complete only after the release API, downloaded assets, GitHub digests, tested candidate, and attestations agree.

Local checks also cover the 80 Python tooling tests, all Node tooling/static gates, native artifact corpus,
47 release source cases, legacy numeric and IndexedDB source comparisons, public selection/cycle flows,
the production HTTP natural-completion route, dependency checkers, and preserved baseline comparisons.
Local receipts, generated `target/` files, and `.claude/` session state stay outside the commit.

## Remaining adapters

The PWA must adopt both migration operations, retain blocked recovery, and atomically persist and pass selection,
consumption identities, and complete original Finish evidence through intent, read, Finish, and installation boundaries.
Its pagehide storage adapter must consume the new release action while retaining the accepted account callback fence.
Android dependency adoption and the remaining suite ownership migration still need their own verification.
This Core publication does not certify any client release. Apple continues to resolve the latest Core at build time.
