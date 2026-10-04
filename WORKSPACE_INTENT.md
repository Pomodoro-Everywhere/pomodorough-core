# Workspace intent planner v1 (CORE-M03 first slice)

The later [durable intent admission reference](DURABLE_INTENT_ADMISSION.md)
supersedes the scoped Desktop-delete admission exception below. It defines shared
retained-ledger admission, typed queued outcomes, source evidence, and remaining
adapter compatibility work.

`workspace.intent.v1` plans a manual workspace mutation without changing storage.
Core decides admission, timer command fields and order, phase selection, local
generation, safe projection, command grouping, ownership writes, and post-commit
effects. It composes `workspace.project.v1`, `hlc.tick.v1`, and the existing
completion phase policy. Clients provide retained canonical and local records,
physical observations, clock readings, and unused identity material. No client
supplies a proposed command, an eligibility flag, or a computed next phase.

## Input

```json
{
  "compatibility": "appleWorkspace",
  "replicationMode": "centralized",
  "intent": {"kind": "start"},
  "workspace": {
    "base": {
      "canonicalTimer": null, "history": [], "tasks": [],
      "durationsMs": {"focus": 60000, "short_break": 120000, "long_break": 180000},
      "autoStartBreaks": false, "selectedTaskId": null
    },
    "local": {
      "commands": [], "taskOperations": [], "durationOperations": [],
      "autoStartOperations": [], "selectedTaskOperations": []
    },
    "canonicalHead": {"wallMs": 1784548800000, "counter": 0},
    "neverSent": {}, "timerDependencies": []
  },
  "selection": {"phase": "focus", "generation": "5", "explicit": false},
  "allocation": {
    "deviceId": "device-local", "deviceSequence": 7,
    "hlc": {"wallMs": 1784548800000, "counter": 0}, "lastUuid": null
  },
  "observation": {"canonicalAnchorAt": null, "commandTimes": {}},
  "clock": {
    "occurredAt": "2026-07-20T12:00:10Z",
    "physicalNow": "2026-07-20T12:00:10Z",
    "observedAt": "2026-07-20T12:00:10Z"
  },
  "identities": {
    "commandUuids": ["019f7f65-dd10-7000-8000-000000000001"],
    "timerUuid": "12345678-1234-4234-8234-123456789012"
  },
  "calendarIntervals": [
    {"start": "2026-07-20T00:00:00Z", "end": "2026-07-21T00:00:00Z"}
  ]
}
```

`compatibility` is one of `appleWorkspace`, `androidCoordinator`,
`desktopStorage`, `desktopTerminal`, `pwaStorage`. `replicationMode` is
`centralized` or `iroh`; it affects Apple's pre-mutation timer replay and
which Start ownership records Core returns.
`intent.kind` accepts `start`, `pause`, `resume`, `cancel`, `cancelAndClear`,
`clear`, `restart`, `selectPhase`, or `skip`. `selectPhase` additionally needs
`phase` (`focus`, `short_break`, `long_break`). `skip` is Apple-only unless the
[PWA selection lifecycle extension](PWA_SELECTION_INTENT.md) is present. `restart`
models Desktop's atomic clear/start action; other profiles have no matching
  entrypoint, so it returns a no-op there. A caller invokes `start` for an
  individual Start button. Finish, automatic break, and reconciliation intents
  remain separate stages. Task, retarget, duration, and auto-start planning are
  described in the second slice below.

`workspace` is the complete raw input to `workspace.project.v1` except `now`.
All five complete retained queues and every canonical field are required. Keep
the canonical base independent of the display projection. The covering head,
never-sent proof, and timer dependencies have that operation's exact contract.
Core rejects duplicate JSON keys and unknown request, intent, clock, identity,
selection, allocation, or observation fields. Existing wire operation objects
retain their extensible schemas, omitted fields, and original values. Object
key order and JSON whitespace are not preserved.

`selection` is durable local state, separate from the canonical base. Its
`generation` is a decimal string, so a 64-bit generation survives JavaScript
bridges. `explicit` represents Apple's explicit phase choice or an opted-in PWA choice. For desktop
compatibility, generation is a nonnegative unbounded decimal string. Apple
wraps `Int64.max` to `0`; Android wraps signed 64-bit to `Int64.min`; PWA keeps
its generation when the lifecycle extension is absent. With the extension,
Core advances the PWA generation and records explicit choice for each user phase or Skip action.

