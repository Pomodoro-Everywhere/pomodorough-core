# Legacy dependency plan v1

`workspace.legacyDependencyPlan.v1` reconstructs local timer dependency metadata from complete persisted records. It is a separate member of the workspace migration family because dependency provenance does not allocate preference operations.

The operation is additive and ships in Core `0.46.0`. The PWA and Android adapters do not call the new operation yet. The verification counts below describe the earlier native milestones; [the release record](RELEASE_0_46.md) defines the current publication scope and independent acceptance.

## Request

`fixtures/legacy-dependencies-v1.json` contains eight complete production IndexedDB receipts and their native returns. Every root field is required.

- `profile` is the string `pwaStorage` or `androidCentralized`.
- `workspace` contains the original canonical `base`, all five `local` queues, `canonicalHead`, `neverSent`, `displayContext`, `timerDependencies`, and `now`.
- `workspace.timerDependencies` is the raw nullable metadata record. Null permits reconstruction from retained legacy fields. An array is authoritative, including an empty array. Core never resurrects an old dependency field after metadata has explicitly discharged it.
- `ownership` contains nullable `ownerId`, nullable `expectedOwnerId`, and nullable `timerOwner`. Account identities must match. Timer ownership remains unchanged.
- `deviceId` is the persisted local device identity. Android entity records may omit their request-level device identity. Core decorates only its validation copy.
- `outgoing` is the complete nullable saved request. Its owner must match. `sent`, `payload`, `queueIds`, and the original `body` receive separate validation when present. A missing saved body returns blocked recovery. A body must contain an explicit nonempty string `deviceId` matching the persisted request device, recognized operation queues, and every declared claim and source acknowledgement. An empty or redacted object cannot hide cached source commands. Cached commands never supply a missing body identity.
- `calendarIntervals` contains platform-provided objects with `start` and `end` timestamps. Intervals must not overlap and must last at most 26 hours. Core selects the interval containing the exact source completion. It does not calculate a local date.
- `sourceAcknowledgements` contains stored acknowledgements with `commandId`, string `outcome`, and optional nullable `reason`. Outcomes are `applied`, `ignored`, or `rejected`. Each identity must have a matching saved command. Duplicate and phantom identities fail validation.

The five queue names are `commands`, `taskOperations`, `durationOperations`, `autoStartOperations`, and `selectedTaskOperations`. The input retains extensions, null values, empty values, and omitted fields.

## Legacy representations

The live PWA implementation in `server/web/sync-storage.js` reconstructs edges from `dependsOnCommandId`. A command with `generatedBreak: true` also receives a source-day range. When the source command is missing and no explicit range exists, the current implementation throws `Timer dependency has an invalid source time.`

The frozen PWA source in the fresh `pwa-core-045` receipts produces a generated Start with `dependsOnCommandId` pointing to the focus Finish. Later break controls copy that Finish identity, so they can form sibling edges. The current Core intent flow already uses direct-parent chains for future operations.

Android's `PendingCommandEntity.generatedByFinishCommandId` is local metadata. Its `toModel()` does not send that field. The `androidCentralized` profile recognizes this native representation and derives generated Start classification from the retained Start and focus Finish records. Native `neverSent` row flags, when supplied, must agree with the complete delivery proof.

Core upgrades a legacy sibling edge only when the child has `neverSent` proof and no saved request claims it. The internal edge then points to the last earlier command for the same generated timer and device. Core never changes the retained `dependsOnCommandId` or `generatedByFinishCommandId` field, even for proven operations. A possibly delivered sibling returns `possiblyDeliveredSibling` recovery.

## Source evidence and calendar validation

A retained source must replay to an exact completed focus with the same command identity, timer identity, and duration. Core uses its existing workspace replay. Valid persisted `physicalOccurredAt` observations enter only the private observed replay. They never replace wire timestamps or saved canonical history.

An existing source-day range is preserved when valid. Core checks its length and requires it to contain the exact completion. A missing range requires a matching platform interval. Cadence counts only focus completions causally at or before the source, including deterministic identity ordering for timestamp ties. Later completions in the same day do not affect that source's phase.

A source absent from the local queue requires all of the following before Core can discharge its edge:

