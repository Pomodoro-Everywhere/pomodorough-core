# Outgoing batch planner v1

`dispatch_json("sync.batchPlan.v1", input)` plans operation IDs for the five centralized-sync domains. The existing native and WASM dispatch entry points expose the same operation name. This change builds and tests only native code.

The planner consumes scheduling descriptors, not wire payloads. It does not create operations, rewrite clocks, serialize requests, modify delivery evidence, acknowledge operations, or promote dependencies. Payload validation remains with the existing operation validators and adapters.

## New request schema

```json
{
  "kind": "new",
  "mode": "sync",
  "limits": {"perDomain": 256, "total": 512},
  "nextDomain": "commands",
  "queues": {
    "commands": [{"id":"start-1","deviceId":"device-a","hlcWallMs":1784548801000,"hlcCounter":0,"deviceSequence":1}],
    "taskOperations": [],
    "durationOperations": [],
    "autoStartOperations": [],
    "selectedTaskOperations": []
  },
  "timerDependencies": []
}
```

Every field shown is required. Every queue is required, including empty queues. Unknown fields, unknown domain names, duplicate JSON keys, null queues, and malformed numbers fail with a Core error. Descriptors accept only the fields shown for commands. Other domains omit `deviceSequence`, or supply null. A null sequence on commands is invalid.

IDs and device IDs are nonempty strings. IDs are unique within each domain, but the same ID in different domains is valid. Each domain accepts at most 10,000 descriptors. The existing 16 MiB dispatch input cap also applies. HLC components are integers in `0..=9007199254740991`. Timer wall clocks and sequences must be positive. Zero clocks for non-timer legacy operations are scheduling keys, not permission to bypass payload validation.

`mode` is `sync`, `merge`, `replace_remote`, or `keep_remote`. The last three name bootstrap strategies. Positive integer limits can lower endpoint caps but cannot exceed them:

| Mode | Maximum per domain | Maximum total |
| --- | ---: | ---: |
| `sync` | 256 | 512 |
| Bootstrap strategies | 4096 | 8192 |

`keep_remote` requires empty input queues. The planner never derives which local operations a keep-remote decision discards.

`nextDomain` is one of the five queue names. New workspaces start with `commands`. This is a persisted round-robin cursor, not a clock or operation ID.

Each dependency has exactly two string fields:

```json
{"operationId":"generated-start","dependsOnOperationId":"finish"}
```

Both endpoints must exist in the complete command queue. A child has at most one parent. Parent replay key must be strictly less than child replay key, which also rejects self-edges and cycles. Generated-break metadata stays in the reconciliation input and durable dependency record. For scheduling, every unresolved dependency is an acknowledgement barrier, including generated Finish to Start and held retargets.

## Ordering and progress

Within each domain, the planner sorts by `(hlcWallMs, hlcCounter, deviceId, id)`. Strings use Rust string order, equivalent to UTF-8 byte order. Clients use the returned order rather than sorting again. Timer keys must preserve increasing per-device `deviceSequence`. Duplicate sequences or reversed clocks fail closed.

The command queue contributes only its sorted prefix before the first unresolved dependency child. The parent can be selected, but selecting it never releases the child in that batch. Later commands also wait, even when they have no explicit dependency. This preserves timer replay order and matches the web client's prefix barrier. Desktop and Android currently filter individual held commands instead; their integration must supply the complete queue.

Selection visits domains in fixed order: commands, tasks, durations, auto-start, selected-task, then commands again. Each visit takes at most one eligible operation. Full, empty, and held domains consume no budget. Selection stops at the total budget or after a full pass with no eligible operation. `nextDomain` advances to the domain after the last selection.

With a positive budget and the returned cursor persisted, every continuously eligible domain gets a turn within five successful single-slot batches. Finite queues drain without starvation once acknowledgements remove selected entries and reconciliation resolves barriers. New operations whose keys follow existing operations do not starve existing entries. Infinite backdated arrivals, an unresolved dependency, failed delivery, and repeated rejection without reconciliation cannot have an unconditional progress guarantee.

Bootstrap `merge` and `replace_remote` are atomic. Any per-domain or total overflow returns `oversized` with no selected IDs. Any dependency barrier returns `blocked_dependency` with no selected IDs. Overflow takes precedence when both apply. The planner never turns one atomic replacement into several requests or omits held state from a replacement. No partial bootstrap-merge protocol is introduced.

## Result schema

