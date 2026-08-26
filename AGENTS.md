# Pomodorough Core

This crate is the deterministic Rust/WASM authority for timer, projection, identity, clock, bootstrap, and reconciliation policy shared by clients.

## Ownership boundaries

- `timer.rs` owns timer replay and canonical timer/history output.
- `projection.rs` owns the combined projection operation; `sync_projection.rs` owns per-domain reducers.
- `reconciliation.rs` composes reconciliation. Its `reconciliation/` modules separately own acknowledgements, canonical projection, clocks, timer dependencies, and validation.
- `fixture_projection.rs` owns legacy fixture-only projection adapters.
- `clock.rs`, `task.rs`, and `bootstrap.rs` own their named policies.
- `wasm_abi.rs` is the allocation/dispatch/free boundary. Keep ABI behavior and linear-memory limits stable.

Split by responsibility rather than forwarding through a facade. Preserve serialized field names, missing/null/value distinctions, deterministic ordering, acknowledgement semantics, error precedence, and JavaScript-safe numeric bounds.

## Verification

For every source change, complete all gates:

```sh
cargo fmt --all -- --check
cargo test --all-targets --locked
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo build --release --target wasm32-unknown-unknown --locked
python3 scripts/canonicalize_wasm_artifact.py target/wasm32-unknown-unknown/release/pomodorough_core.wasm
python3 scripts/verify_wasm_artifact.py target/wasm32-unknown-unknown/release/pomodorough_core.wasm
```

Run differential fixtures when changing reducers or reconciliation, and add negative coverage for every new validation boundary. Keep generated `target/` and `graphify-out/` content outside commits.
