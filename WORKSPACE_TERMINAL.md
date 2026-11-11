# Workspace terminal state

CORE-M05 adds a bounded persisted-state boundary to `workspace.project.v1`.
The root request and result shapes do not change. The workspace timer now retains
optional native `lastIntent.deviceId` metadata. The operation accepts a raw canonical
terminal timer alongside its corresponding history row. The same aggregate works
with `workspace.readModel.v1`, `workspace.intent.v1`, and staged
`workspace.completionMutation.v1`.

## Identity and provenance

The canonical timer's `id` must equal the matching row's `timerId`. The row's `id`
can differ for a legacy history identity. Core preserves that identity. A history
ID cannot alias another session's timer ID. Duplicate history IDs and duplicate
session IDs retain their existing rejection. Running and paused overlaps fail.

The pair must agree on task, phase, status, and planned duration. Null and omitted
optional task values both represent no task under the existing timer wire type.
Every supplied terminal timestamp must equal the timer's anchor as a parsed
RFC 3339 instant. A completed row requires `completedAt`. Cancelled and superseded
rows require `endedAt` and cannot contain `completedAt`.

A history row records `elapsedMs`: the time its session ran, pauses excluded,
as an integer from zero through the planned duration. Every row Core writes
carries it. A row without it predates the field. Such a row counts its planned
duration when completed and zero otherwise, and the row paired with the canonical
terminal timer takes that timer's `elapsedAtAnchorMs`. A row that records
`elapsedMs` must equal the paired timer's `elapsedAtAnchorMs`. An explicit
`"elapsedMs": null` loads as a row without the field, like every other optional
field of the row. A value outside zero through the planned duration fails closed
with `invalid shared-core input: invalid timer history` on the reducer, workspace
projection, read model, intent, and reconciliation boundaries. A value above the
signed 64-bit range fails at JSON decode.

Each ending records this elapsed time on the timer and its history row:

| Ending | Status | Elapsed |
| --- | --- | --- |
| Deadline reached | completed | The planned duration. |
| Finish while running, before the deadline | completed | The time measured to the command's `occurredAt`, so it can be below the plan. |
| Finish at or after the deadline | completed | The planned duration. The deadline completes the session first. The Finish still applies: `completedAt`, `endedAt`, and the history `commandId` move to the Finish. |
| Finish while paused | completed | The elapsed time its Pause stored. |
| Finish on a completed, cancelled, or superseded session | completed | The elapsed time that session already recorded. A cancelled or superseded row without `elapsedMs` therefore becomes completed with 0, and a completed row without it keeps the planned duration. |
| Cancel | cancelled | The command's `observedElapsedMs`, clamped to zero through the planned duration. |
| Superseded by another timer's command | superseded | The time measured to the superseding command, clamped. |

A Finish command's own `observedElapsedMs` is never used. Pause and Resume each
store their command's `observedElapsedMs`, clamped to zero through the planned
duration, so a Finish while paused records a value that a client supplied.
Intent provenance follows the public reducer's transitions:

- An explicit Finish has a `finish` intent. Its command ID and time equal the
  history command ID and terminal time.
- An explicit Cancel has a `cancel` intent with the same exact command and time.
- Deadline completion retains its prior `start` or `resume` intent, whose time
  cannot exceed completion time. History has no terminal command ID.
- Supersession retains a prior `start`, `pause`, or `resume` intent. If history
  names a superseding command, it must differ from that prior command. The prior
   intent can occur after the end when HLC order and occurrence time differ.
   Missing superseding provenance stays missing. Core does not add an occurrence
   ordering constraint to the public reducer's HLC ordering.
- A legacy completed or cancelled snapshot without intent is accepted only when
  its history also has no command provenance. Supersession of an imported active
  snapshot can lack prior intent while history records its superseding command.
  Empty command IDs fail.
- A `clear` intent does not justify a retained terminal pair. Explicit null and
  an applied Clear are the supported cleared representations.

If a provenance command remains in the complete retained queue, Core compares only
fields that the corresponding reducer uses. Finish and Cancel compare kind, timer,
and occurrence time. Cancel also compares observed elapsed clamped against the
session's planned duration. Their task, phase, and command duration do not replace
the session values and cannot conflict with the pair. Pause and Resume likewise
ignore task, phase, and command duration. Start owns phase and duration, so those
fields remain checked for a retained prior Start. A later retarget can change task
without changing that prior intent. A superseding command must target another
timer and use a reducer kind that can supersede a session.

