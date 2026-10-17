# WASM test discovery

`wasm_test_suite.json` classifies every `tests/**/*.mjs` file. Discovery rejects missing files, unclassified files, duplicate entries, and unknown categories before executing any gate.

- `candidate` scripts accept one artifact argument. CI and release run every candidate script against the exact canonical WASM before upload or sealing.
- `historical` scripts compare native behavior with the digest-checked official v0.48.0 artifact. They run separately with their documented argument or environment and stdin contracts.
- `support` files are imported modules, not executable gates.

List the complete classification or run the candidate gates:

```sh
python3 scripts/wasm_test_suite.py --list
python3 scripts/wasm_test_suite.py target/wasm32-unknown-unknown/release/pomodorough_core.wasm
```

The historical scripts retain their existing paths and imports. Native R49 regressions still run through `cargo test --all-targets --locked`.

```sh
R49_C01_BASELINE_ORACLE="$PWD/tests/r49_c01_official_baseline.mjs" \
R49_C01_OFFICIAL_WASM=/path/to/official-v0.48.0.wasm \
cargo test --locked --test r49_c01_monotonic_retarget
node tests/r49_c01_standalone_parity.mjs /path/to/official-v0.48.0.wasm
node tests/r49_c02_official_baseline.mjs /path/to/official-v0.48.0.wasm
```

The C01 baseline command intentionally fails the new live-timer assertions on the published baseline. The standalone and C02 comparisons assert their preserved or explicitly corrected envelopes. Their digest guards must not accept a candidate artifact in place of the historical artifact.
