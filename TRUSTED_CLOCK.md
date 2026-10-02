# Trusted clock observations

`clock.observe.v1` is a pure clock transition boundary. It accepts raw adapter readings and saved clock state. It returns clock state, trusted time, and optional local anchor translation. It does not read the OS, make network requests, tick an HLC, allocate identities, or mutate a workspace.

This is a bounded CORE-M05 addition. Existing workspace endpoints and canonical wire timestamps keep their contracts. Client adoption, complete physical snapshot translation, and native timer elapsed continuity remain separate work.

## Envelope

The request has these fields:

```json
{
	"schemaVersion": 1,
	"compatibility": "androidTrustedClock",
	"action": "current",
	"state": {
		"serverClockOffsetMs": 100,
		"serverClockUncertaintyMs": 1,
		"serverClockSamplePhysicalMs": 1000000,
		"serverClockSampleElapsedRealtimeMs": 20000,
		"serverClockBootId": "boot-a",
		"retainedWallMs": 1000100,
		"anchor": null,
		"requestSample": null
	},
	"reading": {
		"wallMs": 1000003,
		"monotonicMs": 20003,
		"bootId": "boot-a"
	},
	"trustedAnchorMs": 1000999
}
```

`schemaVersion`, `compatibility`, `action`, and `state` are required. Other fields are optional unless the selected action needs them. Unknown envelope, reading, server, and typed state fields fail. Reading and server keys must belong to the selected profile, even when their values are `null`. Duplicate object keys fail at every depth.

The supported profiles are `appleTrustedClock`, `androidTrustedClock`, `desktopTrustedClock`, and `pwaTrustedClock`. They preserve different production recovery policies. A profile does not infer another profile from its fields.

The adapter supplies observations rather than `nowMs`, elapsed time, a midpoint, an uncertainty estimate, or a reboot decision. Integral readings use JSON integer tokens. Epoch milliseconds are decoded physical or network timestamp readings, not caller-adjusted trusted time. `retainedWallMs` and `minimumWallMs` are existing persisted lower bounds, not HLC results computed by this endpoint.

Integral clock readings and persisted clock values fit JavaScript's safe integer range. Android and Desktop monotonic tokens must be literal JSON integers in `0...9007199254740991`; decimal and exponent tokens fail before reading conversion. This includes request and receipt observations. Epoch clock readings are positive except Android's existing local and continued-time zero allowance. Offsets are signed safe integers. Uncertainty is between 0 and 30000 ms. Reboot skew is 300000 ms. Desktop continuity drift is 1000 ms. Physical anchor outputs follow native mapper ranges rather than the positive wire clock range.

Every successful output includes `schemaVersion`, `compatibility`, `state`, `trustedNowMs`, `physicalDeltaMs`, and `physicalAnchorMs`. Unavailable scalar results are `null`. Android also returns `sample` and `sampleStale`. Desktop also returns `sample` and `trustedResponseMs`. Apple also returns `occurrenceWallMs`, `trustedDateSeconds`, and `physicalAnchorSeconds`.

`trustedAnchorMs` is an optional raw decoded wire anchor. Apple also accepts `trustedAnchorSeconds`, a raw Foundation Date Unix reading. The two anchor keys are mutually exclusive, including null-valued keys. Other profiles reject `trustedAnchorSeconds`. Core never returns a rewritten canonical timer or history record from this operation.

The reading fields are profile-specific:

- Apple: `wallSeconds`, `uptimeSeconds`.
- Android: `wallMs`, `monotonicMs`, `bootId`.
- Desktop and PWA: `wallMs`, `monotonicMs`.

## Server observations

`sample` requires `server`. Android's `advance` also requires `server`. Other actions reject a supplied server observation.

The server object supports these raw observations:

- `serverTimeMs`: decoded server physical timestamp.
- `serverHlcWallMs`: optional observed server HLC wall value. Android requires it. A supplied value must be within 300000 ms of the server timestamp.
- `requestWallMs`: physical wall time at request dispatch. Desktop may omit it only in the all-timings-absent response path.
- `responseWallMs`: physical wall time at receipt. Android and PWA require it. Desktop requires it when any receipt timing is supplied. Apple rejects it.
- `requestMonotonicMs` and `responseMonotonicMs`: integral Android or Desktop monotonic readings. Apple and PWA reject these keys.
- `requestUptimeSeconds` and `responseUptimeSeconds`: fractional Apple uptime readings. Other profiles reject these keys.
- `requestSequence`: PWA's positive raw request sequence. Other profiles reject this key.

Apple uses only uptime for round-trip duration. PWA uses only wall readings. Android includes wall versus monotonic disagreement in uncertainty and tolerates bounded wall reversal. Desktop rejects wall reversal and disagreement above 1000 ms before computing uncertainty.

## Apple state and actions