Ignored fields still pass the existing command shape, timestamp, identity, and
numeric validators. The canonical timer and history pair always compare task,
phase, duration, status, identity, and terminal timestamps strictly. These are
separate validations.

These checks run before queue suppression. A frozen or uncertain domain cannot
hide conflicting provenance. A pair conflict returns:

```text
invalid shared-core input: conflicting workspace terminal timer/history
```

Malformed timer and history fields retain existing validation errors. The Core
error envelope and WASM exports do not change.

## Replay and missing state

`src/timer/workspace.rs` seeds one session from an exact pair. The canonical
object owns display identity, elapsed, starter, and last intent. The history row
owns its history identity and terminal command. The shared reducer then owns
ordering, transitions, Clear, and history generation. Core does not remove a row
or pick a terminal object by array order.

A missing corresponding history row uses the canonical timer to seed the session.
Core validates standalone terminal elapsed and intent before synthesizing history.
An underelapsed completed timer without a `finish` intent, or a standalone terminal
Clear intent, fails on the first read. The raw workspace result also passes the pair validator before return,
so deadline completion cannot produce a result rejected on its next read.
A prior active intent cannot become a synthetic terminal command. Deadline
completion therefore creates history without a Finish command, and supersession
without replacement evidence leaves its terminal command absent. Explicit Finish
and Cancel retain their exact commands.
A missing canonical timer remains invalid in the required workspace base schema.
Explicit `canonicalTimer: null` remains authoritative even when terminal history
exists. Equal timestamps on different sessions do not change which timer is
displayed. An eligible Clear removes the current display and preserves history.
A suppressed Clear leaves the raw terminal display intact.

The result uses the existing typed projection serialization. Optional null fields
are omitted where the timer and history wire types already omit them. Anchor and
history timestamps use reducer UTC formatting. The saved intent timestamp retains
its existing representation. Unknown extension fields are not domain fields or
history selectors. Retained delivery payloads remain exact in `projectionPending`
and the caller's durable queues.

`projection.apply.v2` and `timer.reduce.v1` still reject every canonical/history
overlap with `canonical timer overlaps timer history`. Reconciliation retains its
strict canonical-response contract. CORE-M05 changes only the new workspace
boundary and its composed consumers.

## Physical observation and native metadata

Intent and completion planners validate the complete raw wire workspace first.
The physical observation then enters private replay state through
`timer/workspace/observation.rs`. No command, canonical timer, intent, or history
timestamp in the raw JSON workspace is rewritten to make validation pass.

An explicit `observation.canonicalAnchorAt` controls the display anchor. Otherwise,
a saved physical time for the canonical intent identifies its anchor offset.
Eligible command replay uses saved command observations without changing HLC
order or durable payloads. If replay reproduces an installed terminal identity,
Core retains its wire history timestamps and saved lifecycle intent. The display
anchor can therefore be physical while its history and intent remain wire values.
That display projection is not a canonical wire aggregate for persistence.
New optimistic transitions retain their existing physical display behavior.
Completion admission keeps the existing concurrent-Finish no-op path. An active
presentation carrying a Finish or Cancel marker is checked without expiry before
the stale decision. Its no-op projection does not manufacture terminal history
from that marker. Invalid standalone terminal snapshots still fail validation.

The repair covers wire Finish at `12:00:10Z` and physical observation at
`12:00:15Z`, with the full originating Finish queue retained after proof retirement.
It also covers an empty queue with `canonicalAnchorAt: 12:00:15Z`. Intent and
lifecycle calls return the original workspace and observation without synthetic
caller state. A physical observation cannot mask conflicting raw wire provenance.

Workspace `lastIntent.deviceId` accepts a nonempty string or null. Null is omitted
in typed output. A supplied value is retained for the same intent, including after
its originating command is replayed. A newly generated intent derives its device
from the originating command. Device metadata does not change lifecycle identity
or stale-intent decisions. The three identity fields remain type, command ID, and
occurrence time.

Legacy `timer.reduce.v1`, `projection.apply.v2`, and replay-page outputs still omit
the native field. Legacy timer reduction also keeps ignoring malformed and
duplicate native extension values. New workspace input rejects invalid device
metadata and recursive duplicate JSON fields. This is an additive workspace
output contract, not a change to the older reducers' serialized fields.

## Source evidence

The supported representation comes from current client storage and adapters:

