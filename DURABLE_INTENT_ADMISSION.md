# Durable intent admission

This reference describes the CORE-M03 admission repair in `workspace.intent.v1`
and `workspace.completionMutation.v1`. It supersedes the Desktop-delete-only
admission exception described in `WORKSPACE_INTENT.md`.

## Admission and display

`src/workspace_intent/admission.rs` validates every newly allocated group against
the complete retained ledger. The replay includes possibly delivered operations.
It does not change their payloads, clocks, dependencies, or delivery proof.

Every command must receive an `applied` reducer outcome in that admission replay.
Every task or preference operation must win its domain and produce its requested
value. A selected-task winner whose target task is absent fails admission. One
ignored, losing, or malformed member rejects the whole plan before persistence.
The existing identity, clock, ownership, requested-timer, and causal checks remain
in force. A stale presented timer retains the entrypoint's no-op result without
allocation.

The returned `projection` still uses the conservative delivery policy. A claimed
old row suppresses its whole display domain. A null `canonicalHead` does not prove
display eligibility. Neither condition alone rejects a valid fresh operation.
The fresh operation receives proof only for its own identity.

Desktop and PWA storage selection also accept a task established by a claimed
Upsert in the retained ledger. The task remains absent from safe display. The
known-task cache alone does not establish an active task for selection.

The planner preserves profile decisions before allocation. Android's coordinator
uses its complete Room queues. Desktop's reader uses retained queues when no head
exists. Apple's centralized presentation keeps its non-expiring safe view. PWA's
null-head local path can use retained work. This is not a request to apply all old
work to the display.

## Result fields

The schema version remains `1`. Immutable operation payloads do not gain fields.

Task and preference plans retain `groupOutcomes`, with one ordered array per queue.
Timer intents and every completion stage add `commandOutcomes`. Empty and no-op
command results contain `[]`. Each entry has this shape:

```json
{"id":"operation-id","outcome":"queued"}
```

`outcome` is the serialized Rust `DisplayOutcome` enum:

- `applied`: the accepted operation also applies in the returned safe projection.
- `queued`: the operation passes retained-ledger admission but does not apply its
  requested value in the safe projection.

A selected-task operation can win its display-domain ID while its task remains
hidden by the task-domain barrier. That result is `queued`, not `applied`.
The top-level `outcome: "planned"` means the adapter has an atomic durable commit
to make. It does not mean the display changed. Existing alarms, ownership writes,
selection updates, and completion records remain explicit commit effects.

Completion planning obtains exact Finish provenance from the admission replay.
It returns the safe projection separately. Generated-break eligibility and
preferences retain their existing entrypoint decisions. Expiry observation and
deferred source barriers are unchanged.

## Duration retention

Coalescing removes only same-phase work with never-sent proof, no outgoing claim,
and, for PWA, the same tab owner. Claimed rows and other tabs' rows remain beside
the fresh operation. Outgoing claim identities that also carry never-sent proof
fail validation. The planner does not overwrite a claimed duration to make room
for a new value.

## Pending-only Start

`tests/durable_intent_admission.rs` states the supported cases explicitly.
Android can Pause a timer that exists only in a possibly delivered pending Start.
Desktop can do so through its null-head reader. The PWA null-head storage sequence
also accepts the subsequent Pause. Their safe Core projection remains empty, and
the new Pause is `queued`.

Apple does not invent a visible timer after the only Start loses proof. Its Pause
entrypoint remains a no-op. The Desktop head-covered path also retains that
precondition. PWA's storage and owner-display paths differ when a persisted
`projectionPending` record exists. The pending-only head-covered PWA case needs
that raw display context during adapter migration. The probe does not claim that
all PWA display configurations admit that action.

## Source evidence

`scripts/durable_admission_evidence.py` runs the native checks and four source
probes. It retains complete requests, raw observations, production returns, proof,
outgoing payloads, and complete Core results under `target/durable-admission-evidence/`.

- `durable_admission_desktop_probe.py` invokes the real SQLite Store and Qt task
  controller. Every case captures a real outgoing claim, loses its response,
  closes SQLite, reopens it, and then queues another action. It compares complete
  returned operations, allocation, raw rows, physical times, proof, and outgoing
  payloads. Add-and-select and deletion compare the complete controller outcome
  and each production queue return.