`allocation` contains persisted device identity, last device sequence, HLC,
and optional last UUIDv7. `identities.commandUuids` supplies ordered, unused,
lowercase UUIDv7 values; supply up to two. Supply `timerUuid` as a lowercase
UUIDv4 when Start may occur. Extra valid identities are not consumed. Core
derives the next HLC using the persisted local HLC and `clock.occurredAt`,
checks the 5-minute clock-skew limit, and advances the local sequence for each
command. Each candidate UUIDv7 must encode the greater of the resulting HLC
wall time and the previous UUIDv7 timestamp. The local HLC must already
include any previously merged remote head;
Core does not silently overwrite it from a retained queue or a covering head.
UUID generation and trusted-clock measurement remain platform inputs. Core
does not generate random values. PWA commands use HLC wall time as
`occurredAt`; other profiles use the supplied trusted occurrence time.

`clock.occurredAt` is the trusted wire time. `clock.physicalNow` is the timer
projection time. PWA projection uses the new command's HLC wall time after a
mutation. For a live running timer, Core derives the pre-mutation projection
time from elapsed observation, so a wall jump alone cannot expire that timer.
`clock.observedAt` is the elapsed-time observation: physical for Apple,
Android, and Desktop, trusted for PWA. It cannot be later than `physicalNow`.
All three are RFC 3339. For Apple,
`observation.canonicalAnchorAt` and `observation.commandTimes` map trusted
canonical and retained command dates to physical time without editing their
wire payloads for Apple, Android, and Desktop local replay. PWA replays wire
time; Android can persist the returned command observation separately. Include
only known retained command IDs. Platform clients measure physical time and,
where applicable, trusted time and uptime before planning.

For PWA live timer observations, add both `clock.monotonicNowMs` and
`clock.continuityId`. The first is the raw `performance.now()` reading in
milliseconds, including fractional values. The second is a nonempty
identifier for the lifetime of the monotonic clock. A browser context can
generate a new identifier at startup. An Android adapter can use its boot ID
if it later adopts this observation format. Neither field is a wire timer
timestamp. A monotonic reading requires a continuity ID. When the reading is
absent, the ID may be omitted or retained. Core uses `clock.observedAt` for
that read and keeps the saved anchor while the timer stays running. An explicit
new continuity ID clears it. This slice accepts monotonic observations only
for `pwaStorage`.

`observation.monotonicAnchor` is optional. When present, it has this shape:

```json
{
  "timerId": "existing-timer",
  "anchorAt": "2026-07-20T12:00:00Z",
  "elapsedAtAnchorMs": 5000,
  "sampledTrustedNowMs": 1784548810000,
  "sampledMonotonicMs": 100,
  "continuityId": "browser-session-1"
}
```

Pass back the anchor that Core returned, without calculating elapsed time.
Core retains it while the timer ID, wire anchor, anchored elapsed, and clock
continuity identity match and monotonic time has not moved backwards. Core
computes elapsed and effective projection time from the sample and raw
monotonic progress. A new or missing anchor samples elapsed from
`clock.observedAt`. An absent monotonic reading uses wall elapsed for that
read without replacing the anchor. This permits the next valid reading to
continue from the original sample. Pause and terminal transitions clear the
anchor. Resume and Start seed an anchor for the new running timer when a
monotonic reading is available. Core rejects malformed anchor fields even
when the saved timer key is stale or a reading is absent. A stale timer key,
new continuity identity, or backwards monotonic reading causes a wall-time
fallback and a new sample when a reading is available. Core never edits the
canonical base or pending wire timestamps for this calculation.

`calendarIntervals` supplies local-day bounds used by Apple Skip and Android
cancel-after-completion. Core counts completed focus history rows, including
counts 0 through 12 and the 3/7/11 long-break Skip positions. The platform
resolves calendar time zones and daylight-saving boundaries; Core chooses the
phase. A missing interval for a required reference time fails.

`requestedTimer` is optional for most single-command actions. It is required
for Desktop Terminal `cancel`, non-idle Desktop/PWA `cancelAndClear`, and
Desktop `restart`. Pass the timer presented to the user. Desktop checks timer
identity, status, phase, duration, anchor, elapsed, task, and last command;
PWA checks timer identity and phase. Mismatch with the fresh projected timer
returns `staleTimer` without consuming identities. Desktop Terminal `cancel`
performs the same fingerprint check as its atomic `queue_cancel_and_clear`.

Desktop `restart` also accepts a presented completed/cancelled/superseded timer
when the projected canonical timer is null, the complete retained command
queue is empty, and the canonical history contains that timer ID. This matches
`storage.py`'s retained terminal fallback. Core uses the presented timer only
as the restart target. It does not insert it into the canonical base, which
must stay null beside its history row. An unrelated ID or any retained command
does not activate that fallback. The resulting Clear/Start pair still must win
Core projection as an atomic group.