- Apple `SharedCoreModels.swift::CoreProjectionBase.init` clears a timer whenever
  its session appears in history. `IrohRoomProjection.restoreTerminalTimer` then
  restores the genesis terminal object or reconstructs a terminal result.
- Desktop `storage.py::_projection_input` clears the overlapping timer.
  `ui_controller.py::presented_timer` restores the retained terminal object unless
  a matching Clear applies. `core.py::_compatibility_projection_base` also clears
  overlap in the older compatibility reader.
- Android `CoreProjectionDispatcher.projectionInput` uses `takeUnless` to clear
  the same overlap. Its native test names this history-only mapping explicitly.

`scripts/workspace_terminal_source_probe.py` executes the unchanged Desktop input
and presentation methods. It compiles Apple's unchanged terminal mapper and
elapsed method with the actual `CanonicalTimer`, `TimerIntent`, and `HistoryItem`
definitions. Twenty fixture cases compare complete returned terminal values.
Two more Apple cases exercise the nullable-timer reconstruction branch for Finish
and Cancel using history and outcomes from the real native reduction.
Ten more vectors retain an actual task-attributed completion mutation's full queue
and retired proof. Two Apple vectors compile the unchanged
`PersistedTimerState.physicalCanonicalTimer` method with saved observations.
The probe compares complete returned timers, including device metadata. Physical
clock dependencies come from the observation inputs, not Core's expected output.
Desktop compares complete history. Two intentional differences expose the old
public reducer's synthetic Start or Pause terminal command when history is absent.
The probe asserts that exact difference and compares every remaining field.
Only documented optional-null omission
and equivalent whole-second timestamp encoding are normalized for comparison.
No returned field is replaced with an input field. The probe prints source hashes.

Android's mapper is inspected, not compiled by this probe. Source parity does not
establish native app, persistence, alarm, packaged-WASM, or artifact adoption.

## Independent checker evidence

Before the implementation, the new positive fixture test failed on its first
raw persisted Finish pair with `canonical timer overlaps timer history`. The test
then passes without changing that input. The separate strict-public-reducer test
continues to reject the same pair.

`fixtures/workspace-terminal-v1.json` has twenty positive cases and twenty
conflicting pairs. A runner starts from `request`, overlays `timerOverrides` and
`historyOverrides` on the supplied objects, and applies the case controls as
shown in `tests/workspace_terminal.rs`. Overrides replace fields, not nested
objects. `conflictError` is the exact expected error for every rejection.
Four more cases in `elapsedRangeRejections` put `elapsedMs` at -1 and at the
planned duration plus one, with and without the paired timer. Their exact error
is `elapsedRangeError`. They are a separate list because they fail history
validation before the pair is compared.
`retainedCommands` supplies the complete originating queue. The ten
`ignoredCommandFields` vectors are also consumed by native reducer comparisons.

Native coverage includes restart, Clear, suppression, explicit null, missing
history, legacy history identities, equal-time distinct sessions, and timestamp
offset equivalence. Additional adversarial tests check duplicate identities,
cross-session aliases, C02 Start rejection, retained command conflicts, prior
intent conflicts, cancellation elapsed, resume followed by Finish, and composed
read-model and intent rejection. Lifecycle tests install a real staged Finish
result as a raw pair, then project, read, and continue it without clearing state.

The checker repair first added six production-dispatch regressions to the rejected
implementation. All six failed before source changes. The failures covered the
real task-attributed Finish with omitted command task, sixteen of twenty Finish
and Cancel field variants, both physical observation forms in both planners,
both invalid standalone cases, HLC supersession with occurrence rollback, and
missing device metadata. The expanded zero-command matrix then exposed an
additional deadline result that failed reopening. Its output postcondition is now
checked before return.

`tests/workspace_terminal_checker.rs` contains fourteen passing regression tests.
Ten field vectors exercise all four Finish, Cancel, Pause, and Resume kinds against
real public reducer outputs with complete uncertain queues. A 120-case
zero-command matrix accepts sixty-two aggregates and rejects fifty-eight. The two
added acceptances are Finish completions below the plan. The test compares the
accepted set with a table of twenty-six status and intent rows, so a change that
swaps one acceptance for another fails even when both counts stay. Every accepted
result reopens exactly. Proof retirement, metadata preservation, invalid physical
masking, old extension behavior, and generated intent devices have separate checks.

## Integration requirements

Adapters must pass the authoritative terminal timer and history directly to the
workspace call. Read-model sources, intent workspaces, and lifecycle workspaces
use that same representation without caller reconstruction or timer clearing.
Hosts still supply complete retained queues, covering head, delivery proof,
dependencies, and coherent wire timestamps. Results cannot replace canonical
state after optimistic replay.

