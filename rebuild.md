Astra AutoAim Rust 重构方案 v1
Summary
目标定为：纯 Rust 核心，仅保留 大恒相机 SDK 和可能的 ONNX Runtime 原生库 作为 FFI / 动态库边界。
重构不做“逐文件翻译”，而是改为 数据流驱动架构：Telemetry -> Frame -> Detection -> TrackState -> AimCommand -> SerialTx。
第一版优先保证：功能可跑通、接口稳定、可离线回放、可替换硬件输入；第二阶段再追求性能与工程完备度。
分阶段路线图
Phase 0：冻结现状并抽取契约
固化当前 C++ 项目的输入输出契约：串口帧格式、相机配置、检测结果字段、控制指令字段。
明确并修正文档中的数据语义，尤其是云台姿态、模式字段、角度单位、像素坐标系。
产出一份“离线回放样本包”：若干图片 / 视频帧、串口输入样本、期望输出日志。
验收标准：Rust 侧能在不接硬件时复现同一批输入样本的完整处理链。
Phase 1：先落地基础骨架
建立 Cargo workspace，拆出 types / config / protocol / app 基础 crate。
先实现统一配置加载、日志、错误模型、命令行入口。
引入消息总线，确定阶段间只通过 typed channel 通信，不共享可变全局状态。
验收标准：程序可启动，能读取配置，能跑 mock camera + mock serial 的空流水线。
Phase 2：替换串口与协议层
用 Rust 重写串口收发、帧编解码、控制报文打包、遥测报文解析。
输出强类型结构 Telemetry、AimCommand，彻底替代 std::vector<float>。
验收标准：离线协议单测覆盖正常帧、坏帧、短帧、粘包、空载荷，接口不暴露裸字节细节。
Phase 3：替换视觉检测与图像处理
用 opencv crate 接管图像容器与基础处理；检测推理走 ort 或 ONNX Runtime C API。
明确 detector 只做：预处理 -> 推理 -> 后处理，不在 API 内部重复构造 session。
验收标准：同一张测试图在 Rust 侧输出稳定 Vec<Detection>，耗时可观测，模型只初始化一次。
Phase 4：替换跟踪、预测、解算
用 nalgebra 自实现 Kalman 与状态模型，或过渡性使用 OpenCV KalmanFilter。
用显式相机模型和弹道参数替代当前“像素点直接 atan2”的简化解算。
验收标准：给定固定观测序列，跟踪状态可重复；给定标定参数，目标点可输出稳定 pitch / yaw。
Phase 5：接入相机 SDK 边界
将大恒相机封装为单独 crate，仅暴露 CameraSource trait 所需最小接口。
FFI 层只负责设备生命周期、取帧、错误码翻译，不承载业务逻辑。
验收标准：可在 mock 与 gxiapi 实现之间切换，上层无需改代码。
Phase 6：联调与性能收敛
加入端到端压测、帧率统计、阶段耗时、丢帧计数、串口重连、设备异常恢复。
根据瓶颈决定是否将检测单独线程池化、是否引入 bounded channel 背压。
验收标准：稳定运行、可观测、可恢复，不再依赖“多线程 + 条件变量 + 共享状态”的隐式同步。
crate 拆分方案
astra-types
放共享领域类型：FrameMeta、Detection、ArmorTarget、TrackState、Telemetry、AimCommand、GimbalPose。
只依赖轻量 crate，不依赖硬件库。
作为所有业务 crate 的唯一共享模型入口。
astra-config
负责 yaml / toml 配置加载与校验。
提供 AppConfig、CameraConfig、SerialConfig、DetectorConfig、BallisticsConfig。
统一路径解析，替代当前散落的相对路径常量。
astra-protocol
负责串口帧协议、CRC / checksum、报文打包和解析。
对外公开：encode_command()、decode_telemetry()、FrameCodec。
不直接碰串口设备，只处理字节与类型的转换。
astra-io
负责串口设备与通道驱动。
提供 TelemetrySource、CommandSink trait，以及 serial、mock 两类实现。
对上层输出 typed stream，不暴露底层读写线程细节。
astra-vision
负责图像预处理、模型加载、推理、NMS、检测结果转换。
提供 Detector trait，输出 Vec<Detection>。
可有两种实现：ort 推理实现、mock 检测实现。
astra-tracking
负责目标选择、Kalman、丢失重获、轨迹状态。
提供 Tracker trait，输入 Vec<Detection>，输出 Option<TrackState>。
将“最佳装甲板选择逻辑”从状态机中抽离。
astra-ballistics
负责相机坐标、PnP、姿态转换、弹道补偿、pitch / yaw 求解。
提供 AimSolver trait，输入 TrackState + Telemetry，输出 AimSolution。
可先实现无弹道简化版，再切换实用版。
astra-camera-gx
负责大恒相机 FFI 封装。
提供 CameraSource trait 的 gx 实现；必要时再加 video / image_dir mock 实现。
该 crate 是唯一允许出现 unsafe、原生句柄、厂商错误码翻译的地方。
astra-runtime
负责消息拓扑、任务编排、生命周期、健康检查、指标。
不放算法细节，只连接各阶段。
首选 tokio runtime，阶段之间用 bounded channel。
astra-app
CLI 入口与装配层。
负责根据配置选择 mock / real 设备实现。
只做依赖注入，不直接承载业务。
C++ → Rust 模块映射表
现有 C++ 模块	当前职责	Rust 目标 crate	Rust 目标接口
src/main.cpp	入口、信号处理、启动状态机	astra-app	main() + run_app(config)
slib/FSM	多线程流水线、阶段同步、控制队列	astra-runtime	PipelineRunner, PipelineChannels
slib/RobotControl/src/Serial.cpp	串口收发、帧编解码、线程	astra-protocol + astra-io	FrameCodec, SerialTelemetrySource, SerialCommandSink
slib/RobotControl/src/RobotControl.cpp	遥测解析、控制报文打包	astra-protocol + astra-types	Telemetry, AimCommand, encode/decode helpers
slib/Detector	ONNX 推理、图像预处理、检测结果	astra-vision	Detector::detect(&Mat) -> Result<Vec<Detection>>
slib/KalmanFilter	预测 / 更新	astra-tracking	Tracker::update(detections)
slib/AngleSolver	角度解算	astra-ballistics	AimSolver::solve(track, telemetry)
slib/Camera	SDK 初始化、取帧、相机参数	astra-camera-gx	CameraSource::next_frame()
etc/*.yaml	配置与模型路径	astra-config	AppConfig::load()
第一版目录结构
astra-autoaim-rs/
├─ Cargo.toml
├─ Cargo.lock
├─ crates/
│  ├─ astra-app/
│  │  └─ src/main.rs
│  ├─ astra-runtime/
│  │  └─ src/lib.rs
│  ├─ astra-types/
│  │  └─ src/lib.rs
│  ├─ astra-config/
│  │  └─ src/lib.rs
│  ├─ astra-protocol/
│  │  └─ src/lib.rs
│  ├─ astra-io/
│  │  └─ src/lib.rs
│  ├─ astra-vision/
│  │  └─ src/lib.rs
│  ├─ astra-tracking/
│  │  └─ src/lib.rs
│  ├─ astra-ballistics/
│  │  └─ src/lib.rs
│  └─ astra-camera-gx/
│     └─ src/lib.rs
├─ config/
│  ├─ app.yaml
│  ├─ camera.yaml
│  ├─ serial.yaml
│  └─ detector.yaml
├─ models/
│  ├─ armor.onnx
│  └─ best.onnx
├─ samples/
│  ├─ frames/
│  ├─ telemetry/
│  └─ expected/
├─ scripts/
│  ├─ run_mock.ps1
│  └─ run_real.ps1
└─ docs/
   ├─ protocol.md
   ├─ architecture.md
   └─ migration.md
第一版公共接口与核心类型
公共 trait
trait CameraSource { fn next_frame(&mut self) -> Result<Frame>; }
trait Detector { fn detect(&mut self, frame: &opencv::core::Mat) -> Result<Vec<Detection>>; }
trait Tracker { fn update(&mut self, detections: &[Detection], ts: Timestamp) -> Option<TrackState>; }
trait AimSolver { fn solve(&self, track: &TrackState, telemetry: &Telemetry) -> Result<AimSolution>; }
trait TelemetrySource { fn recv(&mut self) -> Result<Telemetry>; }
trait CommandSink { fn send(&mut self, cmd: &AimCommand) -> Result<()>; }
共享类型
Telemetry { mode, robot_id, color, gimbal_pose, shoot_speed, timestamp }
Detection { bbox, confidence, class_id, center, timestamp }
TrackState { target_id, center_px, velocity_px, confidence, lost_count }
AimSolution { pitch_rad, yaw_rad, fire, debug }
AimCommand { detect_flag, pitch_rad, yaw_rad, armor_pos, fire }
关键默认决策
角度、姿态、弹道内部统一使用 弧度制；序列化时再转换。
时间统一使用 std::time::Instant / 单调时钟，不在核心层直接用 wall clock。
阶段间通信统一有界 channel，避免无限队列。
错误统一用 thiserror，日志统一 tracing。
技术选型清单
必选
tokio：异步 runtime 与任务调度
flume 或 tokio::sync::mpsc：阶段间消息通道
serde + serde_yaml：配置
thiserror + anyhow：错误
tracing + tracing-subscriber：日志与指标
opencv：图像容器、预处理、可选 Kalman / PnP
nalgebra：矩阵、状态估计、几何
serialport 或 tokio-serial：串口
ort 或 ONNX Runtime C API 封装：推理
推荐
camino：路径处理
clap：CLI
crc：协议校验
approx：数值测试
criterion：性能基准
bindgen：相机 SDK FFI 生成
cc：必要时编译少量桥接 C/C++
不推荐的第一版选择
不建议第一版就上 ECS / actor framework。
不建议第一版就做 GPU provider 多后端抽象。
不建议第一版同时自研图像容器替代 opencv::core::Mat。
C++ OpenCV API → Rust opencv crate API 对照表
C++ OpenCV API	Rust opencv crate 对应	说明
cv::Mat	opencv::core::Mat	图像/矩阵主容器
cv::Point2f / cv::Point2d	opencv::core::Point2f / Point2d	坐标点
cv::Rect / cv::Rect2f	opencv::core::Rect / Rect2f	检测框
cv::Size	opencv::core::Size	图像尺寸
cv::Scalar	opencv::core::Scalar	颜色/标量
cv::cvtColor(src, dst, code)	imgproc::cvt_color(&src, &mut dst, code, dst_cn)	输出参数改为 &mut
cv::resize(src, dst, size)	imgproc::resize(&src, &mut dst, size, fx, fy, interp)	Rust 通常显式写完整参数
cv::split(mat, channels)	core::split(&mat, &mut channels)	channels 常为 core::Vector<Mat>
cv::merge(channels, dst)	core::merge(&channels, &mut dst)	与 C++ 同义
cv::rectangle(img, rect, color, thickness)	imgproc::rectangle(&mut img, rect, color, thickness, line_type, shift)	画框
cv::putText(...)	imgproc::put_text(...)	文本绘制
cv::getTextSize(...)	imgproc::get_text_size(...)	文本尺寸
cv::imread(path, flags)	imgcodecs::imread(path, flags)	读图
cv::imwrite(path, img)	imgcodecs::imwrite(path, &img, &params)	需额外传参数数组
cv::KalmanFilter	opencv::video::KalmanFilter	可过渡使用，长期建议 nalgebra 自实现
kf.predict()	kf.predict(&control) 或对应重载	Rust 绑定保留 OpenCV 风格
kf.correct(measurement)	kf.correct(&measurement)	更新测量
cv::solvePnP(...)	calib3d::solve_pnp(...)	PnP 解算
cv::projectPoints(...)	calib3d::project_points(...)	反投影/验证
cv::FileStorage	不推荐直接迁移；优先 serde_yaml	Rust 侧更适合配置反序列化
cv::waitKey() / imshow()	highgui::wait_key() / highgui::imshow()	调试可用，生产路径尽量不用
cv::dnn	不建议用于本项目	已有 ONNX Runtime，更适合继续沿用
opencv crate 的原理说明
1. 它不是“Rust 版 OpenCV”，而是 Rust 绑定
opencv crate 本质是对系统里已安装的 OpenCV 动态库 / 开发库做绑定。
运行时仍调用原生 OpenCV 的 C++ 实现，所以性能与能力主要取决于底层 OpenCV 版本，而不是 Rust 自己重写了一套。
2. Rust API 形状为什么和 C++ 很像
OpenCV 原本是面向 C++ 的面向对象 API，opencv crate 会尽量保留其命名和调用结构。
因此你会看到：
模块名很接近：core / imgproc / calib3d / video
类型名很接近：Mat / Rect / Scalar / Point2f
很多函数仍沿用 “输入 + 输出参数” 的风格
3. 为什么很多函数是 Result<T>
C++ OpenCV 常用异常或错误码。
Rust 绑定会把这些失败路径包装成 opencv::Result<T>，让调用方显式处理错误。
这会比当前 C++ 项目的“出错打印日志后返回空对象”更安全。
4. 为什么输出参数常是 &mut
OpenCV C++ 大量 API 通过“调用者提供输出对象”减少拷贝。
Rust 绑定会把这种模式映射为 &mut dst，既保留性能，也符合 Rust 借用规则。
例如：cvtColor(src, dst) 变成 cvt_color(&src, &mut dst, ...)。
5. Mat 在 Rust 里仍然是对底层 OpenCV 对象的安全封装
Mat 并不是 Rust 自己管理像素数组的纯安全容器。
它内部仍包装 OpenCV 的 native 对象；Rust 负责在类型层面约束所有权和借用，减少误用。
真正涉及裸指针、跨线程、FFI 时，仍要谨慎处理生命周期。
6. 为什么建议“图像继续用 OpenCV，数学转到 nalgebra”
图像预处理、颜色空间、几何变换这些，OpenCV 非常成熟。
但状态估计、控制、几何建模如果继续强绑 OpenCV，会让业务逻辑被图像库牵着走。
因此建议：
图像层：opencv
算法 / 状态层：nalgebra
配置 / 协议 / 调度层：纯 Rust
测试计划
协议单测：正常帧、截断帧、粘包、错误校验、边界角度编码。
检测单测：固定图像输入，验证 bbox 数量、坐标范围、置信度阈值行为。
跟踪单测：给定观测序列，验证预测 / 更新的稳定性与丢失恢复。
解算单测：给定内参与目标点，验证 pitch / yaw 输出符号、单位、极值行为。
集成测试：mock camera + mock serial + sample frames 跑通完整链路。
回归测试：对照 C++ 样本输出，确保 Rust 版本在可接受误差内一致。
假设与默认值
默认保留 ONNX 模型文件，不在第一版更换模型或训练流程。
默认相机 SDK 不重写为纯 Rust，只做最小 FFI 封装。
默认第一版目标平台仍以 Linux 为主，后续再补 Windows 兼容。
默认第一版先实现单目标追踪，不扩展多目标融合。
默认第一版不引入 GUI，只保留日志和可选图像调试输出。
默认第一版把当前 yaml 配置迁到 serde_yaml，不再继续依赖 OpenCV FileStorage。
