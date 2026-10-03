# Terminal-aware reconciliation v3

`reconcile.rebase.v3` is the opt-in CORE-PWA05 contract. It accepts the server's
completed timer and matching history row without changing the canonical input.
`reconcile.rebase.v1`, `reconcile.rebase.v2`, `timer.reduce.v1`, and
`projection.apply.v2` retain their strict overlap rejection.

## Dispatch and input

Native callers use `dispatch_json` or `dispatch_envelope_json` with operation
`reconcile.rebase.v3`. WASM callers use the existing `pomodorough_dispatch` export.
There is no new allocation, free, or operation-specific WASM export.

The request has the v2 fields and queue schemas:

- `local`, or its `pending` alias, contains the five retained operation queues.
  Both aliases in one request are invalid.
- `sent` identifies the exact request covered by this response.
- `response` is the complete raw HTTP response, including the five ACK arrays,
  `revision`, `canonicalTimer`, `history`, `tasks`, `durationsMs`,
  `autoStartBreaks`, `selectedTaskId`, `serverTime`, `serverHlcWallMs`, and
  `serverHlcCounter`. `canonicalTimer` is required and nullable.
- Optional `neverSent` contains the existing per-queue proof arrays.
- Optional `timerDependencies` contains the existing dependency records.

V3 rejects unknown top-level controls and duplicate JSON keys at every depth.
Response and wire-operation extensions remain extensible. Numeric bounds,
canonical tasks, preference validation, ACK sets, and clock validation use the
existing reconciliation boundaries.

## Terminal validation and projection

The raw timer and history pass the same `timer::workspace` validation as
`workspace.project.v1`. Exact completed, cancelled, and superseded pairs are
accepted. Task attribution, phase, status, duration, terminal time, history
identity, and command provenance must agree according to
[workspace terminal semantics](WORKSPACE_TERMINAL.md). C02 cross-session
identity collisions and active overlaps remain invalid.

Every local command is validated against the original pair before ACK removal
or display filtering. A malformed or conflicting suppressed command cannot hide
behind delivery proof. Core seeds the validated terminal session internally.
The caller does not clear `canonicalTimer`, delete history, or add a Clear command.

Retained commands with the canonical intent's ID must match its type, timer ID,
and occurrence instant even when `history` is empty. This check precedes ACK
consumption and suppression in both v3 and the shared workspace boundary.
Valid standalone terminals still synthesize history. Finish retains the reducer's
existing ignored task, phase, duration, and observed-elapsed command semantics.

ACK validation, dependency promotion, rejection cascades, generated-break
normalization, frozen-operation checks, and whole-domain projection eligibility
share the v2 implementation. Retained clocks and occurrence timestamps never
move. A frozen child cannot be rewritten or discarded without its own ACK.
Only the existing generated-break policy can normalize proven never-sent work.
Unchanged fields retain their original omissions, nulls, and extensions.

## Output schema

V3 returns every v2 field:

- `revision`.
- `pending`, `pendingTaskOperations`, `pendingDurationOperations`,
  `pendingAutoStartOperations`, and `pendingSelectedTaskOperations`.
- `pendingTimerDependencies`, `promotedTimerOperationIds`,
  `droppedTimerOperationIds`, and `droppedTimerIds`.
- `baseTimer`, `baseHistory`, `baseTasks`, `baseDurationsMs`,
  `baseAutoStartBreaks`, and `baseSelectedTaskId`.
- `projectionPending`, with all five queue names.
- `timer`, `history`, `tasks`, `durationsMs`, `autoStartBreaks`, and
  `selectedTaskId`.

V3 also returns these required fields:

- `schemaVersion: 3`.
- `canonicalResponse`, the original parsed response value, including extensions.
  Its six canonical fields are also returned verbatim in the `base*` fields.
- `workspace`, the complete safe production projection with `canonicalTimer`,
  `history`, `tasks`, `durationsMs`, `autoStartBreaks`, `selectedTaskId`,
  `timerOutcomes`, and `winningOperationIds`.

The top-level projected fields equal the corresponding `workspace` fields.
`workspace.canonicalTimer.lastIntent.deviceId` retains valid native origin
metadata when the source supplies it. Raw base evidence remains separate from
the reducer's normalized output. JSON whitespace and object-key order are not
preserved. The operation does not return or replace an outgoing claim or a
client's logical clock.

## Completion-state composition

`timer.completionState.v1` accepts the original `canonicalResponse.canonicalTimer`
and `canonicalResponse.history`. It does not require overlap normalization.
Its existing ACK input is `{commandId, outcome}`, so the adapter selects those
two fields from validated response ACKs. It does not pass HTTP `reason` fields
into the closed completion-state ACK schema.

Pending IDs come from all returned durable queues, not only `projectionPending`.
Sendable IDs come from the existing dependency-aware batch plan. PWA installation
uses its existing `pwaRejectedFinish` compatibility and matching `sentContext`.
Applied Finish preserves the phase advanced by the originating mutation.
Desktop installation derives its next phase from the original canonical pair.

## Evidence and qualification

`fixtures/reconciliation-terminal-v3.json` preserves the exact request and
response text from the independent Go HTTP 200 Finish checker. Native tests
also cover task-attributed generated Finish and Start, exact two-step ACK
promotion and rejection, partial proof, retained extensions, and the shared
positive and conflicting terminal matrix.

The retained-intent fixture adds six missing-history conflicts: changed time,
type, and timer identity, each in acknowledged and frozen-retained form.
Both contracts run the same six controls for exact provenance, unknown extensions,
and ignored Finish fields. The red/green probe records complete inputs and returns
from the pre-fix and fixed pinned native oracles.

The aggregate artifact catalog includes v3 success, conflict, malformed JSON,
duplicate keys, unknown controls, unsafe integers, and overflow cases. Stateful
scenarios consume actual previous returns for Finish ACK, Start reject or apply,
and completion-state installation. Semantic mutants test retained evidence and
dependency results. The official artifact runner compares complete envelopes
and raw JSON against pinned native output.

Native-only verification runs `node tests/aggregate_wasm_parity.mjs --native-only`.
Static verification runs
`node --experimental-vm-modules --test scripts/test_aggregate_artifact_gate.mjs`.
These commands do not build WASM. Official v0.43.0 at
`ceaf4fca40ad435178c0db5c9549373a84b23fef` does not export v3.
New official CI bytes and adapter adoption remain required for client acceptance.
CORE-PWA04 display context and CORE-PWA06 legacy claim recovery remain separate
contract gaps owned by the main backlog.
