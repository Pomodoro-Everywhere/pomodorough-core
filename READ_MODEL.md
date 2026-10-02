# Workspace read model v1

`workspace.readModel.v1` returns display decisions from a raw canonical workspace. It does not write a command, change a selection, or modify stored state. Core validates the raw base, all five local queues, covering head, delivery proof, and dependencies through `workspace.project.v1`. Core then projects the timer at the observation time. The operation does not accept an unverified projection result.

## Input

The request uses this shape. `source.value` is the full raw input for `workspace.project.v1` without `now`.

```json
{
  "profile": "appleWorkspace",
  "source": {"kind": "workspace", "value": {"base": {}, "local": {}, "canonicalHead": null, "neverSent": {}, "timerDependencies": []}},
  "selectedPhase": "focus",
  "observedAt": "2026-03-08T07:30:00Z",
  "calendarIntervals": [{"start": "2026-03-08T05:00:00Z", "end": "2026-03-09T04:00:00Z"}],
  "monotonic": null
}
```

The `base` and `local` values in this abbreviated shape require the fields defined by `workspace.project.v1`. `fixtures/read-model-v1.json` contains a complete request. The only `source.kind` is `workspace`. The five `profile` values are `appleWorkspace`, `androidCoordinator`, `desktopStorage`, `desktopTerminal`, and `pwaStorage`. `selectedPhase` is `focus`, `short_break`, or `long_break`.

An earlier draft accepted `source.kind: "projectionResult"` without a way to verify its origin. That variant is rejected. To migrate a caller of the draft, supply its retained canonical base, local queues, canonical head, never-sent proof, and timer dependencies as `source.value` with `source.kind: "workspace"`. A caller that retained only a projected snapshot must recover the raw workspace before calling this operation. No client currently calls this draft operation. A `now` field inside `source.value` is rejected; Core derives projection time from `observedAt` or a matching monotonic observation.

The platform supplies RFC 3339 `observedAt` and half-open civil-day intervals. Each interval must last between 23 and 25 hours, inclusive, to allow daylight-saving changes, including half-hour changes. Intervals cannot overlap, and exactly one interval must contain `observedAt`. Core does not infer a time zone or prove that an interval matches a specific local calendar. Completion counts use each history item's `completedAt` timestamp; the history validator rejects a completed item without one.

PWA may supply `monotonic` instead of null:

```json
{
  "nowMs": 5000.375,
  "continuityId": "browser-session",
  "anchor": {
    "timerId": "timer-one",
    "anchorAt": "2026-03-08T07:29:00Z",
    "elapsedAtAnchorMs": 120000,
    "sampledTrustedNowMs": 1772955000000,
    "sampledMonotonicMs": 4999.875,
    "continuityId": "browser-session"
  }
}
```

The monotonic anchor must match the current running timer and continuity identity. A mismatch or backward monotonic reading uses `observedAt` instead. A matching anchor protects projection from a wall-clock jump. Core keeps fractional milliseconds for the readout and rounds `observedElapsedMs` to the nearest integer for a wire command. Non-PWA profiles reject monotonic observations.

## Output

The output has fixed top-level fields `schemaVersion`, `canonical`, `display`, `availableIntents`, `cadence`, and `tasks`.

```json
{
  "schemaVersion": 1,
  "canonical": {
    "timerId": "timer-one", "phase": "focus", "status": "running",
    "plannedDurationMs": 1500000, "elapsedMs": 180000.5,
    "remainingMs": 1319999.5, "deadlineAt": "2026-03-08T07:52:00Z",
    "progress": 0.12000033333333333, "observedElapsedMs": 180001
  },
  "display": {
    "phase": "focus", "status": "running", "plannedDurationMs": 1500000,
    "elapsedMs": 180000.5, "remainingMs": 1319999.5,
    "progress": 0.12000033333333333, "remainingSecondsCeil": 1320
  },
  "availableIntents": ["pause", "finish", "cancel", "cancelAndClear", "selectPhase"],
  "cadence": {
    "completedFocusToday": 0, "completedFocusTodayPlannedDurationMs": 0,
    "completedFocusTotal": 0,
    "longBreakProgress": 0, "nextCompletedFocusBreakPhase": "short_break",
    "skipDestination": "short_break"
  },
  "tasks": {"total": 0, "selectedTaskId": null, "completedFocusTodayByTask": {}}
}
```

