# PWA pagehide lease release

CORE-PWA10 adds `action: { "kind": "release" }` to `workspace.ownershipPlan.v1`.
The native implementation is ready for independent checking. Official WASM
verification, publication, and PWA adoption remain open.

## Interface and policy

Release uses the existing raw ownership request. The action is an empty tagged
object. The clock is exactly `{ "nowMs": ... }`. It has no lease duration or
presented timer ID because the original pagehide caller supplies neither.
Unknown controls, positional records, enum objects, duplicate keys, missing
required fields, empty identities, and invalid clocks produce error-only envelopes.

Core validates the full workspace and returns it unchanged. All five queues,
the canonical base, history, proof, head, dependencies, and display context stay
intact. Release uses the accepted raw-number decoder only for this new action
and serializes its successful envelope once. This preserves values such as
`90.49999999999999` in retained extension fields. Install and renew retain their
previous decoding and envelopes.

A successful result has the existing seven fields: `schemaVersion`, `workspace`,
`ownership`, `renewed`, `reason`, `ownershipWrites`, and `effectsAfterCommit`.
`renewed` is always false, effects are empty, and `retryAtMs` is absent.

- A missing owner returns `missingOwner` and no writes. Release never claims ownership.
- A different device or tab returns `notOwner` and no writes, even after expiry.
- An absent or null stored tab cannot match the required nonempty local tab.
- The same stored device and tab return an empty reason and one `recordTimerOwner` instruction.
- That instruction preserves the owner identity and sets `leaseExpiresAtMs` to the exact `nowMs` integer.

The original PWA writes `nowMs` even when the own lease is expired, absent, null,
or already equal to `nowMs`. Core preserves that behavior, including the attempted
write at equality. Stamping an expired record can increase its old expiry, but
the result is expired immediately. This is not a renewal or a new claim.

Release does not prune a stale owner or require its timer to be active. A paused,
finished, cancelled, cleared, naturally expired, or replaced timer does not change
the identity test. Core never emits a timer command, completion mutation, or
ownership removal from this action.

`nowMs` must be a nonnegative JavaScript-safe integer that the existing Chrono
timestamp boundary can represent. Core does not round the clock or add a duration.
Malformed stored owners are rejected rather than normalized or released. This is
an explicit defensive difference from the old JavaScript method, which could
spread a matching malformed record into a new owner.

## Source evidence

`scripts/pwa_release_source_adapter.cjs` compiles complete production storage
modules from server commit `50c86a2`, the accepted frozen v0.45 candidate, and the
current dirty checkout. It asserts byte equality of their release function bodies
and records each source SHA-256. The migrated module replaces only the release
function with Core transport and ordered ownership-write execution. The existing
account guard, workspace capture, transaction completion, and public return remain
production code.

`scripts/pwa_release_source_probe.cjs` compares the complete public method return,
every attempted owner write, all IDB stores, and cold reopen. The public storage
method still resolves to `undefined`. Evidence represents that value with
`returnWasUndefined: true` because JSON cannot encode `undefined`.

Each Core call retains the actual transaction observations beside its exact JSON
request. Assertions compare all persisted queues and metadata with the decoded
request. No expected result substitutes for an observation or returned value.
The probes use `fake-indexeddb`; they do not certify browser-native IDB.

The official v0.45 artifact already supports `workspace.ownershipPlan.v1`, but
rejects the new action with `invalid shared-core input: action.kind must be a JSON
string enum`. The evidence records the actual error, request, and artifact hash.
This rejection is the baseline-red capability result. The valid legacy release
behavior is a parity control, not a behavior regression.

The actual pagehide listener is also exercised with delayed success, failure,
and account-mismatch callbacks after account B replaces account A. The source
comparison leaves host behavior unchanged. B's persisted stores remain exact.
Delayed success and ordinary failure leave B's memory unchanged. The existing
account-mismatch catch calls `quarantineAccountMismatch()` without checking the
issuing context and still quarantines B's current memory. That host-only defect
requires a separate account-scoped callback repair. Core release policy does not
claim to fix it, and no client code changes in this stage.

## Shared and hosted gates

`fixtures/pwa-ownership-release-v1.json` supplies 21 successful cases and 31
negative cases to native tests and the aggregate artifact corpus. Native tests
compare complete envelopes and separately check the raw numeric token through
the public envelope. A concrete-schema guard checks the release request against
the existing manifest and verifies both empty action variants.

