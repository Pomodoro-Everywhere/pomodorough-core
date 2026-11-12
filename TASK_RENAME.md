# Task rename and history carryover

Core 0.56.0 adds a `rename` task operation and the pure operation
`task.carryHistory.v1` (R54-C04). Before 0.56.0 a rename was stored as an
upsert of the new identity and a `delete` of the old one. Finished history kept
the old task ID, so the renamed task showed zero runs and minutes.

## The `rename` task operation

Task identity hashes the normalized title, so a rename gives the task a new
ID. The `workspace.intent.v1` `renameTask` plan now emits a `rename` operation
for the old identity in place of the `delete`:

```json
{
  "id": "019f7f65-dd10-7000-8000-000000000002",
  "deviceId": "019f7f65-dd10-7000-8000-00000000000a",
  "occurredAt": "2026-03-08T07:00:00Z",
  "hlcWallMs": 1772953200000,
  "hlcCounter": 0,
  "taskId": "<old task id>",
  "type": "rename",
  "title": "<new title after normalization>"
}
```

- `taskId` is the old identity. `title` is the new title. The new identity is
  `task.identity.v1` of `title`.
- The clock fields are the same as for every task operation.
- Validation: `taskId` is nonempty (`invalid task identity`). `title` is
  already normalized, and its identity differs from `taskId`. Otherwise the
  operation fails closed with `invalid task rename`.
- Task reduction (`task.reduce.v1`, `projection.apply.v2`,
  `workspace.project.v1`): a winning `rename` removes its task exactly like
  `delete`. It never creates the new task. The plan still emits the separate
  upsert of the new identity when that identity does not exist yet.
- The plan keeps its order and size: upsert (when needed), rename, selection
  move (when the old task was selected), retarget (when a running or paused
  focus is assigned to the old task). It uses at most four `commandUuids`.
- Old `delete` operations stay valid. Renames stored before 0.56.0 left no
  edge, so their history stays where it is. Nothing is backfilled.

Every store and codec that limits task operation types to `upsert` and
`delete` must accept `rename` and keep its `title`. The server adopts it
before any client sends one.

## Carryover rule

A rename is an edge `(from = taskId, to = identity(title).id, at = occurredAt)`.
Core applies the edges in reducer order `(hlcWallMs, hlcCounter, deviceId, id)`.
Each edge moves every history row whose `taskId` is `from` and whose start is
at or before `at` to `to`.

- **Start of a row.** The reducer session `startedAt` for the row's `timerId`,
  for every status (`completed`, `cancelled`, `superseded`). A row without a
  session uses its end time: `completedAt` when completed, `endedAt`
  otherwise. The end is never before the start, so a missing session can
  only move fewer rows.
- **Canonical timer.** A terminal canonical timer (`completed`, `cancelled`,
  `superseded`) uses the start of its own history row, so the two always move
  together. Without a row it uses its session start, then `anchorAt`. A
  running or paused timer never moves: its task follows the planner's
  retarget command.
- Only `taskId` changes. Row IDs, timer IDs, command IDs, timestamps, elapsed
  time, status and row order stay the same.

Consequences:

- **Merge.** Renaming onto an existing title skips the upsert. The edge moves
  the old rows to the existing task. A row has one `taskId`, so the merged
  totals are the sum of both row sets and no row counts twice.
- **Edge spaces.** Identity trims only non-printable characters, so `"Write"`
  and `"Write "` are different tasks. Such a rename moves the history.
- **Offline peer.** A device that starts a focus on the old task before the
  rename, without seeing it, gets no retarget. Its row starts before the
  rename, so the full reduction moves it to the new task when it ends.
- **Recreated title.** After `A -> B`, a new task titled `A` has the old
  identity again. Its rows start after the rename and stay on `A`.
- **Chains and cycles.** `A -> B` then `B -> C` moves A's earlier rows to C.
  `A -> B` then `B -> A` moves them back. `Y -> Z` then `X -> Y` moves X's
  rows to Y only.
- **Arrival order.** A full reduction sorts the whole log, so the result does
  not depend on the order operations or rows arrived in.
- **Clock skew.** The bound compares the rename's `occurredAt` with session
  start times from other devices. This is the same trust the reducers already
  place in operation timestamps.