- `durable_admission_pwa_probe.cjs` runs unchanged repositories and transactions
  against fake IndexedDB with the existing bundled Core. It invokes
  `retireProofAndPersistOutgoing`, closes the database, reopens it, and compares
  complete mutation returns and persisted metadata. Add-and-select also invokes
  the original action controller. No new local WASM artifact is built.
- `durable_admission_android_probe.py` compiles the original coordinator, wire
  models, allocation, dispatchers, presentation, and Room entity conversions.
  It executes the original `retireNeverSent` method with test storage callbacks,
  serializes the raw rows and outgoing queues, and reopens the saved bytes.
  Every returned plan field is checked. This is method extraction, not a Room
  database or Android runtime test.
- `durable_admission_apple_probe.py` compiles the original mutation controller,
  command builder, wire types, proof policy, and sync claim methods. Test state
  supplies persistence, clocks, entropy, and the native dispatcher. Claim proof
  is retired through the original method, state is saved and reopened, and
  complete transition fields are checked. This is method extraction, not an
  Apple application persistence test.

`complete-red-green.json` replays the exact captured requests through the saved
pre-fix native executable and the fixed executable. It compares complete fixed
outputs with the captured receipts. `desktop-source-red.log` reproduces the
original claimed selection/retarget failure. `pwa-source-red.log` reproduces the
task admission failure. Both original production paths accept the durable write
before baseline Core reports `did not win safe projection`.

The final run passes 107 complete output receipts: 30 SQLite cases, 29 IndexedDB
cases, 28 Android extraction cases, and 20 Apple extraction cases. Baseline Core
rejects 88 of those requests. The Android pending-only Start cases also expose
baseline no-op results where the original coordinator queues Pause.
The PWA live-monotonic case keeps the original clock across database reopen. A
wall-clock jump still produces a 1,000 ms Pause observation from the original
clock methods. This is not a claim that monotonic continuity survives a browser
process restart.

The native regressions also cover null-head commands, claimed durations,
completion history suppression, pending-only Start, grouped add-and-select,
losing clocks, forged proof, wrong ownership, malformed causal metadata, and
ignored retargets. Existing scoped Desktop deletion regressions remain covered.

## Compatibility required before client migration

The independent checker owns disposition of these adapter differences:

- Desktop's phase-unique SQLite duration table replaces a claimed row. Android's
  original coordinator and phase-keyed Room duration entity do the same. Both
  adapters need storage that retains the complete ledger while a claim is live.
  The probes record the difference instead of weakening Core retention.
- Apple's original `synchronized` method still requires every member to win the
  safe projection. Its auto-start toggle has a separate queue-only path.
  The probe records its other safe-winner refusals explicitly. Migration must
  consume Core admission and queued outcomes at the commit boundary.
- Native null-head readers can display retained work while Core deliberately
  returns a conservative projection. The probes compare that difference without
  substituting the native display for Core's safe output.
- PWA's persisted `projectionPending` display context must be mapped for the
  pending-only head-covered case. A null head is not a substitute for that raw
  context.

No client source, backlog, release metadata, or WASM asset changes are part of
this repair. The main backlog remains checker-owned.

## Verification

The runner uses Cargo and rustc from the pinned Rust `1.97.1` toolchain. Its native
suite passes 516 tests. Its command has exactly these two explicit skips:

```text
wasm_abi_handles_malformed_ranges_and_cleanup_in_a_real_host
c4_release_wasm_rejects_oversized_allocations_without_trapping
```

Formatting and warning-denying all-target, all-feature locked Clippy run in the
same environment. The size audit reports zero violations and the same seven
documented exceptions elsewhere in the suite. Core has no production size
exception. Core complexity changes from means 3.58 cyclomatic and 2.88 cognitive
to 3.56 and 2.85 after the shared admission extraction and live-monotonic guard.
Both p95 values remain 9. The extra entrypoint clock branch is required by the
fixture-backed PWA wall-jump case.
