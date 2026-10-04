# Raw legacy preferences migration

`workspace.legacyPreferences.v1` imports PWA preferences with the original migration's zero-clock precedence. It does not represent a new user intent.

The independent CORE-PWA11 reproduction shows why this distinction matters. The current PWA duration importer calls `workspace.intent.v1` with `setDuration`. That call assigns a current HLC to a legacy 30-minute preference. A production HTTP merge then replaces the newer remote 35-minute preference with 30 minutes. The original migration in server commit `50c86a2` emits an epoch operation with HLC `0/0`. The same HTTP merge ignores that operation as superseded and retains 35 minutes.

The new operation is additive and ships in Core `0.46.0`. The PWA adapter has not adopted this operation. The verification counts below describe the earlier native milestones; [the release record](RELEASE_0_46.md) defines the current publication scope.

## Request

`fixtures/legacy-preferences-v1.json` contains a complete request. Every request field is required. Control objects reject unknown fields and array representations.

- `profile` is the string `pwaStorage`.
- `settings` is the original JSON object from the settings record. Core reads the legacy values and explicitness markers directly. Unknown settings remain unchanged.
- `workspace` contains the original canonical `base`, all five complete `local` queues, `neverSent`, `canonicalHead`, `timerDependencies`, `displayContext`, and `now`.
- `workspace.displayContext` contains the string profile `pwaStorage` and the original nullable `projectionPending` record. Stored members must match retained records exactly, including extensions and omitted fields.
- `ownership` contains both `ownerId` and `expectedOwnerId`. Each identity is null or a nonempty string. Both identities must match.
- `deviceId` is the nonempty persisted local device identity.
- `identities.operationUuids` contains at most five distinct lowercase UUIDv4 candidates supplied by the host. Core assigns candidates only to generated operations.
- `outgoing` is the complete nullable saved claim. Its original body, payload, metadata, field presence, and extensions remain unchanged. A nonnull claim must belong to `ownership.ownerId`.

The workspace's five queue names are `commands`, `taskOperations`, `durationOperations`, `autoStartOperations`, and `selectedTaskOperations`. A missing device field in a retained preference operation uses `deviceId` only in Core's temporary projection copy. Returned retained records preserve their original fields. Existing invalid timestamps fail validation instead of receiving an epoch replacement.

## Numeric JSON decoding

The migration has a bounded decoder in `src/legacy_preferences/json.rs`. The existing strict parser first checks syntax, duplicate keys, recursion depth, and number range. The migration decoder then reads the original `RawValue` tokens. It does not consume the strict parser's approximate floating-point values.

Integer tokens retain their integer representation. Decimal and exponent tokens use Rust's correctly rounded decimal-to-binary64 conversion before preference coercion. The decoder applies to unknown settings, retained extensions, and outgoing objects as well as duration values. A numeric `90.49999999999999` therefore remains below `90.5`, matching JavaScript's JSON decoder. The original migration produces 5,400,000 milliseconds for that value in every phase.

The migration's success envelope embeds the serialized result directly. A second approximate JSON parse would otherwise change preserved numeric fields after the migration has finished. Error envelopes retain the existing representation. Other operations retain their previous decoders and envelope handling. The `serde_json` Cargo features remain unchanged.

This repair changes decoding, not duration quantization. It does not rewrite input tokens, replace expected outputs, add an epsilon, or adjust a duration around a rounding boundary. Decimal HLC tokens such as `1.0` and `1e0` still fail integer validation. Unsafe integers, duplicate keys, and invalid raw control shapes still fail closed.

## Legacy duration behavior

Core visits `focus`, `short_break`, and `long_break` in that order. Unknown phase keys do not generate operations. Missing or null values do not generate operations.

For each present value, Core reproduces the original `Number(value)` conversion, nonfinite fallback to one, clamp to the inclusive range 1 through 180, and `Math.round` quantization. The result is multiplied by 60,000. Conversion includes numeric strings, booleans, JSON array coercion, decimal exponents, hexadecimal, binary, octal, and ECMAScript whitespace. Invalid text and ordinary objects fall back to one minute. A JSON object that shadows `toString` cannot convert and produces an error, including inside an array.

An operation is omitted when its quantized duration equals the legacy default. Those defaults are 25 minutes for focus, 5 minutes for a short break, and 15 minutes for a long break. Defaults are not compared with the remote projection. Thus `24.5` focus minutes rounds to the default and produces no operation, even when the remote duration is 35 minutes.