## Output and atomic commit

The result has exactly these top-level fields:

| Field | Type and meaning |
| --- | --- |
| `schemaVersion` | Integer `1` |
| `outcome` | `planned` or `noop` |
| `reason` | Empty string on `planned`; `staleTimer`, `ineligible`, `invalidAction`, `invalidTransition`, or `selectionUnchangedOrActive` on `noop` |
| `workspace` | Complete updated `workspace.project.v1` input, without `now` |
| `selection` | Updated `{phase, generation, explicit}` |
| `allocation` | Updated `{deviceId, deviceSequence, hlc:{wallMs,counter}, lastUuid}` |
| `observation` | Updated `{canonicalAnchorAt, commandTimes, monotonicAnchor?}` |
| `timerObservation` | `null` without a timer; otherwise `{timerId, elapsedMs, remainingMs, deadlineAt}`. Live elapsed and remaining milliseconds retain fractional precision. `deadlineAt` is the effective RFC 3339 deadline for a running timer, and `null` for other statuses. Use `remainingMs` for a live countdown; do not compare `deadlineAt` to an untrusted wall clock. |
| `commands` | New wire commands in append order |
| `atomicCommandIds` | Ordered new command IDs; commit every member together |
| `ownershipWrites` | Ordered `recordStart`, `recordLocalStart`, or `removeTimerOwner` records |
| `projection` | Existing `workspace.project.v1` workspace result |
| `effectsAfterCommit` | Ordered sync, alarm, and alert effects |

`workspace` contains the unchanged canonical base, original retained payloads
in their input order, and any new timer commands, never-sent proofs, and
inherited timer dependencies. `commands` lists only new commands in append
order. `atomicCommandIds` lists their IDs in the same order. The returned
`projection` comes from the existing Core workspace projector. Apple, Android,
and Desktop physical timestamps apply only in temporary replay input. Use the
result for display, not as
the next canonical base. In centralized mode, `ownershipWrites` contains
`recordStart` entries with `timerId`, `deviceId`, and `startCommandId`. Iroh
never returns a centralized `recordStart`: Apple and Android return
`recordLocalStart` for their local owner state, while Desktop returns no Start
ownership write. PWA cancel/clear returns `removeTimerOwner`.
`effectsAfterCommit` contains ordered `launchSync`, alarm actions
(`scheduleAlarm`, `pauseAlarm`, `resumeAlarm`, `cancelAlarm`), and possibly
`clearCompletionAlert`. Alarm schedule/resume include `phase` and `durationMs`.
Local-only phase changes have no commands or post-commit effects. A no-op
returns `outcome: "noop"`, an explanatory `reason`, unchanged durable fields,
and empty command/effect/ownership arrays.

Integration order:

1. Acquire the platform's workspace/account lock and its mutation transaction.
   Read a fresh canonical base and **all** durable queues, proof, dependencies,
   selection, allocation, and physical-time metadata. Preserve original queued
   payloads for exact retry. Respect bootstrap/account admission outside Core.
2. Supply clock observations, local calendar intervals, the optional presented
   timer, and unused UUID candidates. Call `workspace.intent.v1` inside the
   transaction. Do not persist an identity reservation or mutate durable
   records before checking `outcome`. Core owns command construction and
   admission.
3. On `planned`, persist the returned workspace local queues, proof,
   dependencies, selection, allocation, observation, and ownership writes as
   one atomic unit. Never persist the returned projection as the canonical
   base. For cancel/clear or restart, every `atomicCommandIds` member must
   commit together and retain its listed order. On an error, roll back all
   writes and leave identity allocation unchanged. On `noop`, write nothing.
4. After commit succeeds, execute `effectsAfterCommit` in order. Schedule
   sync only after durable proof and queues commit. Retire never-sent proof
   atomically before any possible network delivery, per `workspace.project.v1`.
    Recompute projection after reconciliation from the new canonical base;
    never reuse a cached optimistic base.

For a live read with no mutation, a `noop` still returns `timerObservation`
and an updated `observation.monotonicAnchor`. Keep that returned anchor for
the next observation, even though a `noop` writes no command or durable
mutation. The anchor is local metadata; it never becomes a wire command.

The operation does not release ownership leases, execute alarms, send commands,
acknowledge operations, or persist anything itself. Platform adapters own those
side effects and the atomicity boundary. Existing immutable retry payloads,
canonical timestamps, device sequences, and identity prefixes remain intact.

## Intent compatibility

The source locations below explain versioned entrypoint differences. They do
not assert that clients already call this new Core operation.