1. Exact canonical completion evidence in stored history, or an exact completed canonical timer whose last intent is the same Finish.
2. The saved Finish command with the same source identity, timer identity, phase, and duration. Its wire `occurredAt` must equal the canonical completion instant. Every supplied `completedAt` and `endedAt` must agree with that instant.
3. A stored `applied` or `ignored` acknowledgement for that saved identity.
4. Valid causal ordering and a source-day range or platform interval containing the completion.
5. The existing generated Start payload matching the canonical phase and duration.

Core does not fabricate a missing parent command. Canonical-timer-only evidence remains a private observation and does not add persisted history or operation identities. Rejected sources and canonical payload differences require an explicit decision. This migration does not discard their queues.

Source matching follows the explicit Finish behavior in `timer::transition_session`, which assigns the command occurrence to both terminal timestamps. Comparison uses complete parsed instants, including nanoseconds. Equivalent offsets remain valid. A local `physicalOccurredAt` observation cannot replace the occurrence in a delivered request.

A known rejected source acknowledgement blocks migration whether the source is retained locally, absent, or available only through the saved body. An empty dependency array does not erase that rejection. Blocked recovery preserves all wire rows and returns no metadata writes on reopen.

Core checks phase, duration, and elapsed normalization for every possibly delivered member of a generated batch. Duration admission uses the canonical base, so an unacknowledged duration operation cannot hide or create a frozen payload conflict. A mismatch returns `possiblyDeliveredPayloadDecisionRequired`. When the batch contains a Finish, the normalization target is the generated Start's preserved phase and duration. Every member still receives the phase, duration, and elapsed checks. Finish presence never permits rewriting a frozen Pause or Finish. Proven members may normalize later through reconciliation while this migration retains their original records.

These checks validate persisted request evidence within the declared account and profile. A syntactically valid body does not establish network authorization or permission to send, replace, or discard a saved request.

## Result

The result has `schemaVersion: 1` and these fields:

- `outcome` is `planned`, `noop`, or `blocked`.
- `workspace` is the complete original workspace. A successful plan changes only `timerDependencies`.
- `timerDependencies` is the complete valid array on success and null when blocked.
- `validatedDependencies` contains individually validated candidate edges. On blocked results, this field is diagnostic and cannot be installed as a complete graph.
- `metadataWrites` contains at most one `recordTimerDependencies` write. Blocked results contain no writes.
- `classifications` identifies each validated edge, its original record, its resulting metadata, and its preservation decision. Generated edges include the exact completion time and source phase after completion.
- `ownership` and `outgoing` preserve the complete inputs. `outgoingAction` and `wireAction` are always `preserve`.
- `recovery` contains `status`, `blocksSync`, `blocksMutations`, `automaticRepair`, and `unresolved`. A blocked result sets both block flags and never authorizes automatic repair. Each unresolved dependency includes the exact command, dependency record, source identity, and available source evidence. A missing saved body includes the complete outgoing record.
- `effectsAfterCommit` is empty. This migration allocates no identity and requests no network effect.

The adapter atomically writes only the returned complete metadata when `outcome` is `planned`. It retains all queues, wire fields, canonical records, proof, allocation state, owner records, and saved body bytes. A blocked result supports a visible recovery state without destroying the saved queue. Reopen returns the same block or a metadata no-op.

Malformed representations return an error envelope with no persistence instructions. Incomplete provenance returns successful `blocked` recovery rather than claiming full resolution. Core rejects positional records, enum objects, extra root controls, duplicate keys, invalid clocks, bad timestamps, invalid proof, cycles, duplicate edges, and stale ownership.

Source contradictions, including inconsistent canonical and saved occurrence times, remain defensive boundary errors. The adapter must present those errors as visible workspace recovery and preserve the request and every queued record. An error envelope is not permission to discard the request or silently continue. Successful `blocked` results use the explicit recovery fields for the same retention behavior.

## Verification

`scripts/legacy_dependencies_source_probe.cjs` executes the actual frozen production Finish implementation against IndexedDB. It reads every persisted input back before native dispatch, compares complete returns, and checks rollback and reopen. The all-five-queue scenario uses actual records from the fresh `pwa-core-045/source-parity.json` receipts. The missing-parent case reproduces the current live error before verifying blocked Core recovery.

The fixture captures complete old creation returns, complete old and current storage reads, raw native requests, complete native envelopes, persisted inputs and outputs, and complete restart returns. Its capture mode refuses to replace an existing fixture. Normal mode compares all receipts with the fixed fixture.