```json
{
  "status": "planned",
  "selected": {
    "commands": ["start-1"],
    "taskOperations": [],
    "durationOperations": [],
    "autoStartOperations": [],
    "selectedTaskOperations": []
  },
  "nextDomain": "taskOperations",
  "heldTimerOperationId": null,
  "counts": {
    "commands": 1,
    "taskOperations": 0,
    "durationOperations": 0,
    "autoStartOperations": 0,
    "selectedTaskOperations": 0
  },
  "total": 1
}
```

`counts` and `total` describe the complete input, not the selected batch. `heldTimerOperationId` identifies the first timer barrier, even when other domains make progress. `planned` can select zero IDs for an empty request. Normal sync returns `blocked_dependency` only when no ID can be selected and a timer barrier exists. New blocked plans leave `nextDomain` unchanged.

## Saved request schema and recovery

```json
{
  "kind": "saved",
  "mode": "sync",
  "limits": {"perDomain":256,"total":512},
  "queues": {
    "commands": ["start-1"],
    "taskOperations": [],
    "durationOperations": [],
    "autoStartOperations": [],
    "selectedTaskOperations": []
  }
}
```

The ID arrays describe the exact saved claim, not the current queue. Saved requests do not accept descriptors, dependencies, or a cursor. The same ID and count validation applies.

A fitting claim returns `replay_saved`, preserving each array's exact order. `nextDomain` and `heldTimerOperationId` are null. This result authorizes only replay of the original durable request under existing account and request-identity checks. It does not validate its payload or let an adapter reconstruct it from mutable local state.

An oversized claim returns `oversized_saved` with empty selected arrays. Its original payload, request ID, revisions, owner, ordering, and delivery evidence remain durable. No send or replacement claim follows this result. Missing never-sent proof means possibly delivered. A timeout, restart, or locally detected overflow does not prove non-delivery.

Automatic recovery for possibly delivered oversized claims remains a protocol decision. Safe recovery needs authoritative evidence of non-application or an exact-request lookup/replay mechanism that can resolve the original claim. The current planner cannot provide that evidence. It never silently reidentifies, truncates, splits, or resends a modified saved request.

## Adapter transaction contract

1. Validate account ownership and bootstrap gates. If a durable outgoing claim exists, use `kind: saved` before considering new work.
2. For new work, capture all five queues, all unresolved timer dependencies, and the cursor in one consistent transaction. Extract descriptors from exact existing records. Supply the owning device ID for preference wire records that omit it.
3. Call the planner without pre-slicing or pre-filtering queues. A missing dependency parent requires reconciliation or repair, not deletion of the edge to make planning succeed.
4. Resolve every selected ID back to its exact record in that transaction. Preserve payloads, timestamps, extension fields, and missing/null/empty distinctions through the existing validated wire encoder. Planner descriptors never replace payloads.
5. Atomically persist the complete immutable outgoing request and returned cursor, and retire never-sent proof only for selected IDs. A failed transaction consumes neither IDs nor cursor. Multiple callers need the existing single-claim lock or compare-and-set.
6. Retry a fitting saved claim exactly. Do not advance its cursor again. Commit validated acknowledgements, queue removal, dependency promotion/drop, and claim clearance atomically through existing reconciliation.
7. Use `pendingTimerDependencies` from `reconcile.rebase.v2` for the next plan. Removing a dependency requires reconciliation evidence, not a local observation that its parent was selected.

Bootstrap blockers need a user-visible recovery state. Loosening the global timer-prefix barrier, splitting bootstrap merge, and recovering possibly delivered oversized claims require separate protocol decisions. None is inferred here.

## Integration locations and fixtures

The inspected web paths are `server/web/sync-core.js` `buildSyncBatch` and `sendableTimerCommands`, plus `app-sync.js` outgoing persistence. Desktop claims and bootstrap requests originate in `desktop/src/pomodorough/storage_sync.py`. Android uses `TimerSyncConstruction.kt` and `CentralizedSyncCoordinator.kt` eligibility filtering. Client changes are outside this patch.

`fixtures/batch-plan-v1.json` defines language-independent count-vector expansion, exact prefix results, all 32 domain-presence masks, and timer barrier cases. `tests/r43_c03_batch_plan.rs` executes those vectors, saved-claim restarts, draining, tiny-budget fairness, and invalid-input cases. `tests/r43_c03_batch_reconciliation.rs` crosses the planner with real retarget and generated-break reconciliation.
