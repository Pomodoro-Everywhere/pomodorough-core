# Raw workspace bootstrap plan

`bootstrap.workspacePlan.v1` classifies a raw local workspace and a remote
snapshot, then calls the existing `bootstrap.plan.v1` strategy policy. The new
operation is opt-in. The shipped operation retains every mode, strategy, reason,
count rule, and owner rule.

## Request

The shared fixture `fixtures/bootstrap-workspace-v1.json` contains a complete
request and legacy record examples. The request has these fields:

- `profile`: `appleWorkspace`, `androidRepository`, `desktopStorage`, or `pwaStorage`.
- `currentUserId`: the incoming account identity, or null for an unowned workspace.
- `local.ownerId`: the saved workspace owner, or null.
- `local.workspace`: the persisted input of `workspace.project.v1` described below.
- `local.preferences`: the raw local settings described below.
- `local.knownTasks`: the complete cache of known tasks. This cache alone does not
  make a workspace meaningful.
- `local.projectionPending`: the optional persisted PWA display queues. Other
  profiles reject a non-null value. These are original records, not a classification.
- `remote`: the raw canonical bootstrap snapshot.

`local.workspace` contains `base`, `local`, `canonicalHead`, `timerDependencies`,
and `now`. Its `local` object must contain all five complete pending queues:
`commands`, `taskOperations`, `durationOperations`, `autoStartOperations`, and
`selectedTaskOperations`. These arrays contain the original operation objects.
`neverSent` contains the persisted proof records accepted by `workspace.project.v1`.
An absent `neverSent` means no proof. `canonicalHead` must be an HLC object with
`wallMs` and `counter`, or null. No synthetic head is required for an empty account.
`now` is the raw observation time in the canonical timestamp domain. Android
validates this observation but derives its projection horizon in Core.

Both `local.workspace.base` and `remote` contain these fields:

```json
{
  "canonicalTimer": null,
  "history": [],
  "tasks": [],
  "durationsMs": {"focus": 1500000, "short_break": 300000, "long_break": 900000},
  "autoStartBreaks": false,
  "selectedTaskId": null
}
```

The remote object can retain transport fields such as revision, acknowledgements,
server clocks, and user identity. Classification does not consume these fields.
The local base remains the original canonical wire state. Physical timestamp
translation and transport validation remain adapter responsibilities.

`local.preferences` requires `durationsMs`, `autoStartBreaks`, and `selectedTaskId`.
It can retain native fields such as `selectedPhase`. Such fields do not count as
synced state. Android also accepts null `durationsMs` and raw `focusMinutes`,
`shortBreakMinutes`, and `longBreakMinutes`. Missing minute fields use the native
defaults of 25, 5, and 15. A non-null duration map takes precedence over minutes.
PWA accepts the raw `defaultDurationsMs` map. Its absence uses the product defaults.

The operation rejects derived classifications at the root, local, workspace,
base, preferences, and remote boundaries. These include `hasLocalState`,
`hasRemoteState`, history counts, and `projectionResult`. Clients supply records,
not preselected outcomes or counts. `horizon` and `projectionHorizon` are also
rejected at these boundaries. There is no caller-computed Android horizon field.

## Result

An empty request produces this complete result:

```json
{
  "plan": {"mode": "auto", "strategy": "keep_remote", "reason": "empty"},
  "classification": {
    "profile": "appleWorkspace",
    "local": {"hasState": false, "completedHistoryCount": 0, "displayHistoryCount": 0},
    "remote": {"hasState": false, "completedHistoryCount": 0, "displayHistoryCount": 0}
  }
}
```

`plan` is the exact v1 result, including omitted fields. In particular, two
meaningful workspaces without completed history still produce
`auto/merge/local_state_only`. Remote-only state without completed history still
produces `auto/keep_remote/empty`. No new reason strings appear in `plan`.

`completedHistoryCount` uses the shipped v1 rule. Only `status: "completed"` rows
with a nonempty string `timerId` or `id` count. The rule prefers `timerId` and
deduplicates namespaced identities. `hasState` includes noncompleted history.
`displayHistoryCount` preserves the selected profile's presentation rule. It does
not change the plan's counts or strategy.

Owner precedence stays in v1. The same nonempty saved owner produces
`normal_sync/same_owner`. A different owner produces
`auto/keep_remote/different_owner`. A saved owner without an incoming identity
fails. Classification remains available with owner-based plans.

## Explicit profiles

All profiles count every nonempty pending queue as meaningful, including a queued
delete, default duration reset, disabled auto-start, or deselection. Delivery
uncertainty does not erase this presence.

`appleWorkspace` preserves `AppModel.hasLocalBootstrapState` and
`AccountSynchronization.hasRemoteBootstrapState`. Local presence includes the
raw canonical timer, raw history, active tasks, local duration settings, and
auto-start. Local and remote selection alone do not count. Known-task caches alone
do not count. Display counts use the v1 identity rule.

