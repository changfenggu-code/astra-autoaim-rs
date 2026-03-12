# CLAUDE.md (中文版)

本文档为 Claude Code (claude.ai/code) 在本项目中工作时提供指导。

## 项目概述

Astra AutoAim 是一个用 Rust 重写的实时计算机视觉流水线，用于机器人自动瞄准（RoboMaster 比赛）。本项目优先考虑低延迟和稳定的控制，而非最大帧吞吐量，硬件特定的边界被隔离在专用 crate 中。

## 常用命令

```bash
# 检查编译
cargo check

# 以模拟模式运行（无需硬件）
cargo run -p astra-app -- .\config\app.example.yaml

# 使用真实串口但模拟视觉运行
cargo run -p astra-app -- .\config\app.example.yaml

# 运行测试
cargo test

# 运行特定 crate 的测试
cargo test -p astra-vision

# 使用 ONNX Runtime 检测器运行测试
cargo test -p astra-vision --features ort

# 启用大恒相机支持进行构建（需要 GxCamera SDK）
cargo build --features gx-camera

# 同时启用相机和检测器
cargo build --features gx-camera,ort-detector
```

## 架构

### 流水线流程
```
camera -> detect -> track/solve -> command -> serial tx
```

### Crate 职责

| Crate | 用途 |
|-------|------|
| `astra-types` | 共享领域类型（Frame、Detection、Telemetry、AimCommand、TrackState、AimSolution） |
| `astra-config` | YAML 配置加载与验证 |
| `astra-protocol` | 串口帧编码/解码（遗留协议） |
| `astra-io` | 设备 I/O  traits（TelemetrySource、CommandSink、SerialTransport）及模拟实现 |
| `astra-vision` | 检测接口与实现（Detector trait） |
| `astra-tracking` | 目标跟踪接口（Tracker trait） |
| `astra-ballistics` | 瞄准解算接口（AimSolver trait） |
| `astra-camera-gx` | 大恒相机 FFI 边界（CameraSource trait） |
| `astra-runtime` | 流水线编排、RuntimeHandles（LatestValue 存储）、PipelineRunner |
| `astra-app` | 二进制入口点、依赖注入 |

### 关键 Traits

- `CameraSource`: `next_frame() -> Result<Frame>`
- `Detector`: `detect(&Frame) -> Result<DetectionBatch>`
- `Tracker`: `update(&DetectionBatch, &Telemetry) -> Option<TrackState>`
- `AimSolver`: `solve(&TrackState, &Telemetry) -> Result<AimSolution>`
- `TelemetrySource`: `recv() -> Result<Telemetry>`
- `CommandSink`: `send(&AimCommand) -> Result<()>`

### 并发模型

- 为相机捕获、推理、控制、串口接收、串口发送使用专用线程
- 跟踪、预测和瞄准解算在同一控制线程中运行
- 使用有界通道和最新值替换行为（丢弃过时的帧/命令）
- `LatestValue<T>` 用于线程间共享状态

### 特性标志

- `gx-camera`（在 astra-runtime 中）：启用大恒相机支持
- `gx`（在 astra-camera-gx 中）：启用相机 FFI 绑定
- `mock`（在 astra-camera-gx 中默认）：启用模拟相机源
- `ort`（在 astra-vision 中）：启用 ONNX Runtime 检测器（需要 onnxruntime.dll）
- `ort-detector`（在 astra-runtime 中）：启用 ort 检测器后端

### ONNX Runtime 配置

`ort` 特性需要 `onnxruntime.dll`。DLL 路径按以下顺序解析：
1. `ORT_DYLIB_PATH` 环境变量
2. `third_party/onnxruntime/lib/onnxruntime.dll`
3. `onnxruntime-win-x64-1.24.3/lib/onnxruntime.dll`
4. 系统 PATH（onnxruntime.dll、bin/onnxruntime.dll、models/onnxruntime.dll）
5. `C:\Windows\System32\onnxruntime.dll`

模型输出格式：期望 `[batch, num_boxes, 6]`，其中 6 = [x1, y1, x2, y2, confidence, class_id]

## 配置

配置从 YAML 加载。关键部分：
- `app.mode`："mock" 或 "real"
- `camera.source`："mock" 或 "gx"
- `serial.backend`："mock" 或 "real"
- `detector.backend`："mock" 或 "ort"
- `runtime.*`：缓冲区深度和模拟周期数

## 测试策略

系统设计为无需硬件即可测试：
- 所有 I/O traits 的模拟实现
- `MockCameraSource`、`MockDetector`、`MockTelemetrySource`、`MockCommandSink`
- 完全模拟模式运行进行开发：`mode: mock`

## 维护说明

- 本文件（CLAUDE.zh-CN.md）与英文版 CLAUDE.md 需同步更新
- 任何对项目架构、命令、配置或 crate 职责的修改都应同时反映在两个文件中

## 版本管理

本项目使用语义化版本控制（Semantic Versioning，SemVer）：

| 版本 | 递增 | 描述 |
|------|------|------|
| `0.0.x` | patch | Bug 修复、小改动 |
| `0.x.0` | minor | 新功能、功能性添加 |
| `x.0.0` | major | 破坏性变更、重大发布 |

### 发布流程

```bash
# 更新 Cargo.toml 和所有 crate 的 Cargo.toml 中的版本号
# 然后标记提交：
git tag -a v0.1.0 -m "Release v0.1.0: 添加 ONNX Runtime 检测器支持"
git push origin main --tags
```

### 版本历史

- `v0.0.1` - 初始项目搭建，包含模拟实现
- `v0.1.0` - ONNX Runtime 检测器集成
