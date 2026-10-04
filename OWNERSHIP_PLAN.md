# PWA raw ownership plan

`workspace.ownershipPlan.v1` implements CORE-PWA09 for the `pwaStorage` profile.
The CORE-PWA10 extension adds [pagehide lease release](PWA_LEASE_RELEASE.md).
The operation accepts raw workspace records and an existing owner observation.
Core returns the complete owner state and ordered storage writes.

## Request

`fixtures/pwa-ownership-plan-v1.json` contains a complete request and boundary vectors.
The request has these required fields:

- `profile`: exactly `pwaStorage`.
- `action`: `{ "kind": "install" }`, `{ "kind": "renew", "timerId": "..." }`, or `{ "kind": "release" }`.
- `workspace`: the raw `workspace.project.v1` input without `now`. All five retained queues, the canonical base, the head, proof, dependencies, and `displayContext` are present.
- `ownership`: an exact stored owner object or explicit `null`. Omission fails.
- `localDeviceId` and `localTabId`: nonempty raw identities.
- `clock`: `{ "nowMs": ..., "leaseDurationMs": ... }` for install and renew. Release accepts only `{ "nowMs": ... }`.

An owner object requires nonempty `timerId` and `deviceId`. Its legacy `tabId`
and `leaseExpiresAtMs` fields may be absent or null. A present tab must be
nonempty. A present expiry must be an integer from zero through 9007199254740991.
Strings, booleans, arrays, empty objects, unknown owner fields, and duplicate JSON
keys fail closed. Unknown request, action, and clock controls also fail.

`strict_json::shape` validates concrete JSON records, scalar string enums,
required fields, nullable fields, and tagged objects before typed deserialization.
`ownership_plan/schema.rs` declares the ownership request with that shared helper.
Install and release are explicit empty-struct variants. Only their `kind` key is valid.
`workspace.neverSent` is required and must be an object, including when empty.
The helper is applied only to the new ownership boundary. Existing operation
decoders, absent-context behavior, and the shared display selector stay unchanged.

`nowMs` is a nonnegative JavaScript-safe integer representable by the existing
timestamp and projection boundary. `leaseDurationMs` is a positive
JavaScript-safe integer. A requested write rejects an unsafe expiry sum.
A retained owner does not require an expiry addition.
Release does not add a duration and rejects a supplied `leaseDurationMs` field.

The PWA supplies local wall time from `Date.now()` for heartbeat, sync install,
and bootstrap install. This operation does not substitute a trusted server
clock or a monotonic sample. The full display context determines the effective
timer through the existing workspace selector, including accepted CORE-PWA07
admission. Tasks and preferences are validated as workspace records. They do
not independently grant ownership.

For installation, `workspace.base` contains the raw canonical projection fields
from the accepted response. Its retained queues and context come from the
existing Core reconciliation result. For a stale response, the request uses the
actual stored canonical workspace. The host does not send a computed
`owns`, `claimable`, manual grant, or dropped-timer decision.

## Result and writes

Every successful return contains `schemaVersion: 1`, the unchanged raw
`workspace`, `ownership`, `renewed`, `reason`, `ownershipWrites`, and
`effectsAfterCommit: []`. A denied live peer renewal also contains `retryAtMs`.
Other returns omit `retryAtMs`.
Release keeps this result shape, always returns `renewed: false`, and has no retry time.

`ownershipWrites` contains only these instructions, in execution order:

```json
{ "kind": "recordTimerOwner", "timerId": "...", "deviceId": "...", "tabId": "...", "leaseExpiresAtMs": 123 }
```

```json
{ "kind": "removeTimerOwner" }
```

An empty list retains the exact existing record, including absent legacy fields.
The host persists all returned writes inside its existing account-guarded IDB
transaction. Only transaction completion exposes `renewed`. An abort persists
none of the group and runs no after-commit effect.

## Policy

Core projects every retained record through the existing workspace validation
and display selector. Missing-owner installation requires a displayed running
or paused timer. When the raw canonical timer has the same ID and a present
`startedByDeviceId`, that exact device field decides eligibility. Otherwise,
any retained Start for that timer is evidence, including a foreign-device,
covered, claimed, or undisplayed Start. No new delivery proof is created.

An install retains a valid existing owner without renewing it or claiming for
another device. Renewal requires the actual timer and local device to match.
The same tab may renew a live lease. A different tab may renew only when the
lease is absent, null, or expired. Expiry equality permits takeover. A foreign
device cannot take over an existing owner even after expiry.

The original PWA installs a missing owner before checking the heartbeat's
presented timer ID. Core preserves that order. A stale presented ID can install
the actual active timer's owner but returns `renewed: false` with `staleTimer`.
A successful missing-owner renewal returns two identical record instructions:
the installation write and the renewal write. Preserving both writes also
preserves the original abort classification when the first write fails.

Core removes an owner whose timer is absent, explicitly terminal, cleared, or
replaced. The removal returns `staleOwner` and does not claim a replacement in
the same call. This cleanup extends the old renewal method, which could renew
an explicitly finished timer's orphan owner. It also removes an owner after a
real outgoing Start claim is rejected and removed by V3 acceptance. The original
installer retains that orphan. Native, artifact, and original production-source
comparisons identify these requested differences explicitly.
Release does not apply this cleanup. It matches the stored device and tab even
when the owner's timer is absent, terminal, or replaced. It stamps the matching
owner's expiry with `nowMs` and otherwise retains the exact owner, including
expired peers. A missing or null own expiry is stamped as in the original PWA.

A natural deadline does not act as an explicit Finish. Core retains its peer
lease until completion commits, using the existing non-expiring replay to
distinguish explicit terminal state. A missing owner is not newly claimed from
an already expired observed timer.