- **Two concurrent renames of one task.** `A -> B` at t1 and `A -> C` at
  t2 > t1, from a device that never saw the first. The task reducer keeps the
  later clock as the winner for A, and both upserts survive. The carry does
  not look at the winner: it applies both edges in reducer order, so A rows
  that start at or before t1 move to B, rows that start in (t1, t2] move to C,
  and later rows stay on A. The log cannot tell this from `A -> B`, recreate
  A, `A -> C`, where the time split is the right answer.

## Where Core applies it

### Local projection

`projection.apply.v2`, `workspace.project.v1`, `workspace.readModel.v1`,
`workspace.intent.v1` and reconciliation apply the edges of the replayed local
task queue to the replayed history and canonical timer, with the replay's own
sessions. `workspace.project.v1` replays the task queue only when every
retained operation has never-sent proof and a clock above the canonical head.
A replayed edge is therefore never already applied to the base, and every base
edge sorts before it. The read-model schemaVersion stays 2.

Reconciliation (`reconcile.rebase.v1`, `v2` and `v3`) carries the pending
edges in its projected pending result in the same way, over the server's
carried base history. v1 restamps pending clocks above the canonical head, and
v2 projects a queue only when every operation is never-sent and above the
head, so the same boundary argument holds. v3 then installs its workspace
projection, which already carried.

A base row that has no session and ends after a rename moves only in the
server's full reduction. This covers a rename that was sent and acknowledged,
and also a never-sent rename whose `occurredAt` falls inside the row: base rows
fall back to their end time, and v1 restamps pending clocks without changing
`occurredAt`. Every local preview (`projection.apply.v2`, `workspace.*` and
`reconcile.rebase.*`) shows the row on the old task until the server returns
the carried history.

### Full reductions: `task.carryHistory.v1`

Input:

```json
{
  "canonicalTimer": null,
  "history": [],
  "sessions": [],
  "taskOperations": []
}
```

- `canonicalTimer`, `history` and `sessions` are the `timer.reduce.v1` (or
  replay page) output for the complete command log. `canonicalTimer` must be
  present and may be `null`.
- `taskOperations` is the complete task operation log in the `task.reduce.v1`
  wire form.
- Each session has exactly the fields `timer.reduce.v1` writes. An unknown
  session field fails closed, here and in `timer.replay.page.v1` input.
- Unknown top-level fields, missing fields, invalid task operations, duplicate
  operation IDs, duplicate session timer IDs, invalid timestamps and a terminal
  timer that disagrees with its row fail closed.

Output: `{"canonicalTimer": ..., "history": [...]}` with the rule applied.

## Uncarried input

Carrying is not idempotent. With `B -> C` at t1 and `A -> B` at t2, an A row
that started before t1 belongs on B after one pass. A second pass moves it on
to C, which is wrong. Every input Core carries must be uncarried:

- `task.carryHistory.v1`: `history` and `canonicalTimer` are the raw reducer
  output for the complete command log, and `sessions` come from the same
  reduction.
- Server: the session cache (`timer_sessions`) stays uncarried. The server
  carries only the history and canonical timer it returns, and never feeds
  carried rows back as reducer seed state or replay page state.
- Clients: the base history passed to `projection.apply.v2`,
  `workspace.project.v1`, `workspace.readModel.v1` and `workspace.intent.v1` is
  the server's canonical base as received. Projection applies only the pending
  local edges on top. A client never stores its own projection output as base
  while the renames in it are still in the local queue.

Core fails closed where it can tell. Reducer sessions keep the raw task, so
`task.carryHistory.v1` rejects a row or terminal timer whose `taskId` differs
from its own session's `taskId`:
`history for timer <id> disagrees with its session taskId; pass uncarried reducer output`.
Core cannot detect two cases, which rely on the rule above:

- a row without a session (its end time stands in for the start);
- carried rows passed to a reducer as seed history, because a seeded session
  copies the row's `taskId`.

## Validating one operation

To reject a bad operation before storing it, call `task.reduce.v1` with
`{"operations": [candidate]}`. It applies the same checks as the full log
(`invalid task rename`, `invalid task identity`, clock and type checks), so a
stored operation can never block later reductions. Callers must not
reimplement title normalization.