- Apple `SynchronizedWorkspaceMutationController` makes Cancel a cancel/clear
  pair, clears a terminal timer on explicit phase selection, permits phase
  selection even during an active timer, and increments generation even when
  selecting the current phase. Start replaces a terminal timer without Clear;
  it resets `explicit`. `TimerSessionController.makeCommand` uses `command-`
  IDs, local dates for physical replay, and `timer-` IDs for new timers.
- Android `TimerMutationCoordinator` accepts Start even with an existing timer
  and permits Resume from `superseded`. `TimerRepository.selectPhase` rejects
  active timers and unchanged selections. Cancel can be one command;
  `cancelAndClearTimer` appends both commands for an active timer or only Clear
  for a terminal completed/cancelled timer. A completed history row can advance
  phase after cancellation. Its generation uses signed 64-bit wrap.
- Desktop `terminal.py` accepts Start only from idle and rejects phase selection
  during an active timer. Its Cancel checks the presented timer fingerprint,
  then queues cancel/clear together. `storage.py`
  has a separately callable Start operation that delegates admission to Core's
  completion request planner, plus an atomic terminal clear/start restart and
  the exact retained-history restart fallback described above. Its
  direct phase setter increments `selectedPhaseVersion` on every selection,
  even when the value stays equal. Desktop generation is unbounded decimal.
- PWA `app-storage.js` builds individual commands with bare UUID IDs and HLC
  wall time. `sync-storage.js` atomically cancels/clears active timers, or
  clears a completed/cancelled timer, after checking the presented timer ID
  and phase. `app-view.js` phase buttons update persisted settings, including
  when the selected phase is unchanged, without a generation counter. Its
  Clear button only dismisses a sound; no timer Clear command is issued from
  that button. This profile's direct `clear` intent describes a wire-level
  command, not that button.

## Task and preference slice (CORE-M03 second slice)

This slice adds intent variants to **the same** `workspace.intent.v1` operation:
`upsertTask` (`title`), `addAndSelectTask` (`title`), `deleteTask`
(`taskId`), `selectTask` (`taskId`, string or null), `setDuration`
(`phase`, integer `minutes`), `changeDuration` (`phase`, integer `delta`),
and `setAutoStart` (`enabled`). `changeDuration` is Android-only, matching its
delta button; `setDuration` is Apple, Desktop Storage, and PWA. Desktop Terminal
uses Desktop Storage for these entrypoints. `addAndSelectTask` is unavailable
for Apple, whose `planTask` and `planSelectedTask` are distinct entrypoints.
Task identity and NFC/title constraints come from `task.identity.v1` directly.
Duplicate-title add selects the existing projected task without an upsert.
Upsert alone always queues a task operation, including Apple `planTask` calls
for a title already present. Delete uses an active task ID, or a validated
Desktop cache identity with the `knownTasks` input below, and emits a delete,
and emits a null selected-task operation if currently selected, except PWA,
where task reduction deselects without a second operation. Desktop Storage
also retargets an active focus timer on delete+deselect. Select emits one
selected-task operation and, for an active focus timer, one immutable retarget
command. Break/terminal selections emit no retarget. Android permits a same-
selection retarget when timer assignment differs; Desktop Storage queues both
records even on same selection. Apple and PWA same-selection requests are no-ops.
Android `addAndSelectTask` routes an NFC-equivalent existing task through
`selectTask` even during an active timer. On active focus, it emits selection
and retarget, or only retarget when the selection already matches but the timer
assignment differs. On an active break, it selects without retarget. A truly
new task remains blocked during an active timer by `issueTaskOperation`.

For these variants, supply account and durability fields, read inside the
platform's workspace/account transaction:

```json
{
  "ownership": {"expectedOwnerId": "account-a", "ownerId": "account-a"},
  "durability": {
    "outgoingDurationOperationIds": [], "localTabId": "tab-a"
  },
  "localDurationsMs": {"focus": 60000, "short_break": 120000, "long_break": 180000}
}
```

Both owner IDs may be null for unowned local work. Core rejects mismatches and
empty non-null owner IDs. `outgoingDurationOperationIds` contains **all**
retained duration IDs in durable outgoing batches and any in-flight send
snapshot, even if never-sent proof has not yet been retired. Unknown or duplicate
IDs fail. `localTabId` is required for PWA duration writes and identifies its
duration owner. A same-phase duration may be removed only when its ID has
never-sent proof, is absent from outgoing/in-flight metadata, and (for PWA)
belongs to that tab. Any other supersession fails closed. Never infer this
metadata from current projection or from memory-only UI state. No Core API can
verify whether a caller omitted an outgoing snapshot; adapters must read it
under the same lock and recheck ownership at commit. Retirement and append
commit atomically. The `workspace.project.v1` input itself is unchanged.
Supply `localDurationsMs` only for Apple `setDuration` and Android
`changeDuration`. It is the persisted settings map, not a synthetic projection.
Core validates all three phases and compares Apple requests against this map;
Android applies its delta and no-op check to this map, even when the current
projection differs. A resulting duration equal to projected duration is still
a new operation when it differs from local settings. Same-phase replacement
requires never-sent proof and no outgoing claim. Other variants reject this
field.

