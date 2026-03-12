# Astra AutoAim Rust

A fresh Rust workspace for the Astra AutoAim rewrite.

## Goals

- Build a pure Rust core around a real-time vision pipeline.
- Keep hardware-specific boundaries isolated behind dedicated crates.
- Make the system testable with mock camera and mock serial inputs before hardware integration.

## Workspace Layout

- `crates/astra-app`: binary entrypoint and dependency wiring.
- `crates/astra-types`: shared domain types.
- `crates/astra-config`: configuration loading and validation.
- `crates/astra-protocol`: serial frame encoding and decoding.
- `crates/astra-io`: device-facing I/O traits and mock implementations.
- `crates/astra-vision`: detection interfaces and placeholder implementation.
- `crates/astra-tracking`: tracker interfaces and placeholder implementation.
- `crates/astra-ballistics`: aim solver interfaces and placeholder implementation.
- `crates/astra-camera-gx`: Daheng camera FFI boundary placeholder.
- `crates/astra-runtime`: pipeline orchestration and shared buffers.

## Suggested Migration Order

1. Stabilize shared types in `astra-types`.
2. Port YAML configuration into `astra-config`.
3. Replace C++ serial protocol with `astra-protocol` + `astra-io`.
4. Port runtime orchestration into `astra-runtime`.
5. Port detection into `astra-vision`.
6. Port tracking and aiming into `astra-tracking` and `astra-ballistics`.
7. Add camera FFI in `astra-camera-gx`.

## Getting Started

```powershell
cargo check
cargo run -p astra-app -- .\config\app.example.yaml
```