Apple state consists of nullable `offsetMs`, `uncertaintyMs`, `anchorMs`, `anchorUptime`, and `lastEmittedMs`. `anchorUptime` remains in seconds, matching `TrustedClockState.swift`. `reading.wallSeconds` is a raw Foundation Date Unix reading, and `reading.uptimeSeconds` is raw uptime.

- `sample` returns the resampled state and preserves `lastEmittedMs`. Half-trip milliseconds truncate toward zero for the midpoint and round upward for uncertainty.
- `current` returns an occurrence date and leaves state unchanged.
- `advance` computes that occurrence and records its native physical millisecond value in the returned state. It is the commit-side transition, not an HLC tick.
- `restore` is unsupported. Apple performs its existing uptime recovery during `current` or `advance`.

An entirely absent sample uses the local Date. Partial sample state and last-emitted-only state fail closed. Same-uptime continuity ignores wall jumps. Lower uptime uses wall plus offset, bounded by the greater of the anchor and last emission plus 300000 ms. A candidate at or before the last emission advances the occurrence by one millisecond.

Foundation stores Date seconds relative to 2001. Core preserves the resulting binary rounding when converting to Unix seconds and when recording or translating milliseconds. `trustedNowMs` is the integer occurrence candidate. `trustedDateSeconds` is the native Date result and is authoritative for a Date-based adapter. `occurrenceWallMs` is the native Date's truncated physical millisecond value for wire generation and independent HLC input. It is `null` if that Date cannot represent a supported physical value. For example, the 1000101 ms candidate records as 1000100 ms at the fixture's early Unix epoch. The compiled Swift probe verifies the actual source candidate, truncated occurrence, state difference, and exact returned seconds.

The physical mapper uses the same Date conversion as `TrustedClockState.physicalDate`. It returns a separate anchor. Apple's `PersistedTimerState.physicalCanonicalTimer` translates only `anchorAt` and preserves `lastIntent`.

`trustedAnchorSeconds` preserves the fractional Date when no sample exists. With a sample, Core performs the native Date-to-milliseconds conversion, applies the offset, validates the resulting native physical range, and constructs the native Date result. No adapter pre-rounding or bypass is needed. The nearest-millisecond `physicalAnchorMs` view is nullable when the returned Date cannot fit Int64. The authoritative `physicalAnchorSeconds` remains available for unsampled finite Dates, including pre-epoch and extended-range Dates.

Apple's temporary request midpoint may exceed the JavaScript-safe bound. Its native method checks Int64 overflow and then bounds the final offset and anchor. For request wall `9007199254740990`, server time `1000000`, and uptime `20` to `20.004`, the verified output is offset `-9007199253740992`, uncertainty `3`, and anchor `1000002`.

## Android state and actions

Android state consists of nullable `serverClockOffsetMs`, `serverClockUncertaintyMs`, `serverClockSamplePhysicalMs`, `serverClockSampleElapsedRealtimeMs`, and `serverClockBootId`. It also includes `retainedWallMs`, an optional runtime `anchor`, and an optional raw `requestSample`.

`anchor` has `serverTimeMs` and `elapsedRealtimeMs`. `requestSample` has `offsetMs`, `uncertaintyMs`, `serverTimeMs`, `midpointPhysicalMs`, and `midpointElapsedRealtimeMs`.

- `sample` returns the measured sample without installing it or changing persisted state. Physical midpoint division truncates toward zero, including a negative physical round trip. Monotonic midpoint division truncates downward.
- `advance` requires the previous raw `requestSample`. It advances that sample using the new receipt's monotonic observation, not the new server timestamp. The new server timestamp and HLC still undergo validation. Uncertainty never decreases during this advance.
- `restore` clears only stale persisted physical, elapsed, and boot fields. Continuity requires a non-null matching boot ID and nondecreasing elapsed time. Offset and uncertainty survive invalidation.
- `current` follows request sample, runtime anchor, persisted anchor, and bounded wall recovery in production order. Recovery clamps to `retainedWallMs` and subtracts uncertainty from the maximum allowed skew. The returned runtime anchor contains the actual recovered time.

`sampleStale` derives from raw monotonic age and uncertainty. Age exactly equal to `300000 - uncertaintyMs` is not stale. This flag does not change the production `now` method's acceptance policy.

Physical anchor translation follows `TimerRepository.translatePhysicalInstant` and Int64 arithmetic. It accepts valid pre-epoch Instants such as `-99` ms. The clock-reading bounds are not reapplied to this separate mapped anchor.

Startup orchestration calls `restore` before `current`, forwarding Core's returned state. Runtime anchors and request samples remain process-local. Sampling remains separate from transaction acceptance. The existing repository installs sample fields and ticks or merges its HLC after the response is accepted.

## Desktop state and actions

Desktop state has `sample`, optional runtime `anchor`, and optional `mode`. `mode` defaults to `monotonic`. It also supports `wall` and `local`, matching the existing read flags.