Apple clamps minutes to 1–180 and clears a terminal timer before the duration
operation. Android clamps the sum of current local minutes and signed delta
to 1–180 and ignores changes during active timers. Desktop Storage and PWA
reject out-of-range absolute minutes; active duration edits do not modify an
already running timer. PWA `ownerId` in the new duration operation is the
supplied tab ID. Apple compares auto-start against the last queued intent, even
when it cannot project; an unprojectable new Apple auto-start operation is
durable with group outcome `queued`. Desktop Delete separately validates durable
admission as described below. Other new operations must win safe Core projection
or the whole plan fails.
Desktop Storage queues duration and auto-start operations even when requested
values match projected settings; its storage entrypoints do not deduplicate.
Without `knownTasks`, Delete retains the projected-task-only admission rule.
Desktop can additionally supply its raw known-task cache to delete a task that
is absent from both the canonical base and the current projection.

New-intent output retains the first slice's `workspace`, `selection`,
`allocation`, `observation`, `commands`, `atomicCommandIds`, `projection`,
`timerObservation`, `ownershipWrites`, and `effectsAfterCommit`. It adds
`operations` (five arrays of **new** operations), `atomicOperationIds` (the
corresponding IDs, ordered per queue), and `groupOutcomes` (each new ID with
`applied` or `queued` for Apple auto-start or admitted Desktop Delete).
`retiredDurationOperationIds` lists
same-phase never-sent rows removed by the plan. Commit **all** operations, proof,
clock, sequence, observation and any retired duration IDs together. Launch sync
only after commit. Returned projection is display state, never canonical base.
Apple terminal-duration Clear emits `cancelAlarm` after `launchSync`. Other
task and preference mutations emit `launchSync` only. Android's existing
retarget path reschedules its alarm during repository installation; a future
adapter must retain that platform side effect.
Existing retained queue objects and their extension fields remain untouched.
`durableOperations` carries the same new IDs, dates, clocks and values in
profile-specific local serializer shape. Apple and Desktop omit `deviceId`
from commands, tasks and durations. Android also omits `deviceId` from
selected-task operations. PWA retains `deviceId` in every queue. `operations`
and `workspace.local` keep Core's projection-ready wire shape; persist the new
`durableOperations` while preserving the exact original retained payloads in
the store. Adapters restore local device identity when reading saved payloads
back into Core, as they already do for `workspace.project.v1`.
The original timer intents still reject account, durability, and local-duration
mutation metadata. They retain their previous output shape. Desktop timer
intents also accept the optional raw `knownTasks` input described below.

Entrypoint differences that need explicit adapter handling: Desktop/PWA
add-and-select historically perform two storage calls, and PWA selected-task
then retarget can persist only the selection if retarget fails. This planner
returns one atomic group instead; adoption must use an atomic platform commit,
not an automatic replacement of the current multi-transaction UI path. Android
and Desktop currently replace same-phase pending duration rows regardless of
possible delivery. The planner rejects possibly-sent rows; adoption must
preserve them and surface the blocked mutation rather than deleting them.
PWA's memory-only `inFlightDurationOperationIds` must be included in the raw
durability union. No client has been switched to this slice.

The new `fixtures/workspace-intent-mutations-v1.json` drives active focus,
paused focus, breaks, terminal duration, and four entrypoint profiles through
the production dispatcher. `tests/workspace_intent_mutations.rs` has 19 native
cases covering all five raw queues, immutable payloads, exact generated IDs,
timestamps, clock/sequence, task identity and duplicate Unicode titles,
atomic select+retarget and delete+deselect, profile serializer shapes, blocked
outgoing/frozen/foreign-tab duration supersession, raw ownership, and Apple
auto-start shadowing. The two Android divergence tests failed before repair:
existing active NFC-equivalent add returned no selection, and local 60-second
duration with projected 120 seconds returned no operation for delta +1.
Production-source probes execute extracted Apple Swift duration clamp, Android
Kotlin duration delta and `addTask` route, Desktop Python retarget elapsed, and
PWA JavaScript retarget builder (5/8/7/2/1 cases). These probes do
not prove platform transaction boundaries or completeness of outgoing claims.
Pinned Rust 1.97.1 native format, Clippy with warnings denied, and all-target
tests passed with only the two named local-WASM-building tests filtered.
Code-size audit reports zero violations and seven existing exceptions; no
Core production size exception was added. Core structural cyclomatic mean is
3.39 and cognitive mean 2.71; new profile-specific admission, proof checks,
and atomic five-domain result validation explain the increase over the
previous Core slice. Independent checker still needs client-side atomic
durability mapping and canonical outgoing/in-flight metadata review before
any adapter adoption. No local WASM build or client change was made.