When no timer exists, `canonical.timerId`, `canonical.phase`, and `canonical.deadlineAt` are null. Its status is `idle`, and its duration, elapsed, remaining, progress, and observed elapsed are zero. The `display` object uses the selected phase duration for idle or completed timers. Apple also uses an idle selected-phase display for cancelled and superseded timers. Android and PWA keep the completed status with a reset clock. A completed canonical timer reports full planned elapsed and zero remaining. The workspace boundary rejects underelapsed standalone completion before producing history. Desktop Terminal preserves the canonical timer separately from its next-interval display. Both Desktop profiles display zero elapsed for cancelled timers while retaining the canonical elapsed. `deadlineAt` is non-null only for a running timer.

`availableIntents` is an ordered list of Core-eligible timer and phase actions based on the projected timer and profile. The list can contain actions without separate visible controls. `finish` maps to the separate completion-command path. On PWA, the active Cancel button maps to `cancelAndClear`, while the terminal clear button stops the alert sound and does not map to a timer intent. PWA terminal timers therefore offer Start but not `clear`. Apple `skip` maps to selection of `cadence.skipDestination`. Both Desktop profiles offer `start` only at idle and `restart` on completed, cancelled, and superseded timers. The Desktop GUI and Terminal primary actions call `queue_restart`, which stores an atomic `clear` then `start` pair. `workspace.intent.v1` supports `restart` for both Desktop profiles and requires the presented timer as `requestedTimer`. Its lower-level `desktopStorage` Start admission can accept a terminal timer, but the read model does not expose that action for the Desktop GUI. `availableIntents` does not include transport, bootstrap, offline, alert, or other platform readiness gates. Adapters apply those gates to controls without changing Core's timer policy.

`skip` chooses a long break after 3, 7, or 11 focus completions today. Completion cadence chooses a long break at positive multiples of four. `longBreakProgress` is zero at zero completions, then cycles from 1 through 4. `cadence.completedFocusTodayPlannedDurationMs` sums the planned duration of every completed focus session in the supplied reference day, including deleted-task and unassigned sessions. It uses the same half-open day interval as `completedFocusToday`. For the shared fixture, the global total is 1,320,000 ms, while the current task row is 900,000 ms; yesterday's 600,000 ms is excluded from both daily totals. `completedFocusTodayByTask` maps each current projected task ID to `{count, plannedDurationMs}`. It includes a zero row for a task without a completion. Deleted tasks and unassigned completions do not appear in the current task map. Core uses projected history task identity after a retarget. Apple's separate all-time history summaries and Android's completed-focus breakdown remain distinct from these daily task rows.

## Parity and limits

`tests/read_model.rs` covers Skip counts 0 through 12, 23-hour and 25-hour day edges, countdown, paused and terminal states, PWA fractional and backward-wall observation, readiness by profile, rejected projection results, daily global and task totals, Desktop restart pairing, retarget attribution, and malformed input. `scripts/read_model_source_probe.py` executes the Desktop GUI primary action, Desktop timer and task methods, PWA methods, and Apple's extracted `AppStatePublisher.dayFocusTotals` method against the shared fixture. The probe stubs persistence and platform state, not restart selection or duration arithmetic. Apple and Android source predicates cover other branches. Swift runtime parity for the complete app and Kotlin runtime parity remain unverified.

Remaining CORE-M04 work includes client adapters, native Swift and Kotlin runtime parity, mapping platform readiness gates, terminal presentation under Apple's Iroh recovery path, and any product-policy alignment across profiles. Distinct all-time history breakdowns for deleted and unassigned tasks are not daily current-task rows. This operation does not change `workspace.intent.v1`, `timer.completionState.v1`, or stored wire structures.
