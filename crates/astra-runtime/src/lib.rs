use anyhow::Result;
use astra_ballistics::{AimSolver, SimpleAimSolver};
use astra_camera_gx::{CameraSource, MockCameraSource, GX_FEATURE_ENABLED};
#[cfg(feature = "gx-camera")]
use astra_camera_gx::{GxCameraConfig, GxCameraSource};
use astra_config::AppConfig;
use astra_io::{
    build_real_legacy_serial_io, legacy_codec_from_serial_config, CommandSink, IoError, LegacyCommandSink,
    LegacyTelemetrySource, MockCommandSink, MockSerialPort, MockTelemetrySource, TelemetrySource,
};
use astra_tracking::{SimpleTracker, Tracker};
use astra_types::{AimCommand, DetectionBatch, Frame, TargetPosition, Telemetry};
use astra_vision::{Detector, MockDetector};
#[cfg(feature = "ort-detector")]
use astra_vision::{OrtDetector, OrtDetectorConfig};
use crossbeam_channel::{bounded, Sender, TrySendError};
use std::{sync::{Arc, RwLock}, thread};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeMode {
    Mock,
    Real,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SerialBackend {
    Mock,
    Real,
}

impl SerialBackend {
    pub fn parse(value: Option<&str>, mode: RuntimeMode) -> Self {
        match value.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
            Some("real") => Self::Real,
            Some("mock") => Self::Mock,
            _ => match mode {
                RuntimeMode::Real => Self::Real,
                RuntimeMode::Mock => Self::Mock,
            },
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mock => "mock",
            Self::Real => "real",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraSourceKind {
    Mock,
    Gx,
}

impl CameraSourceKind {
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "gx" => Self::Gx,
            _ => Self::Mock,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mock => "mock",
            Self::Gx => "gx",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectorBackend {
    Mock,
    Ort,
}

impl DetectorBackend {
    pub fn parse(value: Option<&str>) -> Self {
        match value.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
            Some("ort") => Self::Ort,
            _ => Self::Mock,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mock => "mock",
            Self::Ort => "ort",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeSelection {
    pub mode: RuntimeMode,
    pub serial: SerialBackend,
    pub camera: CameraSourceKind,
    pub detector: DetectorBackend,
}

impl RuntimeSelection {
    pub fn summary(self) -> &'static str {
        match (self.serial, self.camera, self.detector) {
            (SerialBackend::Mock, CameraSourceKind::Mock, DetectorBackend::Mock) => "mock-all",
            (SerialBackend::Real, CameraSourceKind::Mock, DetectorBackend::Mock) => "real-serial-mock-vision",
            (SerialBackend::Real, CameraSourceKind::Gx, DetectorBackend::Mock) => "real-serial-real-camera-mock-detector",
            (SerialBackend::Real, CameraSourceKind::Gx, DetectorBackend::Ort) => "real-all-core",
            _ => "mixed-custom",
        }
    }
}

#[cfg(feature = "ort-detector")]
fn ort_detector_config_from_app(config: &AppConfig) -> OrtDetectorConfig {
    OrtDetectorConfig::from(&config.detector)
}

impl RuntimeMode {
    pub fn parse(mode: &str) -> Self {
        match mode.trim().to_ascii_lowercase().as_str() {
            "real" => Self::Real,
            _ => Self::Mock,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mock => "mock",
            Self::Real => "real",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LatestValue<T> {
    inner: Arc<RwLock<Option<T>>>,
}

impl<T> Default for LatestValue<T> {
    fn default() -> Self {
        Self {
            inner: Arc::new(RwLock::new(None)),
        }
    }
}

impl<T: Clone> LatestValue<T> {
    pub fn store(&self, value: T) {
        if let Ok(mut slot) = self.inner.write() {
            *slot = Some(value);
        }
    }

    pub fn snapshot(&self) -> Option<T> {
        self.inner.read().ok().and_then(|slot| slot.clone())
    }

    pub fn clear(&self) {
        if let Ok(mut slot) = self.inner.write() {
            *slot = None;
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeHandles {
    pub latest_frame: LatestValue<Frame>,
    pub latest_telemetry: LatestValue<Telemetry>,
    pub latest_command: LatestValue<AimCommand>,
}

#[derive(Debug, Clone)]
pub struct PipelineRunner {
    config: AppConfig,
    handles: RuntimeHandles,
}

#[cfg(feature = "gx-camera")]
fn gx_camera_config_from_app(config: &AppConfig) -> GxCameraConfig {
    GxCameraConfig {
        device_index: config.camera.device_index,
        width: config.camera.width,
        height: config.camera.height,
        acquisition_timeout_ms: config.camera.acquisition_timeout_ms,
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MockPipelineReport {
    pub telemetry_updates: usize,
    pub frames_captured: usize,
    pub detections_produced: usize,
    pub commands_sent: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RealPipelineReport {
    pub mode: &'static str,
    pub serial_ready: bool,
    pub camera_ready: bool,
    pub detector_ready: bool,
    pub cycles_completed: usize,
    pub commands_sent: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineStartReport {
    Mock(MockPipelineReport),
    Real(RealPipelineReport),
}

#[derive(Debug, Clone)]
pub struct MockRuntimeAssembly {
    config: AppConfig,
}

impl MockRuntimeAssembly {
    pub fn from_config(config: &AppConfig) -> Self {
        Self {
            config: config.clone(),
        }
    }

    pub fn run(&self, handles: &RuntimeHandles) -> Result<MockPipelineReport> {
        run_threaded_mock_pipeline(&self.config, handles)
    }
}

#[derive(Debug, Clone)]
pub struct RealRuntimeAssembly {
    config: AppConfig,
}

impl RealRuntimeAssembly {
    pub fn from_config(config: &AppConfig) -> Self {
        Self {
            config: config.clone(),
        }
    }

    pub fn prepare(&self, _handles: &RuntimeHandles) -> Result<RealPipelineReport> {
        let serial_ready = !self.config.serial.read_device.trim().is_empty()
            && !self.config.serial.write_device.trim().is_empty();
        let camera_ready = !self.config.camera.source.trim().is_empty();
        let detector_ready = !self.config.detector.model_path.trim().is_empty();

        tracing::info!(
            serial_ready,
            camera_ready,
            detector_ready,
            read_device = %self.config.serial.read_device,
            write_device = %self.config.serial.write_device,
            camera_source = %self.config.camera.source,
            model_path = %self.config.detector.model_path,
            "real runtime assembly prepared"
        );

        Ok(RealPipelineReport {
            mode: RuntimeMode::Real.as_str(),
            serial_ready,
            camera_ready,
            detector_ready,
            cycles_completed: 0,
            commands_sent: 0,
        })
    }

    pub fn build_serial_io(
        &self,
    ) -> Result<(
        LegacyTelemetrySource<astra_io::RealSerialPort>,
        LegacyCommandSink<astra_io::RealSerialPort>,
    )> {
        build_real_legacy_serial_io(&self.config.serial)
    }

    pub fn run(&self, runner: &PipelineRunner) -> Result<RealPipelineReport> {
        let mut report = self.prepare(&runner.handles)?;
        let (mut telemetry_source, mut command_sink) = self.build_serial_io()?;
        let run_report = runner.run_real_serial_mock_vision_pipeline(
            &mut telemetry_source,
            &mut command_sink,
        )?;

        report.cycles_completed = run_report.cycles_completed;
        report.commands_sent = run_report.commands_sent;
        Ok(report)
    }
}

#[cfg(feature = "gx-camera")]
impl RealRuntimeAssembly {
    pub fn run_with_gx_camera(&self, runner: &PipelineRunner) -> Result<RealPipelineReport> {
        let mut report = self.prepare(&runner.handles)?;
        let (mut telemetry_source, mut command_sink) = self.build_serial_io()?;
        let run_report = runner.run_gx_camera_pipeline(&mut telemetry_source, &mut command_sink)?;

        report.cycles_completed = run_report.cycles_completed;
        report.commands_sent = run_report.commands_sent;
        report.camera_ready = true;
        Ok(report)
    }
}

impl PipelineRunner {
    pub fn new(config: AppConfig) -> Self {
        Self {
            config,
            handles: RuntimeHandles::default(),
        }
    }

    pub fn handles(&self) -> &RuntimeHandles {
        &self.handles
    }

    pub fn mode(&self) -> RuntimeMode {
        RuntimeMode::parse(&self.config.app.mode)
    }

    pub fn selection(&self) -> RuntimeSelection {
        let mode = self.mode();
        RuntimeSelection {
            mode,
            serial: SerialBackend::parse(self.config.serial.backend.as_deref(), mode),
            camera: CameraSourceKind::parse(&self.config.camera.source),
            detector: DetectorBackend::parse(self.config.detector.backend.as_deref()),
        }
    }

    pub fn run_cycle<C, D, T, S, TS, CS>(
        &self,
        camera: &mut C,
        detector: &mut D,
        tracker: &mut T,
        solver: &S,
        telemetry_source: &mut TS,
        command_sink: &mut CS,
    ) -> Result<Option<AimCommand>>
    where
        C: CameraSource,
        D: Detector,
        T: Tracker,
        S: AimSolver,
        TS: TelemetrySource,
        CS: CommandSink,
    {
        let telemetry = telemetry_source.recv()?;
        self.handles.latest_telemetry.store(telemetry.clone());

        let frame = camera.next_frame()?;
        self.handles.latest_frame.store(frame.clone());

        let detections = detector.detect(&frame)?;
        let Some(track) = tracker.update(&detections, &telemetry) else {
            self.handles.latest_command.clear();
            return Ok(None);
        };

        let solution = solver.solve(&track, &telemetry)?;
        let command = AimCommand {
            detect_flag: true,
            pitch_rad: solution.pitch_rad,
            yaw_rad: solution.yaw_rad,
            target_position: TargetPosition {
                x: track.center_px.x as i16,
                y: track.center_px.y as i16,
            },
            fire: solution.fire,
            ..AimCommand::default()
        };

        command_sink.send(&command)?;
        self.handles.latest_command.store(command.clone());
        Ok(Some(command))
    }

    pub fn run_mock_cycle(&self) -> Result<Option<AimCommand>> {
        let mut camera = MockCameraSource::new(self.config.camera.width, self.config.camera.height);
        let mut detector = MockDetector {
            confidence_threshold: self.config.detector.confidence_threshold,
        };
        let mut tracker = SimpleTracker;
        let solver = SimpleAimSolver;
        let mut telemetry_source = MockTelemetrySource::default();
        let mut command_sink = MockCommandSink::default();

        self.run_cycle(
            &mut camera,
            &mut detector,
            &mut tracker,
            &solver,
            &mut telemetry_source,
            &mut command_sink,
        )
    }

    pub fn run_real_serial_mock_vision_pipeline<TS, CS>(
        &self,
        telemetry_source: &mut TS,
        command_sink: &mut CS,
    ) -> Result<RealPipelineReport>
    where
        TS: TelemetrySource,
        CS: CommandSink,
    {
        let cycles = self.config.runtime.mock_cycles.max(1);
        let mut camera = MockCameraSource::new(self.config.camera.width, self.config.camera.height);
        let mut detector = MockDetector {
            confidence_threshold: self.config.detector.confidence_threshold,
        };
        let mut tracker = SimpleTracker;
        let solver = SimpleAimSolver;
        let mut commands_sent = 0;

        for _ in 0..cycles {
            if self
                .run_cycle(
                    &mut camera,
                    &mut detector,
                    &mut tracker,
                    &solver,
                    telemetry_source,
                    command_sink,
                )?
                .is_some()
            {
                commands_sent += 1;
            }
        }

        Ok(RealPipelineReport {
            mode: RuntimeMode::Real.as_str(),
            serial_ready: true,
            camera_ready: true,
            detector_ready: true,
            cycles_completed: cycles,
            commands_sent,
        })
    }

    #[cfg(feature = "gx-camera")]
    pub fn run_gx_camera_pipeline<TS, CS>(
        &self,
        telemetry_source: &mut TS,
        command_sink: &mut CS,
    ) -> Result<RealPipelineReport>
    where
        TS: TelemetrySource,
        CS: CommandSink,
    {
        let cycles = self.config.runtime.mock_cycles.max(1);
        let mut camera = GxCameraSource::open(gx_camera_config_from_app(&self.config))?;
        let mut detector = MockDetector {
            confidence_threshold: self.config.detector.confidence_threshold,
        };
        let mut tracker = SimpleTracker;
        let solver = SimpleAimSolver;
        let mut commands_sent = 0;

        for _ in 0..cycles {
            if self
                .run_cycle(
                    &mut camera,
                    &mut detector,
                    &mut tracker,
                    &solver,
                    telemetry_source,
                    command_sink,
                )?
                .is_some()
            {
                commands_sent += 1;
            }
        }

        Ok(RealPipelineReport {
            mode: RuntimeMode::Real.as_str(),
            serial_ready: true,
            camera_ready: true,
            detector_ready: true,
            cycles_completed: cycles,
            commands_sent,
        })
    }

    #[cfg(feature = "gx-camera")]
    pub fn run_mock_serial_gx_camera_pipeline(&self) -> Result<RealPipelineReport> {
        let mut telemetry_source = MockTelemetrySource::default();
        let mut command_sink = MockCommandSink::default();
        self.run_gx_camera_pipeline(&mut telemetry_source, &mut command_sink)
    }

    pub fn run_threaded_mock_pipeline(&self) -> Result<MockPipelineReport> {
        MockRuntimeAssembly::from_config(&self.config).run(&self.handles)
    }

    #[cfg(feature = "ort-detector")]
    pub fn run_mock_serial_mock_camera_ort_pipeline(&self) -> Result<RealPipelineReport> {
        let cycles = self.config.runtime.mock_cycles.max(1);
        let mut camera = MockCameraSource::new(self.config.camera.width, self.config.camera.height);
        let mut detector = OrtDetector::new(ort_detector_config_from_app(&self.config))?;
        let mut tracker = SimpleTracker;
        let solver = SimpleAimSolver;
        let mut telemetry_source = MockTelemetrySource::default();
        let mut command_sink = MockCommandSink::default();

        for _ in 0..cycles {
            let _ = self.run_cycle(
                &mut camera,
                &mut detector,
                &mut tracker,
                &solver,
                &mut telemetry_source,
                &mut command_sink,
            )?;
        }

        Ok(RealPipelineReport {
            mode: RuntimeMode::Mock.as_str(),
            serial_ready: true,
            camera_ready: true,
            detector_ready: true,
            cycles_completed: cycles,
            commands_sent: command_sink.sent().len(),
        })
    }

    pub fn start(&self) -> Result<PipelineStartReport> {
        let selection = self.selection();
        tracing::info!(
            mode = self.mode().as_str(),
            serial_backend = selection.serial.as_str(),
            camera_source = selection.camera.as_str(),
            detector_backend = selection.detector.as_str(),
            selection = selection.summary(),
            frame_buffer_depth = self.config.runtime.frame_buffer_depth,
            detection_channel_depth = self.config.runtime.detection_channel_depth,
            mock_cycles = self.config.runtime.mock_cycles,
            "runtime skeleton initialized"
        );

        match (selection.serial, selection.camera, selection.detector) {
            (SerialBackend::Mock, CameraSourceKind::Mock, DetectorBackend::Mock) => {
                let report = MockRuntimeAssembly::from_config(&self.config).run(&self.handles)?;
                tracing::info!(?report, "mock pipeline run completed");
                Ok(PipelineStartReport::Mock(report))
            }
            (SerialBackend::Mock, CameraSourceKind::Mock, DetectorBackend::Ort) => {
                #[cfg(feature = "ort-detector")]
                {
                    let report = self.run_mock_serial_mock_camera_ort_pipeline()?;
                    tracing::info!(?report, "mock serial + ort detector pipeline run completed");
                    Ok(PipelineStartReport::Real(report))
                }
                #[cfg(not(feature = "ort-detector"))]
                {
                    Err(anyhow::anyhow!(
                        "detector backend 'ort' requires building astra-runtime with the 'ort-detector' feature enabled"
                    ))
                }
            }
            (SerialBackend::Real, CameraSourceKind::Mock, DetectorBackend::Mock) => {
                let report = RealRuntimeAssembly::from_config(&self.config).run(self)?;
                tracing::info!(?report, "real pipeline assembly completed");
                Ok(PipelineStartReport::Real(report))
            }
            (SerialBackend::Mock, CameraSourceKind::Gx, DetectorBackend::Mock) => {
                #[cfg(feature = "gx-camera")]
                {
                    let report = self.run_mock_serial_gx_camera_pipeline()?;
                    tracing::info!(?report, "gx camera pipeline run completed");
                    Ok(PipelineStartReport::Real(report))
                }
                #[cfg(not(feature = "gx-camera"))]
                {
                    Err(anyhow::anyhow!(
                        "camera source 'gx' requires building astra-runtime with the 'gx-camera' feature enabled"
                    ))
                }
            }
            (SerialBackend::Real, CameraSourceKind::Gx, DetectorBackend::Mock) => {
                #[cfg(feature = "gx-camera")]
                {
                    let report = RealRuntimeAssembly::from_config(&self.config).run_with_gx_camera(self)?;
                    tracing::info!(?report, "real serial + gx camera pipeline run completed");
                    Ok(PipelineStartReport::Real(report))
                }
                #[cfg(not(feature = "gx-camera"))]
                {
                    Err(anyhow::anyhow!(
                        "camera source 'gx' requires building astra-runtime with the 'gx-camera' feature enabled"
                    ))
                }
            }
            (_, CameraSourceKind::Gx, _) if !GX_FEATURE_ENABLED => Err(anyhow::anyhow!(
                "camera source 'gx' requires building astra-camera-gx with the 'gx' feature enabled"
            )),
            (_, _, DetectorBackend::Ort) if !cfg!(feature = "ort-detector") => Err(anyhow::anyhow!(
                "detector backend 'ort' requires building astra-runtime with the 'ort-detector' feature enabled"
            )),
            _ => Err(anyhow::anyhow!(
                "unsupported runtime selection: serial={}, camera={}, detector={}",
                selection.serial.as_str(),
                selection.camera.as_str(),
                selection.detector.as_str()
            )),
        }
    }
}

fn run_threaded_mock_pipeline(config: &AppConfig, handles: &RuntimeHandles) -> Result<MockPipelineReport> {
    let cycles = config.runtime.mock_cycles.max(1);
    let frame_depth = config.runtime.frame_buffer_depth.max(1);
    let detection_depth = config.runtime.detection_channel_depth.max(1);
    let command_depth = config.runtime.command_slot_depth.max(1);

    let latest_detection = LatestValue::<DetectionBatch>::default();
    let handles = handles.clone();
    let codec = legacy_codec_from_serial_config(&config.serial);
    let serial_transport = MockSerialPort::default();
    seed_mock_telemetry_frames(&serial_transport, cycles);

    let (frame_tx, frame_rx) = bounded::<()>(frame_depth);
    let (detection_tx, detection_rx) = bounded::<()>(detection_depth);
    let (command_tx, command_rx) = bounded::<()>(command_depth);

    let serial_handles = handles.clone();
    let serial_transport_rx = serial_transport.clone();
    let serial_thread = thread::spawn(move || -> Result<usize> {
        let mut source = LegacyTelemetrySource::new(serial_transport_rx, codec);
        let mut updates = 0;

        loop {
            match source.recv() {
                Ok(telemetry) => {
                    serial_handles.latest_telemetry.store(telemetry);
                    updates += 1;
                }
                Err(error) if error.downcast_ref::<IoError>().is_some() => break,
                Err(error) => return Err(error),
            }
        }

        Ok(updates)
    });

    let camera_handles = handles.clone();
    let width = config.camera.width;
    let height = config.camera.height;
    let camera_thread = thread::spawn(move || -> Result<usize> {
        let mut camera = MockCameraSource::new(width, height);
        let mut frames = 0;

        for _ in 0..cycles {
            let frame = camera.next_frame()?;
            camera_handles.latest_frame.store(frame);
            frames += 1;
            notify_latest(&frame_tx);
            thread::yield_now();
        }

        Ok(frames)
    });

    let infer_handles = handles.clone();
    let infer_detection = latest_detection.clone();
    let confidence_threshold = config.detector.confidence_threshold;
    let infer_thread = thread::spawn(move || -> Result<usize> {
        let mut detector = MockDetector {
            confidence_threshold,
        };
        let mut detections = 0;

        while frame_rx.recv().is_ok() {
            let Some(frame) = infer_handles.latest_frame.snapshot() else {
                continue;
            };

            let batch = detector.detect(&frame)?;
            infer_detection.store(batch);
            detections += 1;
            notify_latest(&detection_tx);
        }

        Ok(detections)
    });

    let control_handles = handles.clone();
    let control_detection = latest_detection.clone();
    let control_thread = thread::spawn(move || -> Result<usize> {
        let mut tracker = SimpleTracker;
        let solver = SimpleAimSolver;
        let mut commands = 0;

        while detection_rx.recv().is_ok() {
            let Some(detections) = control_detection.snapshot() else {
                continue;
            };
            let telemetry = control_handles.latest_telemetry.snapshot().unwrap_or_default();

            let Some(track) = tracker.update(&detections, &telemetry) else {
                control_handles.latest_command.clear();
                continue;
            };

            let solution = solver.solve(&track, &telemetry)?;
            let command = AimCommand {
                detect_flag: true,
                pitch_rad: solution.pitch_rad,
                yaw_rad: solution.yaw_rad,
                target_position: TargetPosition {
                    x: track.center_px.x as i16,
                    y: track.center_px.y as i16,
                },
                fire: solution.fire,
                ..AimCommand::default()
            };

            control_handles.latest_command.store(command);
            commands += 1;
            notify_latest(&command_tx);
        }

        Ok(commands)
    });

    let tx_handles = handles.clone();
    let serial_transport_tx = serial_transport.clone();
    let tx_codec = legacy_codec_from_serial_config(&config.serial);
    let tx_thread = thread::spawn(move || -> Result<usize> {
        let mut sink = LegacyCommandSink::new(serial_transport_tx, tx_codec);
        let mut sent = 0;

        while command_rx.recv().is_ok() {
            let Some(command) = tx_handles.latest_command.snapshot() else {
                continue;
            };
            sink.send(&command)?;
            sent += 1;
        }

        Ok(sent)
    });

    let telemetry_updates = join_thread(serial_thread)?;
    let frames_captured = join_thread(camera_thread)?;
    let detections_produced = join_thread(infer_thread)?;
    let commands_sent = join_thread(tx_thread)?;
    let control_commands = join_thread(control_thread)?;

    tracing::info!(
        telemetry_updates,
        frames_captured,
        detections_produced,
        commands_built = control_commands,
        commands_sent,
        "threaded mock pipeline completed"
    );

    Ok(MockPipelineReport {
        telemetry_updates,
        frames_captured,
        detections_produced,
        commands_sent,
    })
}

fn notify_latest(sender: &Sender<()>) {
    match sender.try_send(()) {
        Ok(()) | Err(TrySendError::Full(())) => {}
        Err(TrySendError::Disconnected(())) => {}
    }
}

fn build_mock_telemetry_frame(sequence: usize) -> [u8; 8] {
    let mode = 12_u8;
    let yaw = ((sequence as u16) + 100).to_le_bytes();
    let pitch = ((sequence as u16) + 200).to_le_bytes();

    [0x45, mode, yaw[0], yaw[1], pitch[0], pitch[1], 30, 0xFF]
}

fn seed_mock_telemetry_frames(transport: &MockSerialPort, cycles: usize) {
    for sequence in 0..cycles {
        transport.push_rx_frame(build_mock_telemetry_frame(sequence));
    }
}

fn join_thread<T>(handle: thread::JoinHandle<Result<T>>) -> Result<T> {
    match handle.join() {
        Ok(result) => result,
        Err(payload) => {
            let message = if let Some(message) = payload.downcast_ref::<&str>() {
                (*message).to_string()
            } else if let Some(message) = payload.downcast_ref::<String>() {
                message.clone()
            } else {
                "unknown thread panic".to_string()
            };

            Err(anyhow::anyhow!("thread panicked: {message}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CameraSourceKind, DetectorBackend, LatestValue, PipelineRunner, PipelineStartReport,
        RuntimeMode, RuntimeSelection, SerialBackend,
    };
    use astra_config::AppConfig;
    use astra_io::{
        legacy_codec_from_serial_config, LegacyCommandSink, LegacyTelemetrySource, MockCommandSink,
        MockSerialPort, MockTelemetrySource,
    };
    use astra_types::{AimCommand, Telemetry};
    use astra_ballistics::SimpleAimSolver;
    use astra_camera_gx::MockCameraSource;
    use astra_tracking::SimpleTracker;
    use astra_vision::MockDetector;

    #[test]
    fn latest_value_replaces_previous_value() {
        let store = LatestValue::default();
        store.store(1_u32);
        store.store(2_u32);

        assert_eq!(store.snapshot(), Some(2));

        store.clear();
        assert_eq!(store.snapshot(), None);
    }

    #[test]
    fn runtime_handles_store_latest_telemetry_and_command() {
        let runner = PipelineRunner::new(AppConfig::default());
        let handles = runner.handles();

        handles.latest_telemetry.store(Telemetry::default());
        handles.latest_command.store(AimCommand::default());

        assert!(handles.latest_telemetry.snapshot().is_some());
        assert!(handles.latest_command.snapshot().is_some());
    }

    #[test]
    fn run_cycle_updates_handles_and_emits_command() {
        let runner = PipelineRunner::new(AppConfig::default());
        let mut camera = MockCameraSource::default();
        let mut detector = MockDetector::default();
        let mut tracker = SimpleTracker;
        let solver = SimpleAimSolver;
        let mut telemetry = MockTelemetrySource::default();
        let mut sink = MockCommandSink::default();

        let command = runner
            .run_cycle(
                &mut camera,
                &mut detector,
                &mut tracker,
                &solver,
                &mut telemetry,
                &mut sink,
            )
            .unwrap();

        assert!(command.is_some());
        assert!(runner.handles().latest_frame.snapshot().is_some());
        assert!(runner.handles().latest_telemetry.snapshot().is_some());
        assert!(runner.handles().latest_command.snapshot().is_some());
        assert_eq!(sink.sent().len(), 1);
    }

    #[test]
    fn start_runs_mock_mode_without_error() {
        let runner = PipelineRunner::new(AppConfig::default());
        let report = runner.start().unwrap();
        assert!(matches!(report, PipelineStartReport::Mock(_)));
        assert!(runner.handles().latest_command.snapshot().is_some());
    }

    #[test]
    fn threaded_mock_pipeline_populates_handles() {
        let runner = PipelineRunner::new(AppConfig::default());

        let report = runner.run_threaded_mock_pipeline().unwrap();

        assert_eq!(report.frames_captured, runner.config.runtime.mock_cycles);
        assert!(report.telemetry_updates >= 1);
        assert!(report.detections_produced >= 1);
        assert!(report.commands_sent >= 1);
        assert!(runner.handles().latest_frame.snapshot().is_some());
        assert!(runner.handles().latest_telemetry.snapshot().is_some());
        assert!(runner.handles().latest_command.snapshot().is_some());
    }

    #[test]
    fn mode_parser_supports_mock_and_real() {
        assert_eq!(RuntimeMode::parse("mock"), RuntimeMode::Mock);
        assert_eq!(RuntimeMode::parse("real"), RuntimeMode::Real);
        assert_eq!(RuntimeMode::parse("unexpected"), RuntimeMode::Mock);
    }

    #[test]
    fn selection_defaults_follow_mode_and_component_settings() {
        let runner = PipelineRunner::new(AppConfig::default());
        assert_eq!(
            runner.selection(),
            RuntimeSelection {
                mode: RuntimeMode::Mock,
                serial: SerialBackend::Mock,
                camera: CameraSourceKind::Mock,
                detector: DetectorBackend::Mock,
            }
        );

        let mut config = AppConfig::default();
        config.app.mode = "real".to_string();
        let runner = PipelineRunner::new(config);
        assert_eq!(runner.selection().serial, SerialBackend::Real);
        assert_eq!(runner.selection().camera, CameraSourceKind::Mock);
        assert_eq!(runner.selection().detector, DetectorBackend::Mock);
    }

    #[test]
    fn prepare_returns_real_assembly_report() {
        let mut config = AppConfig::default();
        config.app.mode = "real".to_string();
        let assembly = super::RealRuntimeAssembly::from_config(&config);

        let report = assembly.prepare(&super::RuntimeHandles::default()).unwrap();

        match report {
            super::RealPipelineReport {
                serial_ready,
                camera_ready,
                detector_ready,
                ..
            } => {
                assert!(serial_ready);
                assert!(camera_ready);
                assert!(detector_ready);
            }
        }
    }

    #[test]
    fn real_assembly_serial_builder_rejects_invalid_config() {
        let mut config = AppConfig::default();
        config.app.mode = "real".to_string();
        config.serial.read_chunk_size = 0;
        let assembly = super::RealRuntimeAssembly::from_config(&config);

        let error = assembly.build_serial_io().unwrap_err();
        assert!(error.to_string().contains("serial configuration is invalid"));
    }

    #[test]
    fn real_serial_mock_vision_pipeline_runs_with_mock_serial() {
        let mut config = AppConfig::default();
        config.app.mode = "real".to_string();
        config.runtime.mock_cycles = 4;
        let runner = PipelineRunner::new(config.clone());
        let codec = legacy_codec_from_serial_config(&config.serial);
        let transport = MockSerialPort::default();
        for sequence in 0..config.runtime.mock_cycles {
            transport.push_rx_frame(super::build_mock_telemetry_frame(sequence));
        }
        let mut telemetry_source = LegacyTelemetrySource::new(transport.clone(), codec);
        let mut command_sink = LegacyCommandSink::new(transport.clone(), codec);

        let report = runner
            .run_real_serial_mock_vision_pipeline(&mut telemetry_source, &mut command_sink)
            .unwrap();

        assert_eq!(report.cycles_completed, 4);
        assert_eq!(report.commands_sent, 4);
        assert_eq!(transport.take_tx_frames().len(), 4);
        assert!(runner.handles().latest_command.snapshot().is_some());
    }

    #[test]
    fn start_returns_mock_report_when_in_mock_mode() {
        let runner = PipelineRunner::new(AppConfig::default());
        let report = runner.start().unwrap();

        assert!(matches!(report, PipelineStartReport::Mock(_)));
    }

    #[test]
    fn unsupported_selection_returns_error() {
        let mut config = AppConfig::default();
        config.camera.source = "gx".to_string();
        let runner = PipelineRunner::new(config);

        let error = runner.start().unwrap_err();
        assert!(
            error.to_string().contains("requires building astra-runtime with the 'ort-detector' feature")
                || error.to_string().contains("requires building astra-runtime with the 'gx-camera' feature")
                || error.to_string().contains("requires building astra-camera-gx with the 'gx' feature")
                || error.to_string().contains("unsupported runtime selection")
        );
    }

    #[cfg(feature = "ort-detector")]
    #[test]
    fn ort_pipeline_rejects_missing_model_path() {
        let mut config = AppConfig::default();
        config.detector.backend = Some("ort".to_string());
        config.detector.model_path = "missing.onnx".to_string();
        let runner = PipelineRunner::new(config);

        let error = runner.run_mock_serial_mock_camera_ort_pipeline().unwrap_err();
        assert!(error.to_string().contains("model path does not exist"));
    }

    #[cfg(feature = "gx-camera")]
    #[test]
    fn gx_pipeline_rejects_invalid_camera_config() {
        let mut config = AppConfig::default();
        config.camera.source = "gx".to_string();
        config.camera.device_index = 0;
        let runner = PipelineRunner::new(config);

        let error = runner.run_mock_serial_gx_camera_pipeline().unwrap_err();
        assert!(error.to_string().contains("camera configuration is invalid"));
    }
}