`androidRepository` preserves `TimerRepository.hasLocalSyncState` and
`hasRemoteSyncState`. Local timer and history use the same horizon as
`SynchronizedProjectionRequestFactory`. Core selects the last command by HLC
wall, counter, UTF-8 device identity, and UTF-8 command identity. Its occurrence
is the horizon. With no commands, the canonical anchor is the horizon. With no
canonical timer, the horizon is the epoch. The complete retained queue participates,
including ignored commands and commands excluded from delivery-safe projection.
Core does not use the maximum occurrence timestamp or the observation deadline.
Active tasks and raw local selection and settings count. Remote
selection counts. An empty string selection is non-null and therefore counts.
Display counts count completed rows without identity deduplication, matching
`visibleHistoryCount`.

`desktopStorage` preserves `SyncStorage._has_bootstrap_state`. Raw canonical
history, the canonical timer, active tasks, base selection, local settings
selection, duration settings, auto-start, and the projected timer count. Empty
string selection counts because the method checks null, not string length.
Bootstrap projection reads complete retained queues at the raw response observation,
matching `bootstrap_resolution_plan` and `Store._project_operation`. Display counts
use `_completed_history_count`, which matches v1.

`pwaStorage` preserves `sync-core.hasLocalState` and `hasRemoteState`. Local synced
settings and selection come from the canonical base, not independent native
preferences. Pending settings operations already make local state meaningful.
Selections require nonempty strings. `defaultDurationsMs` controls the comparison
for both sides. `completedHistoryCount` remains strict v1. Display counts match
`sync-core.completedHistoryCount`, including legacy rows without a status and
identityless rows. Duplicate nonempty identities count once.

PWA projects all retained queues when no persisted `projectionPending` exists.
When that record exists, Core reads its stored command records and adds fresh
commands with never-sent proof and clocks newer than the canonical head. A null
head admits a fresh proven command, matching `projectionQueuesForDisplay`.
Stored records must match the complete retained payloads exactly. Duplicate,
unknown, rewritten, and incomplete stored records fail with a recovery error.

The Android and PWA timer or task projection can differ from the raw base when
operations exist. Nonempty queues independently make presence true. Only timer
projection can change the completed history counts needed by this endpoint.
Task, duration, auto-start, and selection reducers are not required for this read.

## Validation and delivery

Strict JSON rejects duplicate fields before typed decoding. Each history, task,
known-task, and pending queue array is limited to 10,000 records. The existing
16 MiB input limit also applies. Arrays require object records. Duration maps
contain exactly the three phases with integer values between 60,000 and
14,400,000 ms. Android minute fallback values must be integers between 1 and 240.

Classification preserves allowed legacy task and history identifiers. It does not
require UUIDs, recompute task title identities, or reject a dangling legacy
selection. With no timer and no timer commands, history rows can omit replay-only
timestamps and fields. Noncompleted and unclassified rows remain meaningful.

Timer projection uses the existing workspace terminal boundary. It validates
complete retained timer commands, queue identities, HLCs, dependency records,
and proof records before applying the explicit profile's read policy.
A matching terminal timer and history row are accepted without caller repair.
Standalone terminals use Core's existing reconstruction. Apple retains the
delivery-safe policy: commands project only with never-sent proof and a canonical
head below every retained clock in that domain. Android and Desktop bootstrap
reads use complete retained queues, matching their current production callers.
PWA uses its persisted display records as described above. These are read policies,
not authorization to publish possibly delivered operations. The endpoint does not
change `workspace.project.v1`, delivery safety, or any queued payload.

A timer or timer command that needs replay cannot use incomplete legacy history.
Malformed or conflicting aggregates fail with `bootstrap workspace requires
recovery: ...`. Invalid remote timers fail with `bootstrap remote timer requires
recovery: ...`. The operation does not clear records, normalize task identities,
rewrite queued payloads, or select a destructive recovery strategy on error.

## Migration evidence

`scripts/bootstrap_workspace_source_probe.py` compiles the complete current
Android `SynchronizedProjectionRequestFactory`, `CoreProjectionDispatcher`, and
`Models.kt` with Kotlin serialization. The actual factory creates the request,
the actual dispatcher invokes native `projection.apply.v2`, and repository
classifier methods consume the returned timer and history. Raw preferences remain
the supplied repository observation, not values invented by the probe.

Desktop executes `Store._project_operation`, its queue and request builders, the
actual `shared_core.apply_projection_v2` decoder, and the complete
`SyncStorage.bootstrap_resolution_plan` body. Persistence and transport provide
record inputs and inert side-effect hooks. PWA executes the current
`projectOwnerState`, `projectState`, `localBootstrapState`, and `buildBootstrapPlan`
methods. Its clock consumes the supplied raw wall observation. Swift classifier
bodies remain compiled from current sources.

Each successful receipt contains the raw native observation, raw production
projection request, complete production return, and the new Core raw request and
result. Request echoes assert `JSON.parse(inputRaw) == inputDecoded == sentInput`.
Response and observation decode equality are separate assertions. No projected
result is supplied as the new endpoint's raw canonical base.

