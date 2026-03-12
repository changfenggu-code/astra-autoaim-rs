# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Astra AutoAim is a Rust rewrite of a real-time computer vision pipeline for robotic auto-aiming (RoboMaster competition). The project prioritizes low-latency and stable control over maximum frame throughput, with hardware-specific boundaries isolated behind dedicated crates.

## Common Commands

```bash
# Check compilation
cargo check

# Run in mock mode (no hardware required)
cargo run -p astra-app -- .\config\app.example.yaml

# Run with real serial but mock vision
cargo run -p astra-app -- .\config\app.example.yaml

# Run tests
cargo test

# Run tests for a specific crate
cargo test -p astra-vision

# Build with ONNX Runtime detector
cargo test -p astra-vision --features ort

# Build with Daheng camera support (requires GxCamera SDK)
cargo build --features gx-camera

# Build with both camera and detector
cargo build --features gx-camera,ort-detector
```

## Architecture

### Pipeline Flow
```
camera -> detect -> track/solve -> command -> serial tx
```

### Crate Responsibilities

| Crate | Purpose |
|-------|---------|
| `astra-types` | Shared domain types (Frame, Detection, Telemetry, AimCommand, TrackState, AimSolution) |
| `astra-config` | YAML configuration loading and validation |
| `astra-protocol` | Serial frame encoding/decoding (legacy protocol) |
| `astra-io` | Device I/O traits (TelemetrySource, CommandSink, SerialTransport) with mock implementations |
| `astra-vision` | Detection trait and implementations (Detector trait) |
| `astra-tracking` | Target tracking trait (Tracker trait) |
| `astra-ballistics` | Aim solving trait (AimSolver trait) |
| `astra-camera-gx` | Daheng camera FFI boundary (CameraSource trait) |
| `astra-runtime` | Pipeline orchestration, RuntimeHandles (LatestValue stores), PipelineRunner |
| `astra-app` | Binary entrypoint, dependency wiring |

### Key Traits

- `CameraSource`: `next_frame() -> Result<Frame>`
- `Detector`: `detect(&Frame) -> Result<DetectionBatch>`
- `Tracker`: `update(&DetectionBatch, &Telemetry) -> Option<TrackState>`
- `AimSolver`: `solve(&TrackState, &Telemetry) -> Result<AimSolution>`
- `TelemetrySource`: `recv() -> Result<Telemetry>`
- `CommandSink`: `send(&AimCommand) -> Result<()>`

### Concurrency Model

- Dedicated threads for camera capture, inference, control, serial RX, and serial TX
- Tracking, prediction, and aim solving run in the same control thread
- Bounded channels with replace-latest behavior (drop stale frames/commands)
- `LatestValue<T>` stores for sharing state between threads

### Feature Flags

- `gx-camera` (in astra-runtime): Enables Daheng camera support
- `gx` (in astra-camera-gx): Enables camera FFI bindings
- `mock` (default in astra-camera-gx): Enables mock camera source
- `ort` (in astra-vision): Enables ONNX Runtime detector (requires onnxruntime.dll)
- `ort-detector` (in astra-runtime): Enables ort detector backend

### ONNX Runtime Configuration

The `ort` feature requires `onnxruntime.dll`. The DLL path is resolved in this order:
1. `ORT_DYLIB_PATH` environment variable
2. `third_party/onnxruntime/lib/onnxruntime.dll`
3. `onnxruntime-win-x64-1.24.3/lib/onnxruntime.dll`
4. System PATH (onnxruntime.dll, bin/onnxruntime.dll, models/onnxruntime.dll)
5. `C:\Windows\System32\onnxruntime.dll`

Model output format: expects `[batch, num_boxes, 6]` where 6 = [x1, y1, x2, y2, confidence, class_id]

## Configuration

Configuration is loaded from YAML. Key sections:
- `app.mode`: "mock" or "real"
- `camera.source`: "mock" or "gx"
- `serial.backend`: "mock" or "real"
- `detector.backend`: "mock" or "ort"
- `runtime.*`: Buffer depths and mock cycle count

## Testing Strategy

The system is designed for testability without hardware:
- Mock implementations for all I/O traits
- `MockCameraSource`, `MockDetector`, `MockTelemetrySource`, `MockCommandSink`
- Run in fully mock mode for development: `mode: mock`

## Maintenance

- This file should be kept in sync with the Chinese version: `CLAUDE.zh-CN.md`
- Any changes to project architecture, commands, configuration, or crate responsibilities should be reflected in both files