### Desktop known-task deletion

`knownTasks` is an optional top-level input to the existing `workspace.intent.v1`
operation. Only `desktopStorage` and `desktopTerminal` accept the field.
Other profiles reject its presence, including `null` and an empty array.
Desktop Terminal still routes task mutations through `desktopStorage`.
Its timer intents, including retained-history Restart, accept and validate
the cache without using it to choose commands or tasks.

```json
{
  "compatibility": "desktopStorage",
  "intent": {"kind": "deleteTask", "taskId": "aaf83054-24b2-8c0e-901f-a974147bfe82"},
  "knownTasks": [
    {"id": "aaf83054-24b2-8c0e-901f-a974147bfe82", "title": "Café"}
  ]
}
```

This fragment supplements the complete existing request, including ownership
and durability metadata. `knownTasks` must be an array of objects with string
`id` and `title` fields. Core validates every title with `task.identity.v1`'s
existing normalization and UTF-8 limit, then checks that the resulting ID
matches the record's ID. Raw decomposed titles and nonprintable edge characters
are accepted when their normalized identity matches, as in Desktop Store's
`_normalized_task_identity`. Extra record fields remain allowed, matching the
source record schema. Duplicate identities, including NFC-equivalent duplicate
titles, fail before planning. Malformed unrelated records also fail. Core
does not discard corrupt records or collapse duplicate identities into a map.

When this array is present, Desktop Delete admits a currently projected task
or a matching cache record. A nonempty requested ID absent from both returns
`noop` with `unchangedOrIneligible`. The no-op leaves durable state and allocation
unchanged and returns empty operation, group, ownership, and effect arrays.
An empty requested ID remains invalid. Omitting the field preserves the
existing projected-task-only admission rule, including its unknown-task error.

Cache membership grants only deletion admission. It cannot select a deleted
task, suppress an upsert for a cache-only title, or resurrect a historical task.
Core never inserts cache records into `workspace.base.tasks` or the projected
tasks. Delete has the existing output schema. A selected current task also
queues explicit deselection and an immutable retarget for a running or paused
focus timer. Break and terminal timers do not retarget. A cache-only task with
timer or history attribution but no current selection queues only deletion.
Repeated deletion can append another group after a restart while the cache
still retains the task. Earlier groups and possibly-sent payloads remain exact.
Desktop Delete separates durable admission from safe display eligibility.
Core validates every fresh delete, deselection, and retarget as one group against
the complete retained aggregate through the production reducers. This matches
Desktop Store's prospective-write validation, including claimed rows that safe
display suppresses. Every fresh member must apply in that admission check.
An ignored retarget, malformed member, invalid
retained dependency graph, stale ownership, or insufficient identity material
still rejects the complete plan.

The returned `projection` continues to use the original covering head and
delivery proof for every complete retained domain. Claimed rows can therefore
suppress a domain even though a new valid group is admitted. A new member absent
from safe projection has `groupOutcomes.outcome: "queued"`; a member that wins
safe projection has `"applied"`. Both outcomes belong to one durable atomic
group and must commit. Core never restores proof for a claimed older row,
changes an outgoing snapshot, rewrites an older payload, or forces queued work
into safe display. With `canonicalHead: null`, new Delete members queue while
safe projection remains canonical-only. This exception applies only to
`desktopStorage` Delete, including current-task Delete without `knownTasks`.
Other mutation intents and profiles retain their existing acceptance rules.

Adoption steps:

1. Under the existing account and workspace transaction, read the raw persisted
   `snapshot.knownTasks` array together with the complete canonical base, all
   retained queues, proof, clock, sequence, and observation metadata. Supply
   `knownTasks` without filtering malformed records or adding them to `base.tasks`.
2. Use `desktopStorage` for Delete. Supply up to three unused UUIDv7 candidates
   for delete, deselect, and retarget. Core chooses which identities to consume.
