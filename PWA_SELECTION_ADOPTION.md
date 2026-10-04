# Persist PWA choices through Core

Use this procedure when adopting [the PWA selection intent contract](PWA_SELECTION_INTENT.md).
The existing durable PWA `completionState` record contains `{selection, lifecycle}`.

1. Start the existing guarded read-write workspace transaction.
2. Read `completionState` with the canonical base, all five retained queues, delivery proof, dependencies, display context, and allocation records.
3. Build the existing raw intent request from those transaction observations.
4. When `completionState` exists, copy its complete `selection` and `lifecycle` into the request.
5. For the first adoption action without that record, retain the legacy selection input and add empty lifecycle arrays, including `finishEvidence`.
6. Set `intent` to the actual user action, either `selectPhase` with the requested phase or `skip`.
7. Call `workspace.intent.v1` synchronously inside the transaction.
8. Persist the complete returned `{selection, lifecycle}` as `completionState` with the existing plan writes.
9. Keep `settings.selectedPhase` equal to the returned selection phase.
10. Commit before executing any returned effect or updating the public action result.

Do not suppress a same-phase action before calling Core.
Do not set the next explicit flag, increment a generation, or calculate Skip in the adapter.
Do not reset explicit choice on Start. Core resets the previous cycle's flag after it admits the new Start command.
Do not copy a projected timer into the canonical base.
If Core rejects the request, abort the transaction and preserve every existing record.

Pass the same durable context through the other natural-completion boundaries:

- For `workspace.readModel.v1`, include the persisted `selection` and `lifecycle`. Use its phase as `selectedPhase`.
- For `workspace.completionMutation.v1`, include the context with the actual presented timer and the existing raw ownership observation.
- For `timer.completionState.v1`, include the context with the actual current canonical pair, prior history, sent commands, and acknowledgements.

Persist any returned selection and lifecycle together.
Reading does not persist completion consumption. Finish and canonical installation retain their existing transaction boundaries.
For an exact natural completion, a null command identity means the presentation was consumed.
A non-null command identity means the explicit Finish obligation was discharged.
Keep both identities when Core returns both.
Persist the complete `finishEvidence` array that Core returns with the lifecycle.
Pass that array through intent, read, Finish, and installation requests after restart.
Do not build original evidence from a consumed command identity.
The [cycle and discharge repair](PWA_CYCLE_REPAIR.md#evidence-backed-finish-discharge) defines the required original record and its trust boundary.

Each independent instance reads the current record inside its own serial transaction.
After restart, read that record again instead of rebuilding selection from a cached phase or resetting the generation.
Copy the raw record into the installed account state so synchronous public reads use the same persisted context.
The exact current and frozen source proof is `scripts/pwa_selection_source_probe.cjs`.
Its in-memory changes show only the required reads, request context, and result writes.

Run the expanded hosted artifact gate against the exact next official WASM bytes before client adoption.
The native verification commands and preserved checker receipts are in [the contract evidence](PWA_SELECTION_INTENT.md#verification-evidence).
