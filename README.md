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

Planned consumers:

- Swift and Kotlin through UniFFI/native libraries;
- Python through PyO3;
- browsers through WebAssembly;
- Go server through the same WebAssembly module embedded with `go:embed` and executed by a pure-Go runtime.

The Go/WASM route keeps server cross-compilation reproducible and avoids CGO while still executing the same Rust reducer.

## Development

```sh
cargo fmt --all -- --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```

The convergence corpus is copied byte-for-byte from the currently released clients/server during bootstrap. This repository becomes its canonical home once every consumer verifies the core artifact and pinned fixture revision in CI.
