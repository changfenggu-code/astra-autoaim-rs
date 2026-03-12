# Architecture Overview

## Pipeline Goal

The first Rust version prioritizes low-latency and stable control over maximum frame throughput.
Old data should be dropped before it can accumulate and poison the control loop.

## Key Path

`camera -> detect -> track/solve -> command -> serial tx`

## Concurrency Decisions

- Use dedicated threads for camera capture, inference, control, serial RX, and serial TX.
- Keep tracking, prediction, and aim solving in the same control thread.
- Use bounded channels and latest-value stores instead of unbounded queues.
- Prefer dropping stale frames and commands over processing old data.
- Treat CPU-only inference as the primary bottleneck and protect it with dedicated resources.

## Buffering Strategy

- `camera -> detect`: latest-frame ring or triple buffer.
- `serial rx -> control`: latest telemetry snapshot.
- `detect -> control`: small bounded channel with replace-latest behavior.
- `control -> serial tx`: latest command slot.

## Crate Responsibilities

- `astra-runtime` owns orchestration and shared synchronization primitives.
- `astra-vision` only performs preprocess, inference, and postprocess.
- `astra-tracking` owns target selection and state estimation.
- `astra-ballistics` owns pose conversion and aim calculation.
- `astra-camera-gx` is the only place where camera FFI should live.