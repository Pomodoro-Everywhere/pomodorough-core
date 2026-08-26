# Pomodorough Core

Shared deterministic domain models and synchronization reducers for all Pomodorough clients and the authoritative server.

## Boundary

The core owns pure, deterministic behavior:

- canonical timer reduction and terminal history;
- HLC/device/operation ordering;
- task, duration, auto-start, and selected-task projections;
- omission versus explicit-null selected-task decoding;
- optimistic replay, acknowledgement reconciliation, and bootstrap reduction;
- canonical/source versus display projection.

Persistence, authentication, HTTP/SSE, Iroh transport lifecycle, alarms, notifications, and UI stay platform-native behind adapters.

## Compatibility strategy

The stable binding boundary uses UTF-8 JSON so every language observes exactly the same omission/null and integer semantics. Rust owns the typed decoder and reducer. Native bindings are thin transport adapters and must not duplicate domain decisions.

Current host adapters:

- Swift through WasmKit;
- Kotlin through Chicory;
- Python through wasmtime;
- browsers through the native WebAssembly API;
- Go through wazero.

Every consumer runs the same WebAssembly artifact. The server route remains pure Go and does not require CGO.

### Canonical artifact provenance

Rust/LLVM can emit byte-different `wasm32-unknown-unknown` code sections from different host toolchain builds even when the source commit, Rust version, target, and behavior are identical. Therefore cross-host local rebuilds are semantic checks, not byte-provenance evidence.

The `ubuntu-24.04` Core CI job is the canonical producer. It uses pinned Rust `1.97.1`, removes non-semantic custom sections, validates the ABI, and uploads `pomodorough-core-wasm-${GITHUB_SHA}`. A release publishes those exact CI-produced bytes as `pomodorough_core.wasm`. Consumers pin the full Core commit and release digest, then verify their embedded bytes against that release artifact. They must not substitute bytes from a macOS or Windows rebuild.

## Development

```sh
cargo fmt --all -- --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```

The convergence corpus is copied byte-for-byte from the currently released clients/server during bootstrap. This repository becomes its canonical home once every consumer verifies the core artifact and pinned fixture revision in CI.