Each generated duration operation has exactly these fields:

```json
{
	"id": "11111111-1111-4111-8111-111111111111",
	"ownerId": "bootstrap",
	"phase": "focus",
	"durationMs": 1800000,
	"occurredAt": "1970-01-01T00:00:00.000Z",
	"hlcWallMs": 0,
	"hlcCounter": 0
}
```

Core removes `durations` and sets `durationSyncBootstrapped` to true after processing the domain. An existing true marker skips that domain and preserves its settings exactly. Core does not migrate or rewrite an older `pendingDurationOperations` settings queue. That record remains unchanged.

## Explicit flags and selection

Auto-start migration follows duration migration. Core emits an operation when `autoStartBreaks` is exactly true or `autoStartBreaksExplicit` is exactly true. The operation's `enabled` field is true only when `autoStartBreaks` is exactly true. An explicitly false preference therefore emits an operation even when the projected default is already false. Core removes both legacy fields and sets `autoStartSyncBootstrapped` to true.

Selection migration follows auto-start migration. A nonempty string `selectedTaskId` emits a selection operation, matching the original importer. Missing, null, empty, and nonstring values without explicitness emit no operation. Core removes `selectedTaskId` and `selectedTaskIdExplicit`, then sets `selectedTaskSyncBootstrapped` to true.

`selectedTaskIdExplicit: true` adds an explicit-null contract for callers that retained deselection intent. With that marker, null emits an operation despite matching the default. A missing, empty, or invalid selection value produces an error. This explicit-null case extends the old importer, which had no deselection marker and skipped null.

Both flag domains retain the same epoch timestamp and HLC `0/0`. Their operations contain `id`, the domain value, `occurredAt`, `hlcWallMs`, and `hlcCounter`. They do not contain `deviceId` or duration ownership fields. A true bootstrap marker skips its domain without stripping fields.

## Result and persistence contract

The result contains the following fields:

- `schemaVersion` is 1. `outcome` is `planned` when settings or queues change, otherwise `noop`.
- `settings` is the complete resulting settings object. `writeSettings` identifies whether the settings record changes.
- `operations` contains only newly generated records in the five named queues. `operationIds` contains their identities in the same order.
- `consumedIdentityCount` counts generated operations. Marker-only work and repeated migrations consume no identities.
- `workspace` contains the original base and retained queues with generated records appended. Only generated identities receive new `neverSent` proof.
- `projection` is Core's validated display projection. Original display membership is fixed before imports are appended. An installed head therefore prevents a newly imported epoch operation from overwriting the remote display.
- `outgoing` is the original claim. `outgoingAction` is always `preserve`.
- `effectsAfterCommit` contains `launchSync` when Core creates operations. Otherwise it is empty.

The host commits new queue members, settings, delivery proof, and changed display context in one guarded transaction. The host keeps the original canonical snapshot, HLC allocation, device sequence, timer dependencies, ownership records, and saved claims. A transaction failure rolls back the entire import. A restart with the returned markers is an exact no-op.

Core does not coalesce, retire, normalize, or reconstruct existing operations. Identity collisions fail instead of replacing a record. Saved claims without bodies remain without bodies. Successful preference migration gives no permission to reconstruct or send such a claim.

## Error boundaries

Duplicate JSON keys fail before decoding, including keys in retained extensions and saved bodies. Invalid control shapes, unknown controls, missing raw records, malformed UUIDs, duplicate candidate identities, and insufficient candidates produce errors. Retained queues still pass the existing workspace validation for identities, clocks, payloads, dependencies, and display membership.

Wrong account ownership and wrong outgoing ownership produce errors. An outgoing identity cannot also appear in never-sent proof. Core checks `sent`, `payload`, `queueIds`, and the original saved `body` independently. A malformed saved body produces an error without a reconstruction plan. All error results contain no persistence instructions.

## Verification evidence

`scripts/legacy_preferences_source_probe.cjs` executes both the frozen pre-port migration methods and the actual server methods from commit `50c86a2`. The probe uses the production storage entrypoints with fake IndexedDB. It captures complete persisted inputs, verifies JSON decode equality, compares complete flag returns and all persisted queue records, and verifies restart and transaction rollback.