The aggregate artifact corpus includes exact full-return success and blocked cases, raw representation failures, restart cases, and actual reconciliation of applied, ignored, and rejected source acknowledgements. A rejected source drops only proven descendants through the existing reconciliation operation. The same drop fails when a descendant is possibly delivered. These cases run in each hosted artifact pass, with raw envelope parity and memory checks.

The local checks use Rust `1.97.1` and exclude only `wasm_abi_handles_malformed_ranges_and_cleanup_in_a_real_host` and `c4_release_wasm_rejects_oversized_allocations_without_trapping`. Both excluded tests build local WASM. New hosted-WASM execution and official publication remain required.

Native verification passes 624 tests, including 28 dependency integration tests and a decoder-field guard. All 617 earlier native tests remain present and pass. The production source probe passes all eight original cases without refreshing their complete-return fixture. The aggregate native oracle passes 1,867 cases. All 1,846 earlier cases remain present with byte-identical envelopes. Dependency coverage includes the nine initial checker reproductions, six residual cases, and twelve residual migration, reopen, and reconciliation cases. The Node host and runner gates pass 144 tests, and the Python gates pass 80 tests.

The first independent checker passed 61 of 70 cases and identified nine failures across four boundaries: source time consistency, retained rejected acknowledgements, saved body completeness, and frozen durations. `fixtures/legacy-dependency-checker-v1.json` preserves those original raw requests and complete native envelopes. The unchanged 70-case checker now passes locally. Independent acceptance of the repair remains open.

`scripts/legacy_dependencies_checker_gate.mjs` verifies the nine fixed inputs and exact blocked reopen returns. Its capture mode refuses to replace an existing baseline. The red capture ran before the repair and matched every original checker envelope. The green receipt contains both original and corrected envelopes. `scripts/legacy_dependencies_independent_recheck.cjs` runs the original checker source unchanged and redirects only its output receipt, preserving the original rejection evidence.

The evidence directory is `PWA10_EVIDENCE_DIR`, defaulting to `/var/folders/r_/_mr22dqn24d31b7460cz8z5m0000gn/T/opencode`. The repair receipts are `core-pwa10-checker-red-receipts.json`, `core-pwa10-checker-green-receipts.json`, and `core-pwa10-independent-green.json`.

The residual recheck found six failures across two boundaries. A finished batch skipped frozen descendant validation, and a complete body without a request-level device identity discharged its source. `fixtures/legacy-dependency-residual-v1.json` preserves the six exact requests and original results. `scripts/legacy_dependencies_residual_gate.mjs` verifies migration, reopen, and actual `reconcile.rebase.v3` composition. The red packet predates the repair. The green packet shows blocked recovery, no writes, unchanged wire records, and the existing reconciliation error for the claimed descendant.

The residual packets are `core-pwa10-residual-red-packet.json`, `core-pwa10-residual-red-composition-packet.json`, and `core-pwa10-residual-green-packet.json`. `scripts/legacy_dependencies_residual_recheck.cjs` executes the original checker sources without editing them and redirects their receipt outputs. The local recheck passes all 100 inputs and 79 reopens, plus 20 actual reconciliation executions. The new tests cover intact body identities, proven normalization, finished Start payload preservation, reducer-ignored terminal fields, and raw numeric extension retention during migration. Final independent acceptance remains open.

`scripts/legacy_dependencies_preservation.mjs` confirms byte equality for all 1,707 pre-PWA10 envelopes. `scripts/natural_completion_baseline_gate.mjs` also compares the original 1,077 official 0.45 envelopes and 20 numeric controls. The ten already documented CORE-PWA12 errors for malformed raw representations remain the only differences. Dependency migration adds no difference to existing operation envelopes.

The size audit reports zero violations and no new exception. The first checker repair added seven production entities for exact source matching, rejected acknowledgement recovery, saved body consistency, and batch-wide frozen payload admission. The residual repair adds no production entity. Rounded cyclomatic mean remains 3.63, cognitive mean remains 3.01, and both p95 values remain 10. The changed decisions implement the six permanent residual cases and their composition controls. Other projects' production fingerprints remain unchanged.

The PWA can retire its live legacy dependency reconstruction after it adopts a verified official artifact containing this operation. The adapter must call the new contract before its current reconstruction fallback, supply raw records and platform intervals, commit metadata atomically, and render blocked recovery. Missing provenance remains a user-decision case after adoption.
