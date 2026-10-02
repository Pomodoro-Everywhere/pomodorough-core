# Safe workspace projection v1

`workspace.project.v1` implements the Core capability for CORE-M01. It chooses
safe optimistic queues from the complete retained workspace and returns the
combined timer, history, task, and preference projection. It is a pure read.
It does not acknowledge operations, change delivery proof, normalize generated
breaks, allocate identities, or change the durable queues.

Client adoption is separate work. The platform adapters observed for this change
still contain local projection policy. This capability alone does not close
CORE-M01 across clients, R43-D04, or R43-A03.

## Request

The operation name carries the schema version. All fields below are required
except `neverSent`. Unknown root, base, local-queue, proof-queue, and head fields
are rejected. Operation objects retain the existing extensible wire contracts.

```json
{
  "base": {
    "canonicalTimer": null,
    "history": [],
    "tasks": [],
    "durationsMs": {
      "focus": 1500000,
      "short_break": 300000,
      "long_break": 900000
    },
    "autoStartBreaks": false,
    "selectedTaskId": null
  },
  "local": {
    "commands": [],
    "taskOperations": [],
    "durationOperations": [],
    "autoStartOperations": [],
    "selectedTaskOperations": []
  },
  "canonicalHead": {"wallMs": 1784548800000, "counter": 10},
  "neverSent": {},
  "timerDependencies": [],
  "now": "2026-07-20T12:00:00Z"
}
```

- `base` is the canonical snapshot, before local optimistic replay. Its six fields
  use the `projection.apply.v2` wire types and validation. Explicit `null` is valid
  for `canonicalTimer` and `selectedTaskId`. Missing fields are invalid. Durations
  contain exactly the three phases, with whole-minute values from 60000 through
  10800000 milliseconds. A selected task must exist in the base tasks. Timer and
  history identities must not conflict. An exact terminal timer and its own
  history row are supported at this workspace boundary. The rules are defined in
  [Workspace terminal state](WORKSPACE_TERMINAL.md).
- `local` contains every retained operation in every domain. Empty arrays are
  required for empty domains. A send batch, cached `projectionPending`, or
  prefiltered queue is not a complete retained queue. Core validates identities
  per domain, so an ID can appear in different domains.
- `canonicalHead` is either an object containing both nonnegative JavaScript-safe
  integer fields or explicit `null`. The head covers every operation represented
  by `base`, including acknowledged operations no longer retained locally.
  `null` means that no trustworthy covering head exists. Omission is invalid.
  The saved head is not compared with the current physical time. It remains a
  valid ordering bound after restart. Do not replace it with the local clock or
  the maximum clock in the retained queues.
- `neverSent` maps the five queue names to arrays of operation IDs with durable
  never-sent evidence. Omission, an omitted queue, or an empty array grants no
  proof. Duplicate IDs, foreign IDs, non-string IDs, and malformed containers are
  errors. The same ID in another domain does not provide proof for this domain.
  There is no `sent` input because this operation processes no server response.
  The adapter must retire proof atomically before any possible delivery.
- `timerDependencies` uses the existing `reconcile.rebase.v2` array schema.
  Each entry has `operationId` and `dependsOnOperationId`. Both IDs must be in
  the complete retained command queue. A child has at most one parent. Cycles,
  self-dependencies, and missing identities are errors. A generated-break edge
  additionally has `generatedBreak: true`, `sourceDayStart`, and `sourceDayEnd`,
  with the existing finish-to-break and day-range checks. After reconciliation,
  the input is its returned `pendingTimerDependencies`, not consumed edges.
- `now` is the explicit RFC 3339 replay time in the same time coordinate system
  as the canonical snapshot and operation timestamps. Core performs timer
  completion at this time. Localized display timestamps are not wire payloads.

Core cannot verify that a host supplied every durable row, that proof reflects
historical delivery, or that the head belongs to the supplied base. Those facts
are persistence and transport preconditions, not inferred domain decisions.

## Result

```json
{
  "projectionPending": {
    "commands": [],
    "taskOperations": [],
    "durationOperations": [],
    "autoStartOperations": [],
    "selectedTaskOperations": []
  },
  "workspace": {
    "canonicalTimer": null,
    "history": [],
    "tasks": [],
    "durationsMs": {
      "focus": 1500000,
      "short_break": 300000,
      "long_break": 900000
    },
    "autoStartBreaks": false,
    "selectedTaskId": null,
    "timerOutcomes": {},
    "winningOperationIds": {
      "tasks": {},
      "durations": {},
      "autoStart": null,
      "selectedTask": null
    }
  }
}
```

`projectionPending` contains each complete domain queue or an empty array. A
domain is eligible only when all its retained operations have never-sent proof
and every `(hlcWallMs, hlcCounter)` strictly exceeds `canonicalHead`. Equal clocks
are insufficient. One stale or uncertain operation suppresses the entire domain,
including newer siblings. A null head suppresses every nonempty domain. Other
domains remain independent.

Eligible arrays preserve input order and the original JSON objects, including
extensions and missing, null, and empty values. Object key order and whitespace
are not byte-preserved. These are projection inputs, not replacement delivery
records. The host keeps the original complete queues for exact retry.

`workspace` has the `projection.apply.v2` domain fields plus optional native
`canonicalTimer.lastIntent.deviceId`. The older operation continues to omit that
field. The existing reducers
own full HLC ordering, task sorting, selected-task cleanup, timer outcomes,
history, and terminal state. Winners and timer outcomes describe only the safe
queues. Suppressed operations do not receive synthetic acknowledgements.