The source probe also executes the current public PWA duration importer. Its current-HLC operation supplies genuine behavior-red evidence. The native operation's initial unsupported dispatch is only a baseline implementation gap, not that behavior-red proof.

`scripts/legacy_preferences_http_test.go` runs through a Go test overlay without editing server files. The test uses production SQLite storage and authenticated Go HTTP handlers. It sends the current public operation and the native migration operation to separate remote-35-minute accounts. The native operation receives `ignored` with reason `superseded by newer duration operation`. The test checks the winning SQLite row using the authoritative HLC, device, and identity ordering.

The HTTP probe also sends the native explicitly false auto-start operation and the explicit-null selection operation. The newer remote true preference wins over legacy false. The explicit-null operation reaches the server and receives an applied acknowledgement even though the selected task is already null.

The aggregate host corpus requires 14 legacy semantic vectors, two stateful restart vectors, and 21 raw legacy rejection vectors. Output mutants cover clock precedence, display precedence, explicit false, explicit null, default quantization, invalid fallback, immutable timestamps, saved body preservation, and restart markers. Runner mutants reject skipped restart and error dispatches.

The numeric repair adds ten required semantic vectors and nine required rejection vectors. Numeric output mutants cover each phase, unknown preferences, retained extensions, outgoing `sent` extensions, and outgoing metadata. Runner mutants reject skipped numeric success and error dispatches. The combined host gate, native corpus, and runner checks pass 113 tests.

`scripts/legacy_preferences_numeric_probe.cjs` starts from the exact independent checker receipts. A preserved pre-repair executable fails nine tests, including the original numeric mismatch and raw-retention mismatches. The repaired executable passes all 27 source tests. The probe executes commit `50c86a2` migration methods against fake IndexedDB for numeric neighbors and string controls in all three phases.

The same probe executes 3,759 boundary cases through the original Git migration methods. The pre-repair executable has 114 duration quantization failures. Adding numeric unknown settings exposes 504 preservation failures. The repaired executable has zero quantization failures and zero preservation failures. The evidence contains the original inputs, original returns, complete native envelopes, and host-decoded values. The no-op case also verifies the records read back from IndexedDB.

`scripts/legacy_preferences_baseline_gate.mjs` preserves all 1,077 existing official 0.45 envelopes before the repair. The capture verifies byte equality with the downloaded official artifact. The post-repair native comparison has zero byte differences across those envelopes and 20 extra numeric and error-precedence controls. The capture refuses to replace an existing baseline file.

Evidence files use the `PWA11_EVIDENCE_DIR` directory. The default is `/var/folders/r_/_mr22dqn24d31b7460cz8z5m0000gn/T/opencode`. They are `core-pwa11-source-parity.json`, `core-pwa11-http-input.json`, and `core-pwa11-http.json`.

Numeric recheck evidence uses the same directory. The files are `core-pwa11-numeric-source-red.json`, `core-pwa11-numeric-source-green.json`, `core-pwa11-numeric-sweep-red.json`, `core-pwa11-numeric-sweep-green.json`, `core-pwa11-preserved-envelopes.json`, and `core-pwa11-preserved-envelope-gate.json`. `PWA11_NATIVE_ORACLE` selects the preserved pre-repair executable for the red run. The preservation gate's default mode compares the current native executable with the saved baseline. Its `--capture` mode is only for the pre-repair capture.

All native checks use Rust 1.97.1. The toolchain's bin directory must precede Homebrew in `PATH`, and `RUSTC` and `RUSTDOC` must name that toolchain explicitly. The two excluded tests are `wasm_abi_handles_malformed_ranges_and_cleanup_in_a_real_host` and `c4_release_wasm_rejects_oversized_allocations_without_trapping`. Both build local WASM. The aggregate native oracle and transfer-only host runner execute without a local WASM build.

The suite size audit reports zero violations and no new exceptions. The production reports add 28 Core entities for the migration contract, raw validation, projection conversion, legacy numeric coercion, and bounded numeric decoding. Core's cyclomatic mean changes from 3.50 to 3.53, and its cognitive mean changes from 2.80 to 2.85. Both p95 values remain 9. The increase reflects the fixture-backed migration and error cases. Existing production functions gain the additive dispatch branch, migration-only envelope handling, and UUID validator visibility. The other projects' production fingerprints remain identical to the initial report.