`sample` is `null` or has exactly `offsetMs`, `uncertaintyMs`, `acquiredPhysicalMs`, `acquiredMonotonicMs`, and `acquiredTrustedMs`. Runtime `anchor` has the same fields.

- `sample` with complete receipt timing returns the response sample as both saved sample and runtime anchor. Odd trips use the later receipt-side half for `acquiredTrustedMs`.
- `restore` clears malformed saved samples or broken wall versus monotonic continuity. Valid persistence installs the runtime anchor.
- `current` rejects malformed samples during trusted reads. A missing or changed runtime anchor first checks 1000 ms drift. An already-installed matching anchor ignores later wall jumps. Backward monotonic time falls back to the raw physical reading without clearing persistence.
- `advance` is unsupported.

`local` mode returns physical time without consulting the saved sample. `wall` mode applies the offset without monotonic continuity. Physical anchor translation includes the native formatter and timestamp parser roundtrip. It retains the original anchor when the translated epoch milliseconds are invalid or Python's datetime formatter cannot represent the date. Dates beyond year 9999 therefore preserve the original anchor, matching `_physical_timestamp`.

Desktop's four timing readings may all be omitted or all be null. In that path, the returned `sample` is null, saved sample and runtime anchor remain unchanged, and `trustedResponseMs` is the observed server timestamp. With complete timing, `trustedResponseMs` is the receipt-side `acquiredTrustedMs`. Partial timing fails closed. This matches the production `_response_clock_context` and preserves clock metadata during legacy no-timing installation.

The full Desktop `_physical_snapshot` translates the timer anchor, timer intent, and history timestamps in a separate projection. That whole-record translation is outside this endpoint.

## PWA state and actions

PWA state has `clockOffset`, optional `minimumWallMs`, and optional process-local `runtime`. `clockOffset` retains the raw saved value. Invalid saved samples are ignored for trusted time, matching `app-state.js`.

A valid `clockOffset` has `offsetMs`, `uncertaintyMs`, `sampledAtWallMs`, `requestSequence`, and `receivedAtWallMs`. Additional sample fields do not invalidate the legacy sample. `runtime` has `identity`, fractional `monotonicMs`, and `wallMs`.

- `sample` returns the wall-based sample from the production `sync-core.serverClockOffset` calculation. It preserves the old runtime until a subsequent current read decides whether its identity still matches.
- `current` computes the sample identity, seeds or resets runtime when necessary, and derives trusted time from raw `performance.now()`. Half-millisecond elapsed values round upward exactly as `Math.round`. A missing monotonic reading uses wall time and leaves runtime unchanged.
- `advance` and `restore` are unsupported.

The runtime identity includes offset, uncertainty, and sampled wall time. It does not include request sequence or receipt time. The minimum wall value clamps every successful read. Runtime state is not portable across browser lifetimes.

PWA timer anchors remain in the trusted domain. `physicalDeltaMs` is `0`, and an observed anchor remains unchanged. The production `OwnerStateProjector.normalizeTimer` proves this behavior. PWA does not subtract the server offset from canonical anchors.

The current production file has `serverClockOffset`, not `sampleServerTime`. The source probe loads that actual function and the actual `TrustedClock` class.

## Fractional JSON transport

Fractional observation fields and fractional continuity fields parse their raw numeric tokens with Rust's decimal-to-binary conversion. Duplicate-key validation still runs first. This avoids a one-ULP change from an intermediate `serde_json::Value` parse.

The dispatcher embeds this operation's already-serialized result directly in its success envelope. Existing operation envelopes are unchanged. This preserves fractional outputs through the native and ABI envelope path without enabling global float parsing changes.

`examples/read_model_probe.rs` forwards its `input` through `RawValue::get()` rather than a `Value` roundtrip. The bridge envelope and all callers retain their interface. The checked-in transport regression verifies that `1300.1000000238419` does not become `1300.100000023842` before dispatch.

## Verification and bounded migration work

`fixtures/clock-observe-v1.json` and `fixtures/clock-observe-checker-v1.json` are shared by native Rust tests and the production-source probe. Checker requests are stored as raw JSON strings. The probe compares complete returned clock state, samples, trusted results, physical anchor results, and native Date seconds. Native policy rejection cases must fail in both implementations. API-only rejection cases cover unrepresentable integral tokens and foreign observation keys and are reported separately. Four-step chains feed each implementation its own returned state.

Swift compiles the actual `TrustedClockState.swift`, `WirePrimitives` clock methods, and production uptime guards. Kotlin compiles the actual `TrustedClock.kt`, server skew guard, persisted sample validation, and `TimerRepository.translatePhysicalInstant`. Python executes extracted production clock methods, `_physical_timestamp`, `utc_timestamp`, `parse_timestamp_ms`, and `_response_clock_context`. Node executes the production TrustedClock class, loads `sync-core.js`, and calls the actual `OwnerStateProjector.normalizeTimer` through `app-state.create`. Model definitions and observation sources are supplied by the probe. Expected outputs are not substituted for production return values.