A finish, cancel, or elapsed deadline can produce a terminal `canonicalTimer`
alongside its history entry. A clear can produce `null` while preserving history.
The adapter renders this result without reconstructing a terminal timer from
command-array order or the latest history row. `null` is authoritative. A
canonical base containing only history does not encode which terminal timer was
previously displayed, or whether it was cleared. This API does not invent that
missing state. A persisted canonical terminal timer identifies that display state
without selecting the latest history row. An exact timer and history pair can be
read again without a caller clearing the timer. `workspace` remains a display
result and must not replace the authoritative base after optimistic replay.

## Internal policy and compatibility

`reconciliation/delivery.rs` owns the eligibility decision for both
`workspace.project.v1` and `reconcile.rebase.v2`. The new operation passes an
optional covering head to `Policy::project_queues`. Reconciliation passes its
validated server head. `Policy::projectable` remains the single whole-domain
decision, including immutable-payload checks. Existing v1 and v2 response fields,
acknowledgement processing, raw retry payload restoration, and clock behavior
remain unchanged.

The workspace entrypoint reuses the dependency graph validator without running
acknowledgement-dependent promotion or dropping operations. It validates retained
timer device sequence and explicit dependency order even when their domain is
suppressed. Reducer order remains `(hlcWallMs, hlcCounter, deviceId, id)`.

Every retained payload passes the shared production projection validators and
reducers before a result is returned. The workspace uses a distinct timer input
boundary that accepts validated terminal pairs. Public `projection.apply.v2` and
`timer.reduce.v1` retain their strict overlap contracts. If filtering changes the
queues, Core replays the safe queues against the original base. This composition can
perform two reducer passes. It avoids a second set of operation validators and
prevents suppressed corrupt payloads from escaping validation. When every queue
is eligible, the first result is reused.

Recursive duplicate JSON fields fail before deserialization. Invalid inputs
return the existing Core error envelope through `dispatch_envelope_json` and the
unchanged WASM ABI. Unsupported timer kinds retain the existing per-command
rejected-outcome behavior. This operation adds no exported WASM symbol.

## Evidence for independent checking

`tests/workspace_projection.rs` exercises the production dispatcher. Its baseline
probe first establishes that `projection.apply.v2` replays a retained duration
of 1800000 over a canonical 1500000. Before this implementation, the same test
then failed with `UnsupportedOperation("workspace.project.v1")`. It now verifies
that the new operation keeps 1500000 and suppresses the unsafe duration queue.

The 19 tests cover these cases:

- All five domains with stale, tied, and newer clocks, absent and complete proof,
  partial proof, later mutations, proof retirement, and disk serialization/reopen.
- Fresh operations in every domain after an earlier projection, rather than
  merging commands into a cached safe subset.
- Canonical 25-minute duration preservation after an optimistic 30-minute
  mutation, delivery claim, unrelated mutation, and reopen. Supplying the
  contaminated 30-minute base still yields 30 minutes, proving why D04 needs
  separate canonical persistence.
- Exact safe JSON objects, extensions, omission and null distinctions, duplicate
  fields, incomplete queues, invalid proof, invalid heads, and malformed payloads
  in suppressed domains.
- Reversed array order, reversed immutable clocks, cross-device dependencies,
  missing parents, duplicate edges, cycles, generated-break metadata, and valid
  generated-break projection.
- Finish, cancel, clear, deadline completion, restart, and acceptance of an exact
  persisted terminal pair at the workspace boundary. Conflicting pairs fail.
- Differential agreement with `reconcile.rebase.v2` across every domain and
  proof state, including acknowledged ordering barriers after reconciliation.

`fixtures/workspace-projection-v1.json` is a shared host corpus with five exact
workspace results. Its runner clones `request` and replaces root fields with
`case.overrides`, without a recursive merge. For each queue listed in
`expectedEligibleQueues`, the expected array is the original complete input
array. Every other expected queue is empty. `expectedWorkspace` is compared as
an exact JSON value. The native production-dispatch test consumes this file.

The native checks use the pinned compiler explicitly because this machine's
default `cargo` and `rustc` are Homebrew binaries:

```sh
export PATH="$HOME/.rustup/toolchains/1.97.1-aarch64-apple-darwin/bin:$PATH"
export RUSTC="$HOME/.rustup/toolchains/1.97.1-aarch64-apple-darwin/bin/rustc"
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --all-features --locked -- \
  --skip wasm_abi_handles_malformed_ranges_and_cleanup_in_a_real_host \
  --skip c4_release_wasm_rejects_oversized_allocations_without_trapping
```

Only those two tests build WASM locally. They are excluded by the task's explicit
constraint. Native results do not prove packaged-WASM adoption. Official Core CI
artifact verification, release, and platform adapter tests remain integration
gates. No local WASM artifact is produced for this change.

Verification on 2026-09-26 passed pinned native formatting, warning-denying Clippy,
and 241 tests, with exactly the two named WASM-building tests filtered out. The
19 workspace tests also passed after their final assertion changes. Existing
C01, C02, and C03 regression suites passed in that native run.

The root `audit_code_size.py` reported zero violations and seven existing
documented exceptions. Core has no production size exceptions. The root
`complexity_report.py` measured Core cyclomatic mean 2.73 before and 2.71 after,
and cognitive mean 1.93 before and 1.91 after. Both p95 values remained 7.
The shared policy's optional-head check is necessary to suppress optimism when
the covering head is unknown. No report snapshots were rewritten.

Run those repository-wide checks from the parent directory:

```sh
python3 pomodorough-helpers/scripts/audit_code_size.py
python3 pomodorough-helpers/scripts/complexity_report.py
```

The adapter migration and persistence requirements are in
[Migrate adapters to safe workspace projection](WORKSPACE_PROJECTION_MIGRATION.md).