Incomplete legacy history, noncanonical legacy tasks, and dangling selection
vectors can pass the classification-only endpoint while failing today's full
client projection. The probe locks the exact rejected vector names and reports
these 69 expected production rejections separately. It neither repairs these
inputs nor counts them as successful projection comparisons.

Desktop's existing `_bootstrap_strategy` omits owners because its caller owns
the account lifecycle. Its complete strategy return and emitted v1 request are
checked separately for owner fixtures. The new endpoint accepts raw owners and
delegates owner precedence to v1. A Desktop adapter must handle `normal_sync`
before invoking its existing strategy-only decoder.

Android's `authenticationBootstrapPlan` has a separate repreview gate that can
suppress a same-owner v1 input. This endpoint classifies the saved owner as supplied.
The source differential covers the repository's state classifiers, not that
account lifecycle gate. Adapter migration must preserve repreview and persisted
resolution admission before invoking a new bootstrap plan.

The new operation is ready for adapter mapping and independent checking against
these fixtures. Adapters still need to capture a coherent persisted workspace,
map native field names, pass complete delivery records, retain existing recovery
gates, and render `plan` plus `classification`. Packaged WASM availability and
client runtime adoption require separate verification. This change does not add
client callers or publish an artifact. The main task owns backlog status.

## Verification receipt

The first new endpoint test failed with
`UnsupportedOperation("bootstrap.workspacePlan.v1")` before implementation.
The corrected native run uses Rust 1.97.1 with an explicit pinned `RUSTC` and a
fresh native target directory. All 489 native tests pass, including seven new
repair tests. Exactly these two tests are filtered because they build WASM:

- `wasm_abi_handles_malformed_ranges_and_cleanup_in_a_real_host`
- `c4_release_wasm_rejects_oversized_allocations_without_trapping`

Pinned formatting and all-target, all-feature Clippy with `-D warnings` pass.
The previous 293-observation probe constructed projections from the raw base and
did not prove production projection parity. The repaired probe passes 329 actual
source comparisons: 73 Apple classifier observations, 74 Android, 79 Desktop,
and 103 PWA. The existing Desktop decoder differential covers all 22 complete
v1 vectors, including its two expected ownership rejections.

Before the fix, the two native horizon tests failed at the deadline and with an
early retained command. The same repaired source probe against the pre-fix native
library fails 63 comparisons: 17 Android, 11 Desktop, and 35 PWA. The receipt
includes the exact running-focus repro. Before the fix, Core returns
`auto/replace_remote/local_only` after the deadline and `choose` with a remote
task. The production Android factory returns `auto/merge/local_state_only` in
both cases. The corrected endpoint returns that exact complete v1 result and
zero completed-history counts. Shared fixtures also cover a null canonical timer,
terminal pairs, command occurrence rollback, and UTF-8 identity ordering.

The size audit reports zero violations and seven pre-existing documented
exceptions. No new exception is required. Compared with the rejected implementation,
Core grows from 595 to 601 entities. Cyclomatic mean remains 3.57. Cognitive mean
changes from 2.86 to 2.87 because Core now owns native horizon selection,
persisted PWA queue validation, and the profile-specific read choice. Both p95
values remain 9. New production functions have at most 49 lines.
`scripts/bootstrap_workspace_metrics_probe.py` verifies the pre-task source
fingerprint after excluding the new endpoint files and removing the three additive
declarations. It also restores the original private visibility of the shared
workspace shape validator for that comparison. Every pre-endpoint function keeps
its complexity scores. Only `dispatch_json` gains one line for
the new operation. New entities cover profile classification, boundary validation,
delivery-aware timer projection, native projection horizons, persisted PWA queues,
and the native probe.

The native and source checks can be repeated with these commands. `RUSTC` points
to the pinned compiler even when a system compiler appears earlier on `PATH`.

```sh
export RUSTC="$(rustup which --toolchain 1.97.1 rustc)"
export RUSTDOC="$(rustup which --toolchain 1.97.1 rustdoc)"
rustup run 1.97.1 cargo fmt --all -- --check
rustup run 1.97.1 cargo clippy --all-targets --all-features --locked -- -D warnings
rustup run 1.97.1 cargo test --all-targets --all-features --locked -- \
  --skip wasm_abi_handles_malformed_ranges_and_cleanup_in_a_real_host \
  --skip c4_release_wasm_rejects_oversized_allocations_without_trapping
rustup run 1.97.1 cargo build --locked --example bootstrap_workspace_probe --example bootstrap_plan_probe
python3 scripts/bootstrap_workspace_source_probe.py
python3 scripts/bootstrap_desktop_source_probe.py
```

Both source probes accept `--native PATH` when a separate native target directory
is used. The repaired source probe accepts `--report PATH` for complete raw receipts
and `--profile PROFILE` for a single production pipeline. The Android probe needs
the Kotlin compiler and serialization runtime jars available with Android Studio
and Gradle. `KOTLINC` and `JAVA_HOME` select explicit installations.
The metrics probe accepts `--before PATH` for the pre-task JSON report.
It rejects unrelated source changes instead of replacing the report snapshot.