Adapter adoption must remove the client overlap clearing and terminal fallback
for workspace calls. Older reducer and reconciliation adapters remain separate
compatibility paths. Missing authoritative terminal state, contaminated canonical
preferences, packaged-Core verification, and supported platform transaction tests
remain integration blockers. The main owner retains the backlog and release work.
The follow-up source probe still reports the existing Android and PWA generated-
break parent divergence. Those sources attach the next Finish to the original
focus Finish, while Core requires the generated Start as its direct parent.
Apple agrees with Core. This separate follow-up adoption work remains open.

## Verification

On 2026-10-01, Rust and Cargo 1.97.1 pass formatting, warning-denying Clippy, and
448 native tests. Exactly the two local-WASM-building tests below are excluded.
The terminal source probe passes twenty-seven Desktop terminal comparisons and
thirty-one Apple comparisons, including both reconstruction branches. It asserts nineteen
exact rejection errors and two intentional legacy-history differences. Existing
lifecycle, read-model, intent, completion-mutation, and workspace-mutation source
probes also pass.

```sh
export PATH="$HOME/.rustup/toolchains/1.97.1-aarch64-apple-darwin/bin:$PATH"
export RUSTC="$HOME/.rustup/toolchains/1.97.1-aarch64-apple-darwin/bin/rustc"
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --all-features --locked -- --exact --skip wasm_abi_handles_malformed_ranges_and_cleanup_in_a_real_host --skip c4_release_wasm_rejects_oversized_allocations_without_trapping
cargo build --locked --examples
cargo test --locked --test workspace_terminal_checker
python3 scripts/workspace_terminal_source_probe.py
python3 scripts/completion_lifecycle_source_probe.py
python3 scripts/read_model_source_probe.py
python3 scripts/workspace_intent_source_probe.py
python3 scripts/completion_mutation_source_probe.py
python3 scripts/workspace_mutation_source_probe.py
python3 scripts/completion_followup_source_probe.py
git diff --check
```

The root size audit reports zero violations and seven existing documented
exceptions. Core has no production exception. The repair adds raw-first observation
replay, terminal output validation, and native metadata preservation, and removes
comparisons of unused command fields. No metric snapshots are rewritten.
Core entity count changes from 513 to 527. Mean cyclomatic complexity changes
from 3.54 to 3.53, and mean cognitive complexity changes from 2.84 to 2.83.
Cyclomatic p95 remains 9. Cognitive p95 decreases from 10 to 9. Existing maxima
remain 19 cyclomatic and 28 cognitive. Other projects' source fingerprints and
metrics are unchanged.

### Elapsed-time history, 2026-11-11

History rows gained `elapsedMs`, and a Finish command now keeps the real elapsed
time on its completed timer. `tests/history_elapsed.rs` has eighteen tests. The
terminal fixture grew to twenty positive cases and twenty rejections with
unchanged error text, plus the four `elapsedRangeRejections`.

The range rejections run on `workspace.project.v1`, on `workspace.readModel.v1`
and `workspace.intent.v1` in all five profiles, and on `reconcile.rebase.v3`.
`tests/history_elapsed_wasm_parity.mjs` repeats them against the release artifact
with an early Finish, a row without the field, and an explicit null: 48 raw
native and WASM calls with pinned values and errors. It is a separate candidate
suite, so the hosted aggregate corpus and its preservation digest of inputs do
not change.

One test pins a known limit instead of changing Core. A strict path, such as
`timer.reduce.v1` or `projection.apply.v2` without the timer, writes `elapsedMs`
0 for a cancelled row that lacks the field. Storing that row beside the raw
cancelled timer, whose `elapsedAtAnchorMs` is above zero, fails the next
workspace read with the conflict error. A client must pass the timer with its
history, as the integration requirements above state.

`scripts/workspace_terminal_source_probe.py` passes its Desktop leg with thirty
terminal comparisons. It names six intentional legacy-history differences: the
two earlier synthetic-command cases, and four cases where Desktop's timer-clearing
legacy input meets a row without `elapsedMs` and therefore writes the pre-field
default. Its Apple leg matches thirty-three of thirty-four timers and stays red on
the nullable-timer Finish reconstruction. Apple's `terminalElapsed` still returns
the planned duration for a completed row and must read the row's `elapsedMs`.