`timer_ownership.rs` owns missing-owner evidence, peer lease admission, the raw
owner type, and checked record writes. `workspace.completionMutation.v1` reuses
those functions. Its manual bypass, automatic admission, cross-platform owner
rules, generated-break writes, and raw returns retain their prior contracts.

## Evidence

Two new native tests first fail with
`unsupported shared-core operation: workspace.ownershipPlan.v1`.
All 11 native ownership policy tests and seven shape regressions pass. The
expanded artifact corpus has 1077 cases. It includes 277 ownership dispatches:
26 successes and 251 rejections.
Its required semantic counts include 16 ownership branches, three canonical
origin branches, and three actual lease boundary branches. Boundary requests
use the preceding Core return's expiry at minus one, equality, and plus one
millisecond. Four additional branches preserve absent and null legacy owner
fields. Output mutants and skipped-scenario mutants fail the gate.

The shape repair first reproduces seven native failures and four artifact-gate
failures against the preserved pre-repair native oracle. All seven formerly
accepted raw requests now return error-only envelopes. The seven mandatory
reproductions remain in `fixtures/pwa-ownership-shapes-v1.json`. A Rust guard
compares that fixture's 47 field declarations and 11 closed records with the
actual concrete schema. Each applicable field gets populated-array, enum-object,
null, and omission negatives. Closed records and the empty install variant also
get unknown controls with false, true, and null values. The gate requires all
222 shape rejection dispatches. Removing any mandatory reproduction or skipping
the rejection group fails a fixture or runner guard.

`scripts/pwa_ownership_shape_probe.mjs` records all seven complete pre-repair
success envelopes beside the new rejection envelopes.

```sh
node scripts/pwa_ownership_shape_probe.mjs \
	/path/to/preserved-pre-repair/artifact_parity_oracle \
	/path/to/core-pwa09-shapes-red-green.json
```

`scripts/pwa_ownership_source_probe.cjs` executes the actual dirty production PWA
storage and heartbeat methods. The original storage methods remain the oracle.
The compiled comparison module substitutes only Core ownership transport and
ordered write execution at renewal, canonical acceptance, and bootstrap
acceptance. Existing guards, response processing, transaction requests, and
transaction completion remain production code. No JavaScript ownership policy
is copied into the comparison module.

The 24 existing source controls remain unchanged and pass. Two new cases reject
four-element and two-element persisted owner arrays. The original production
method returns false, attempts no owner write, and retains the corrupt array.
The new Core path rejects the raw request, attempts no owner write, and retains
every persisted row through cold reopen. The pre-repair Core path normalizes the
array and renews it, so both new source cases fail before the repair.

The 26 source cases compare complete production returns, all IDB stores,
the exact attempted owner-write sequence, and actual raw transaction
observations beside the Core request. They cover peer expiry, tab changes,
foreign devices, canonical origin, stale response acceptance, Keep Remote,
valid owner retention, signed-out offline renewal, concurrent tabs, owner-write
abort, cold reopen, corrupt owners, unsafe expiry, account replacement, and
heartbeat teardown. The probe uses `fake-indexeddb`, not browser-native IDB.

```sh
CORE_PWA09_EVIDENCE=/path/to/core-pwa09-source-green.json \
	node --test scripts/pwa_ownership_source_probe.cjs
```

`CORE_PWA09_ORACLE` selects an already compiled native oracle. The default is
`target/debug/examples/artifact_parity_oracle`.

`scripts/pwa_ownership_preservation_probe.mjs` compares all 799 accepted PWA07
raw requests and native envelopes byte-for-byte. All remain identical. It also
executes every new ownership request against downloaded official 0.44.0 bytes.
Those bytes reject the missing operation. The new native returns satisfy the
expanded success and rejection corpus.

```sh
node scripts/pwa_ownership_preservation_probe.mjs \
	/path/to/accepted-core-pwa07-parity.json \
	/path/to/downloaded-0.44.0/pomodorough_core.wasm \
	/path/to/core-pwa09-preservation.json
```

Pinned Rust 1.97.1 formatting and warning-denying Clippy pass. The all-target,
all-feature native suite passes 566 tests. The Python gates pass 80 tests.
Exactly these two WASM-building tests are skipped:

- `wasm_abi_handles_malformed_ranges_and_cleanup_in_a_real_host`
- `c4_release_wasm_rejects_oversized_allocations_without_trapping`

The size audit reports zero violations and seven existing suite exceptions.
Core has no production size exception. Core complexity means change from 3.51
to 3.50 cyclomatic and from 2.81 to 2.80 cognitive. Both p95 values remain 9.
The extracted lease and claim decisions replace the previous completion
implementation. New validation, installation, pruning, and write-order decisions
have fixture-backed coverage.

## Remaining adoption scope

The new operation is PWA-only. Android, Apple, and Desktop installation and
renewal remain outside this contract. Their existing completion profiles retain
their previous behavior.

The production PWA adapter is not edited by this stage. Its account issuer,
connection identity, revision checks, captured claim matching, bootstrap gate,
and window teardown remain host responsibilities. The source comparison proves
the bounded Core transport replacement inside fake IDB transactions. It does
not certify browser IDB, the whole PWA migration, or a client release.

The next official Core artifact must pass the expanded hosted gate before any
adapter adopts this operation. Native evidence and downloaded baseline
rejections do not establish parity for those future WASM bytes. This stage
builds no local WASM artifact and makes no release. The counts above describe
the earlier CORE-PWA09 milestone. Current release-policy evidence and the
remaining adapter scope are in [PWA lease release](PWA_LEASE_RELEASE.md).