The repair started with eight failing checker Rust tests and one failing bridge transport test against the pre-repair code. The same checked-in regressions pass after the repair. Additional tests exercise 48 invalid token combinations across both integral profiles and all three monotonic observation positions, valid inclusive bounds, and null-valued foreign fields across every profile.

### Desktop oracle input fidelity

Desktop source verification stores raw `nativeObservations` beside each fixture request. These values are fixture metadata, not fields in the Core API. They include the decimal representation of the native wall seconds, integer monotonic nanoseconds, and the raw ISO anchor when an anchor is observed. A `NativeClockRequest` carries this metadata as an attribute outside its serialized dictionary.

Before invoking the production clock, the probe decodes each raw observation through the native adapter conversion or production timestamp parser. It asserts equality with the supplied Core `wallMs`, `monotonicMs`, and `trustedAnchorMs`. A missing observation or decoded mismatch raises an oracle assertion before policy evaluation. It cannot become an expected clock rejection or a restore that clears the sample. The original request values remain unchanged.

The production clock receives the raw wall and nanosecond providers. Current reads use `_trusted_now_ms` without an explicit physical override. The physical mapper receives the raw ISO string. The probe never reconstructs a raw observation with `wallMs / 1000`, `monotonicMs * 1000000`, or `utc_timestamp(trustedAnchorMs)`.

`fixtures/clock-observe-oracle-v1.json` adds two independently reproduced oracle failures. The pre-repair probe mapped decoded anchor `1001` to `900` instead of `901` and discarded a valid sample at the exact 1000 ms drift boundary. Both probe tests failed while the Core fixture test already passed. The repaired oracle uses raw ISO `1970-01-01T00:00:01.001500Z` and raw wall seconds `1.0015`, which each decode to the supplied `1001` ms before complete result comparison.

The same fixture also covers ISO `.001000` versus `.001500`, offset-equivalent ISO strings, pre-epoch timestamp decoding, neighboring wall floats, and the one-millisecond change between accepted and rejected restore drift. Boundary expectations exercise the production parser independently from Core. Tests also reject intentionally mismatched wall, monotonic, and anchor pairs and prevent metadata from entering a Core request.

This repair changes source verification only. Core clock implementation and Core request schema remain unchanged. The acceptance evidence applies to the recorded native observations after their decode preconditions pass. It does not establish packaged client adoption or every native observation on every runtime.

The following integration work remains:

1. Client adapters and atomic persistence adoption. No client calls this new endpoint yet. Android response installation still needs a composed transaction plan alongside independent HLC acceptance.
2. Whole-record physical translation. Apple intent metadata and Desktop history translation must remain separate from immutable wire records. This endpoint proves anchor arithmetic, not every physical snapshot field.
3. Timer elapsed continuity. Desktop `effective_timer_now_ms`, native timer uptime logic, and cross-endpoint clock-to-workspace composition still need raw observation integration. Existing PWA workspace monotonic handling remains independent.
4. Restart ownership of runtime state. Browser lifetime and process lifetime reset runtime anchors. The endpoint preserves current production policies rather than inventing persisted continuity IDs for Apple or PWA.
5. Release and packaged ABI evidence. Verification here is native and source parity. WASM compilation, packaging, release, and independent checker approval remain pending.

The native verification command excludes exactly the two tests that build WASM:

```sh
toolchain_bin="$(dirname "$(rustup which --toolchain 1.97.1 cargo)")"
export PATH="$toolchain_bin:$PATH"
export CARGO_INCREMENTAL=0
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --all-features --locked -- --skip wasm_abi_handles_malformed_ranges_and_cleanup_in_a_real_host --skip c4_release_wasm_rejects_oversized_allocations_without_trapping --exact
cargo build --locked --example read_model_probe
python3 scripts/trusted_clock_source_probe.py
python3 -m unittest discover -s scripts -p test_trusted_clock_probe_transport.py -v
python3 -m unittest discover -s scripts -p test_trusted_clock_source_oracle.py -v
python3 scripts/workspace_terminal_source_probe.py
```

The explicit toolchain path avoids a Homebrew `rustc` taking precedence over Rust 1.97.1. Disabled incremental compilation avoids reusing compiler caches from a different toolchain. The probe reports production-parity observations, native policy rejections, API-only rejections, and chained observations separately.

The size audit reports no violations and no new exceptions. The complexity report keeps both Core p95 values at 9. The small increase in Core's complexity means comes from the new profile-specific recovery and validation branches and the fractional-preserving envelope branch. Existing planners and terminal reducers are unchanged.