The artifact gate requires 21 release policy hits, six actual expiry-boundary
hits, three release-to-peer-renewal hits, and five terminal-state hits. Stateful
cases use the preceding Core return's owner and expiry. It also requires all
31 release negatives and 207 concrete-field representation rejections.
Output mutants and runner mutants fail if policy, raw rejection dispatch, or
stateful release coverage disappears. Existing hosted metadata, exact artifact
hash, envelope parity, and ABI checks still run through the same aggregate gate.

All 1,867 envelopes captured before this release extension remain byte-identical,
including accepted dependency, CORE-PWA11, and CORE-PWA12 work. Comparison with
1,077 official v0.45 envelopes and 20 numeric/error controls keeps only the ten
previously documented CORE-PWA12 structural error differences.

## Native verification on 2026-10-04

Pinned Rust 1.97.1 formatting and warning-denying Clippy pass. The all-target,
all-feature native suite passes 628 tests with exactly the two exclusions listed
below. The host and gate suites pass 181 test executions, and the Python gates
pass 80 tests. The aggregate corpus contains 2,141 native envelopes.

The production size audit has zero violations and seven existing suite
exceptions. Core has no production size exception. Core grows from 770 to 773
reported entities. Cyclomatic mean stays 3.63, cognitive mean stays 3.01, and
both p95 values stay 10. The three new entities declare the action-specific
schema and release decision. Existing functions gain the fixture-backed action,
clock, and numeric-preservation branches. Other projects' production
fingerprints stay unchanged.

All 47 production source probes pass. They include populated five-domain queues,
absent and malformed owner records, bootstrap metadata, actual expiry boundaries,
release followed by peer renewal, owner-write abort, guarded account replacement,
and the three delayed pagehide callbacks. Complete returns and persisted records
are compared with the original, frozen, and current production methods where
their input is valid. Negative cases record the intentional Core rejection.

## Adapter handoff for the independent checker

After official artifact verification and publication, replace the body of
`releaseTimerOwnership` in the PWA storage adapter with the bounded transport
used by `scripts/pwa_release_source_adapter.cjs`.

1. Keep the pagehide event listener, local device/tab capture, and `Date.now()` acquisition in the host.
2. Keep the existing issuing account/database guard and its `allowBootstrap: true` behavior.
3. Read the raw owner, canonical inputs, and all five queues in that same guarded transaction.
4. Call `workspace.ownershipPlan.v1` with `{ "kind": "release" }` and `{ "nowMs": input.nowMs }`.
5. Check the issuing context again before writes. Execute only Core's ordered `ownershipWrites` through `workspaceTransaction.writeOwnership`.
6. Resolve the existing public method only after transaction completion. Preserve its `undefined` return and atomic abort behavior.
7. Remove the adapter's device/tab admission branch and spread-based lease construction once official-artifact parity passes.

The existing `ownershipPlan` adapter helper always sends `leaseDurationMs`.
Its release call must use the action-specific clock above. It must not supply
`owns`, `claimable`, a projected timer, or a normalized owner. A missing owner
record maps to explicit `null`; a stored false value or tuple remains malformed.
The after-commit error callback needs its own issuing-context check before any
quarantine or UI effect. That check stays in the host.

The independent checker can reproduce this stage with these commands from
`pomodorough-core`. `CORE_PAGEHIDE_FROZEN_STORAGE` can select another preserved
copy of the accepted production storage module.

```sh
rustup run 1.97.1 cargo build --locked --example artifact_parity_oracle
CORE_PAGEHIDE_EVIDENCE=/path/to/pagehide-source.json \
	node --test scripts/pwa_release_source_probe.cjs
node --experimental-vm-modules --test scripts/test_aggregate_artifact_gate.mjs \
	scripts/test_aggregate_artifact_native.mjs scripts/test_aggregate_runner_mutants.mjs \
	scripts/test_pwa_ownership_shapes.mjs
rustup run 1.97.1 cargo fmt --all -- --check
rustup run 1.97.1 cargo clippy --all-targets --all-features --locked -- -D warnings
rustup run 1.97.1 cargo test --all-targets --all-features --locked -- \
	--skip wasm_abi_handles_malformed_ranges_and_cleanup_in_a_real_host \
	--skip c4_release_wasm_rejects_oversized_allocations_without_trapping
```

Only those two local WASM-building tests are excluded. This stage builds no
local WASM, changes no client source, and makes no commit or release.