3. Persist all returned `durableOperations`, never-sent proof, allocation, and
   physical observation metadata in one transaction. Keep the cache separate
   from canonical tasks. Preserve Desktop's cache remembrance and title
   normalization with the existing `task.identity.v1` contract. Convert returned
   RFC 3339 command observations to Desktop's integer physical-millisecond
   metadata with an exact round-trip check. Commit `queued` members too.
   Do not infer commit eligibility from safe display winners or replace an
   existing outgoing claim. Write nothing on `noop` or error.
4. After commit, reload and render, then execute `launchSync`. Preserve the
   returned queue order and every `atomicOperationIds` member. Current Qt Delete
   uses two Store transactions when it deselects; adoption intentionally upgrades
   that path to one commit for delete, deselect, and retarget.

`fixtures/workspace-intent-desktop-known-tasks-v1.json` supplies 12 original
cases and three queued vectors shared by native tests and the source probe.
`tests/workspace_intent_desktop_known_tasks.rs` adds 14 native regressions for
the matrix, cache validation, profile rejection, selection and upsert boundaries,
repeated deleted groups, historical attribution, Restart, and exact unrelated
possibly-sent queues, and rejection of incomplete or unsafe atomic groups.
The first run had five failures because `knownTasks` was unknown.
The completed implementation passes all 14 tests. A separate admission unit
test rejects ignored and malformed members even when other members apply.
`scripts/desktop_known_task_source_probe.py` imports the actual Desktop Store
and Qt controller and uses real SQLite transactions. Only wall time and UUID
entropy are controlled. Its native dispatcher calls production Rust for every
identity, clock, and projection decision. Twenty cases compare complete Store
returns, all durable queue fields, retained payloads, allocation, proof, physical
observations, projection, and the full controller outcome with effect types.
Unknown requests compare unchanged SQLite dumps. Corrupt identity rejection
also proves rollback and the actual notice outcome. Restart reopens the real DB.
The probe reads the actual raw canonical head, persisted dependency rows,
delivery proof, and outgoing claim under the Store transaction. It asserts the
native decoding and retains the raw metadata in its evidence. SQLite rows and
the serialized outgoing claim remain byte-for-byte equal across later writes.
The legacy projector omits `lastIntent.deviceId`; the probe explicitly accounts
for that existing serializer difference when comparing complete projections.

The baseline executable rejects the new input. With `--legacy-input`, the same
source probe shows the original `task delete requires an active task` error
beside Desktop's actual complete delete return. Both baseline runs fail.
The original repair passed 15 cases. Local evidence for that first run is under
`target/desktop-known-task-evidence/`: `native-red.log`, `source-red.log`,
`source-legacy-red.log`, `native-green.log`, and `source-green.json`.
The saved native baseline executable is under that directory's `baseline/`.

Independent review then found that the first repair incorrectly required all
fresh members to win safe display. The queued regression baseline fails four
native tests and all three actual Store reproductions with
`workspace mutation did not win safe projection`. Cache-only Delete followed
by `sync_payload`, a lost response, reopen, and repeat Delete appends the new
operation. Selected-running `set_selected_task_id`, claim, reopen, and Delete
returns IDs ending `003`, `004`, and `005`, HLC counters `2`, `3`, and `4`,
device sequence `9`, and retarget elapsed `15000` milliseconds. The original
claimed payload and retired proof remain unchanged. A raw null head also queues
Delete. The repaired planner passes these reproductions and the retained
dependency case without inventing optimistic winners for suppressed domains.
The negative causal case retains a claimed Cancel while safe display still
shows a running timer. Desktop's prospective retarget is rejected. Core also
rejects the complete group. The source probe records Desktop's historical
partial commit of Delete before its second transaction fails. Core's atomic
rejection intentionally prevents that partial commit during adoption.
The native Desktop reader still replays without a covering head; the null-head
probe explicitly checks Core's conservative display against the actual native
pre-mutation projection and documents this existing display difference.

Queued evidence is under `target/desktop-known-task-evidence/queued/`:
`native-red.log`, `cache-claim-red.log`, `selected-claim-red.log`,
`null-head-red.log`, `native-green.log`, `clippy-green.log`, and
`source-green.json`. The source evidence contains complete raw requests,
native delivery metadata, original and appended SQLite rows, actual Store
returns, complete controller outcomes, native projection, and complete Core
results. The queued baseline executable is under `queued/baseline/`.

Run the source comparison with Desktop's installed Python dependencies:

```sh
uv run --frozen --project ../desktop python scripts/desktop_known_task_source_probe.py
```

