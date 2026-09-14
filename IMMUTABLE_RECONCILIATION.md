# Immutable reconciliation

## Retarget command

`timer.reduce.v1` accepts `type: "retarget"` in the existing timer-command envelope. The command has a fresh `id`, device sequence, and HLC. It references the original `timerId`; it never replaces the Start payload.

`taskId` is required for retarget. A nonempty string assigns the session to that task. Explicit JSON `null` unassigns it. Omission and an empty string are invalid. The HTTP adapter requires the existing task-identifier syntax and `phase: "focus"`.

Core applies retarget only to the current running or paused focus timer. Core processes automatic completion at the command occurrence first. Missing, inactive, completed, superseded, and break targets are ignored. The task assignment covers the whole session and its eventual history entry, not a time slice. Repeating the current assignment is valid.

Retarget does not change elapsed time, anchors, duration, starting device, or lifecycle intent. It updates session task attribution and last-command identity. Snapshot readers therefore retain the existing lifecycle-intent vocabulary.

Ordering remains `(hlcWallMs, hlcCounter, deviceId, id)`. Device sequence does not replace that order. Allocate a causal child with `hlc.tick.v1` after its parent, then persist both identity and payload before publication.

## Reconciliation v2

`reconcile.rebase.v2` accepts the v1 input plus optional `neverSent`. This additional field contains per-queue ID arrays:

```json
{ "neverSent": { "commands": ["a-local-command-id"] } }
```

Every `neverSent` ID must exist in its corresponding local queue. The existing `local`, `sent`, `response`, and `timerDependencies` contracts still apply.

`sent` identifies only the request covered by this response. It is not delivery history. Missing `neverSent`, missing queue names, and empty ID arrays all grant no mutation permission. Unknown queue names, non-array values, non-string IDs, duplicates, absent local IDs, and IDs also in the current sent queue are rejected.

V2 never rebases any retained operation's HLC or occurrence timestamp, including wholly never-sent queues. Acknowledgement removal cannot authorize moving an older write after a newer write. No transient ordering-barrier metadata is needed. V1 clock rebasing, including its legacy preference tie ordering, is isolated from v2.

For frozen operations, Core also rejects generated-break normalization that changes the payload. Dependency cleanup cannot discard a possibly delivered child without its own acknowledgement. Retained timer commands must preserve device-sequence causality and explicit parent-before-child dependencies in HLC order. Invalid immutable order produces an error, not a replacement ID.

The output contains the existing pending queues for durable retries and an additional `projectionPending` object with the five local-queue names. A domain queue appears in `projectionPending` only if every retained operation has never-sent proof and its `(hlcWallMs, hlcCounter)` is strictly greater than the canonical response's `(serverHlcWallMs, serverHlcCounter)`. A tie is insufficient because the snapshot does not expose each canonical winner's full ordering key. One unsafe operation excludes the entire domain queue from optimistic projection, without deleting it from pending.

The response head must cover all operations represented in the canonical snapshot, including acknowledged operations no longer present in local queues. The server's full-log HLC aggregation supplies this bound. Clients must persist that head with its canonical snapshot and retain both across restart. A peer snapshot without a trustworthy covering head cannot authorize this optimistic projection. Never replace the saved head with the maximum of remaining pending operations.

Projected queues run through the existing authoritative reducers with unchanged full keys `(hlcWallMs, hlcCounter, deviceId, id)`. The returned projected timer, history, tasks, and preferences use these queues. Older and possibly delivered work remains available for exact retry but does not overwrite the newer canonical snapshot. This deliberately trades optimistic visibility of uncertain work for ordering safety.

All exact acknowledgement-set validation remains in force. Confirmed acknowledged operations leave pending queues according to the existing outcomes. A response does not authorize rewriting unacknowledged operations from earlier attempts.

V1 remains unchanged for shipped clients. V1's C19 rebasing behavior is not the immutable-delivery contract. Clients requiring immutable retries must use v2 and must not fall back to v1 when v2 is unavailable.

## Client migration

1. Persist never-sent proof with new operations in the same allocation transaction. Treat existing records without proof as possibly delivered.
2. Before HTTP submission, bootstrap publication, or Iroh record publication, atomically retire that proof and persist the exact outgoing payload. An interrupted send remains possibly delivered.
3. Retain exact operation payloads and identities across retries, restart, account reconciliation, and peer replication. Derive `neverSent` from durable proof, never from absence in the current `sent` batch.
4. Call v2 with the transaction's current queues, current request IDs, canonical response, dependencies, and delivery proof. Persist its result, canonical snapshot, and covering server HLC atomically with acknowledgement consumption. On restart, restore this complete state before calling v2 again.
5. Preserve all returned pending queues for retries. Use `projectionPending`, not all pending queues, for subsequent `projection.apply.v2` calls against that canonical base. Do not add client retarget overlays.
6. Create a new retarget operation for each user attribution change. Persist the selected-task operation separately when the selection also changes. Never rewrite a pending Start.
7. On delivery-policy or causal-order errors, preserve queues and the claimed request. Surface recovery instead of retrying a modified payload under an existing ID.
8. Require retarget support in every peer that receives raw operations. Old snapshot readers and old raw-operation readers have different compatibility requirements. A Core version number alone is not peer capability negotiation.

The Core cannot prove a client's historical delivery claim or perform its persistence transaction. Client adoption and independent review remain required before this change closes the cross-client findings.

Legacy clients may already have rewritten a pending payload under its original ID. V2 cannot reconstruct the original payload from delivery proof alone. Such conflicts require recovery, not another automatic rewrite.

## Lifetime replay

The existing `timer.replay.page.v1` interface replays the complete sorted log in bounded pages with lossless session state. The server retains untouched sessions between pages. Canonical timer and history are projections, not substitutes for that continuation state.

The server clock adapter streams all five operation domains and the request through a 10,000-entry buffer. Each page is reduced by `hlc.head.v1`; its non-incrementing maximum is the next page's continuation. Every original clock is validated, including clocks below the current maximum.

This removes the 10,000-command and aggregate-clock cliff from server lifetime replay without increasing ABI limits or truncating history. It does not make all storage, response sizes, bootstrap collections, or WASM memory unbounded. Those existing limits remain separate constraints.