Pinned Rust 1.97.1 native format, warning-denying Clippy, and all-target,
all-feature tests pass with exactly the two named local-WASM tests skipped
below. Existing intent and mutation source probes also pass. The size audit
reports zero violations and the same seven documented exceptions. The queued
repair changes Core's production entity count from 603 to 605. Cyclomatic mean
stays 3.58, cognitive mean changes from 2.87 to 2.88, and both p95 values stay 9.
The extra decisions select bounded Desktop Delete admission and validate every
fresh group member against retained causal state independently of display
suppression. Shared queued vectors,
production claim and restart tests, and negative admission tests cover those
branches. No production size exception is added.

## Verification and remaining work

`fixtures/workspace-intent-v1.json` drives the full action/status matrix and
seven checker cases for stale Terminal Cancel, retained-history Desktop
restart, unrelated IDs, and centralized/Iroh ownership writes.
The three new native regressions failed before repair: stale Terminal Cancel
returned `planned` instead of `noop`; retained-history restart returned no
commands instead of Clear/Start; Iroh Start returned `recordStart`. All three
now pass. An unrelated presented timer ID and a nonempty pending command queue
remain negative cases for retained-history restart.
`tests/workspace_intent.rs` calls the production dispatcher for the matrix,
exact command identities and fields, atomic pairs, selected task, local
generation overflow, physical observation, clock skew, malformed boundaries,
frozen queues, and Skip cycle for 0 through 12 completed focuses.
`scripts/workspace_intent_source_probe.py` executes extracted production
methods from Swift, Kotlin, Python, and JavaScript with native Core dispatch
as the comparator. The probes test those extracted methods, not a duplicated
reference algorithm. The checker probe executes Desktop's actual
`_terminal_action_context`, `_retained_terminal_context`, timer fingerprint,
and `_apply_timer_command_side_effects` against those seven cases. The full
probe passes 14 Apple, 36 Android, 40 Desktop, and 41 PWA source cases.
The additional PWA cases execute `app-state.js`'s `elapsedFor` and
`app-storage.js`'s command builder through forward and reverse wall jumps,
pause/resume, null anchor, fractional and backwards monotonic time, timer replacement,
and browser restart. They compare complete generated commands and
`app-view.js` display fields against Core live readings. The first extension
test failed against the first slice because `clock.continuityId` was unknown.
The two checker regressions then failed against the first monotonic extension:
Core returned `16000` instead of `15999.5` live milliseconds, changing a
45-second display to 44 seconds; it dropped the anchor during a null sample,
yielding `45000` instead of `17000` milliseconds on the next reading. Both
regressions failed in native tests and in the production-source probe before
repair. Core now retains fractional live elapsed and rounds only wire
`observedElapsedMs`. It retains an anchor across an absent reading unless an
explicit new continuity ID or terminal timer invalidates it. These probes do
not prove platform transaction or alarm behavior. The 33 native intent tests and
all other native targets pass with the two named WASM-building tests filtered.
Pinned format and warning-denying Clippy pass. Code-size audit reports zero
violations and seven pre-existing exceptions; no Core production exception
was introduced. Relative to the dirty Core baseline before this slice,
cyclomatic mean rose from 2.84 to 3.09 and cognitive mean from 2.06 to 2.36.
Cyclomatic p95 rose from 7 to 8; cognitive p95 rose from 8 to 9. The increase
comes from five-profile admission, continuity validation, and precise live
observation branches needed to preserve client behavior in Core.

Run native checks with Rust 1.97.1 on `PATH` and set `RUSTC` to its compiler:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --all-features --locked -- \
  --skip wasm_abi_handles_malformed_ranges_and_cleanup_in_a_real_host \
  --skip c4_release_wasm_rejects_oversized_allocations_without_trapping
cargo build --locked --example completion_policy_probe
python3 scripts/workspace_intent_source_probe.py
```

Only the two named tests build local WASM. They are excluded by the first
slice's constraint. Packaged-WASM validation, client adoption, and their
transaction tests remain independent integration gates. Extend this same
versioned intent family for finish/expiry/auto-break, task/retarget, and
preference mutations once their complete transactions and compatibility rules
fit. This first slice does not close CORE-M03 across clients.

### Platform continuity differences

PWA `performance.now()` has no OS boot ID. A browser-lifetime continuity ID
prevents a saved anchor from crossing a browser restart even when a new
monotonic reading exceeds the old one. Android exposes a real boot ID and
checks it when restoring a server clock sample. Apple recovers trusted time
from a bounded wall observation when persisted uptime moves backwards; it
does not expose a boot ID in `TrustedClockState`. Desktop checks physical
versus monotonic drift when it restores a persisted server clock sample.
Those platform-specific trusted-clock recovery rules remain outside this
intent operation. Future adapters must supply a trustworthy continuity ID
or decline monotonic elapsed and use the wall-time fallback.
