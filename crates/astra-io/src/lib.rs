use anyhow::{anyhow, Result};
use astra_config::SerialConfig;
use astra_protocol::{FrameCodec, LegacyFrameCodec, LegacyProtocolConfig};
use astra_types::{AimCommand, Telemetry};
use parking_lot::Mutex;
use serialport::SerialPort;
use std::{
    collections::VecDeque,
    io::{ErrorKind, Read, Write},
    sync::Arc,
    time::Duration,
};
use thiserror::Error;

pub trait TelemetrySource {
    fn recv(&mut self) -> Result<Telemetry>;
}

pub trait CommandSink {
    fn send(&mut self, command: &AimCommand) -> Result<()>;
}

pub trait SerialTransport: Clone + Send + Sync + 'static {
    fn read_chunk(&self) -> Result<Option<Vec<u8>>>;
    fn write_frame(&self, frame: &[u8]) -> Result<()>;
}

#[derive(Debug, Error)]
pub enum IoError {
    #[error("no complete telemetry frame is available")]
    NoTelemetryFrame,

    #[error("serial configuration is invalid: {0}")]
    InvalidSerialConfig(&'static str),
}

#[derive(Debug, Default)]
struct MockSerialState {
    inbound_chunks: VecDeque<Vec<u8>>,
    outbound_frames: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, Default)]
pub struct MockSerialPort {
    inner: Arc<Mutex<MockSerialState>>,
}

impl MockSerialPort {
    pub fn push_rx_chunk(&self, chunk: impl Into<Vec<u8>>) {
        self.inner.lock().inbound_chunks.push_back(chunk.into());
    }

    pub fn push_rx_frame(&self, frame: impl Into<Vec<u8>>) {
        self.push_rx_chunk(frame);
    }

    pub fn take_tx_frames(&self) -> Vec<Vec<u8>> {
        let mut state = self.inner.lock();
        std::mem::take(&mut state.outbound_frames)
    }

    pub fn pending_rx_chunks(&self) -> usize {
        self.inner.lock().inbound_chunks.len()
    }
}

impl SerialTransport for MockSerialPort {
    fn read_chunk(&self) -> Result<Option<Vec<u8>>> {
        Ok(self.inner.lock().inbound_chunks.pop_front())
    }

    fn write_frame(&self, frame: &[u8]) -> Result<()> {
        self.inner.lock().outbound_frames.push(frame.to_vec());
        Ok(())
    }
}

struct RealSerialState {
    read_port: Box<dyn SerialPort>,
    write_port: Box<dyn SerialPort>,
    read_chunk_size: usize,
}

#[derive(Clone)]
pub struct RealSerialPort {
    inner: Arc<Mutex<RealSerialState>>,
    read_device: String,
    write_device: String,
}

impl std::fmt::Debug for RealSerialPort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RealSerialPort")
            .field("read_device", &self.read_device)
            .field("write_device", &self.write_device)
            .finish()
    }
}

impl RealSerialPort {
    pub fn open(config: &SerialConfig) -> Result<Self> {
        if config.read_device.trim().is_empty() {
            return Err(IoError::InvalidSerialConfig("read_device is empty").into());
        }
        if config.write_device.trim().is_empty() {
            return Err(IoError::InvalidSerialConfig("write_device is empty").into());
        }
        if config.read_chunk_size == 0 {
            return Err(IoError::InvalidSerialConfig("read_chunk_size must be greater than zero").into());
        }

        let read_timeout = Duration::from_millis(config.read_timeout_ms.max(1));
        let write_timeout = Duration::from_millis(config.write_timeout_ms.max(1));

        let read_port = serialport::new(&config.read_device, config.baud_read)
            .timeout(read_timeout)
            .open()?;

        let write_port = if config.read_device == config.write_device && config.baud_read == config.baud_write {
            read_port.try_clone()?
        } else {
            serialport::new(&config.write_device, config.baud_write)
                .timeout(write_timeout)
                .open()?
        };

        Ok(Self {
            inner: Arc::new(Mutex::new(RealSerialState {
                read_port,
                write_port,
                read_chunk_size: config.read_chunk_size,
            })),
            read_device: config.read_device.clone(),
            write_device: config.write_device.clone(),
        })
    }

    pub fn device_names(&self) -> (&str, &str) {
        (&self.read_device, &self.write_device)
    }
}

impl SerialTransport for RealSerialPort {
    fn read_chunk(&self) -> Result<Option<Vec<u8>>> {
        let mut state = self.inner.lock();
        let mut buffer = vec![0_u8; state.read_chunk_size];

        match state.read_port.read(&mut buffer) {
            Ok(0) => Ok(None),
            Ok(bytes_read) => {
                buffer.truncate(bytes_read);
                Ok(Some(buffer))
            }
            Err(error) if matches!(error.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn write_frame(&self, frame: &[u8]) -> Result<()> {
        let mut state = self.inner.lock();
        state.write_port.write_all(frame)?;
        state.write_port.flush()?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct LegacyTelemetrySource<T> {
    transport: T,
    codec: LegacyFrameCodec,
    raw_buffer: Vec<u8>,
}

impl<T> LegacyTelemetrySource<T> {
    pub fn new(transport: T, codec: LegacyFrameCodec) -> Self {
        Self {
            transport,
            codec,
            raw_buffer: Vec::new(),
        }
    }

    pub fn codec(&self) -> LegacyFrameCodec {
        self.codec
    }
}

impl<T> TelemetrySource for LegacyTelemetrySource<T>
where
    T: SerialTransport,
{
    fn recv(&mut self) -> Result<Telemetry> {
        loop {
            if let Some(frame) = self.codec.try_extract_telemetry_frame(&mut self.raw_buffer) {
                return self
                    .codec
                    .decode_telemetry_frame(&frame)
                    .map_err(|error| anyhow!(error));
            }

            match self.transport.read_chunk()? {
                Some(chunk) => self.raw_buffer.extend(chunk),
                None => return Err(IoError::NoTelemetryFrame.into()),
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct LegacyCommandSink<T> {
    transport: T,
    codec: LegacyFrameCodec,
}

impl<T> LegacyCommandSink<T> {
    pub fn new(transport: T, codec: LegacyFrameCodec) -> Self {
        Self { transport, codec }
    }
}

impl<T> CommandSink for LegacyCommandSink<T>
where
    T: SerialTransport,
{
    fn send(&mut self, command: &AimCommand) -> Result<()> {
        let frame = self
            .codec
            .encode_command(command)
            .map_err(|error| anyhow!(error))?;
        self.transport.write_frame(&frame)
    }
}

pub fn legacy_codec_from_serial_config(config: &SerialConfig) -> LegacyFrameCodec {
    LegacyFrameCodec::new(LegacyProtocolConfig {
        tx_header: config.protocol.tx_header,
        rx_header: config.protocol.rx_header,
        frame_footer: config.protocol.frame_footer,
        tx_payload_len: config.protocol.tx_payload_len,
        rx_payload_len: config.protocol.rx_payload_len,
    })
}

pub fn build_real_legacy_serial_io(
    config: &SerialConfig,
) -> Result<(LegacyTelemetrySource<RealSerialPort>, LegacyCommandSink<RealSerialPort>)> {
    let transport = RealSerialPort::open(config)?;
    let codec = legacy_codec_from_serial_config(config);
    let source = LegacyTelemetrySource::new(transport.clone(), codec);
    let sink = LegacyCommandSink::new(transport, codec);
    Ok((source, sink))
}

#[derive(Debug, Clone)]
pub struct MockTelemetrySource {
    telemetry: Telemetry,
}

impl Default for MockTelemetrySource {
    fn default() -> Self {
        Self {
            telemetry: Telemetry::default(),
        }
    }
}

impl TelemetrySource for MockTelemetrySource {
    fn recv(&mut self) -> Result<Telemetry> {
        Ok(self.telemetry.clone())
    }
}

#[derive(Debug, Default)]
pub struct MockCommandSink {
    sent: Vec<AimCommand>,
}

impl MockCommandSink {
    pub fn sent(&self) -> &[AimCommand] {
        &self.sent
    }
}

impl CommandSink for MockCommandSink {
    fn send(&mut self, command: &AimCommand) -> Result<()> {
        self.sent.push(command.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_real_legacy_serial_io, legacy_codec_from_serial_config, CommandSink, IoError,
        LegacyCommandSink, LegacyTelemetrySource, MockSerialPort, RealSerialPort, TelemetrySource,
    };
    use astra_config::SerialConfig;
    use astra_types::{AimCommand, OperatingMode, TargetPosition, TeamColor};

    #[test]
    fn telemetry_source_reads_frame_from_mock_serial() {
        let transport = MockSerialPort::default();
        transport.push_rx_chunk(vec![0x00, 0x45, 12, 0x10, 0x27]);
        transport.push_rx_chunk(vec![0x20, 0x4E, 30, 0xFF]);

        let codec = legacy_codec_from_serial_config(&SerialConfig::default());
        let mut source = LegacyTelemetrySource::new(transport, codec);

        let telemetry = source.recv().unwrap();
        assert_eq!(telemetry.raw_mode, 12);
        assert_eq!(telemetry.mode, OperatingMode::Armor);
        assert_eq!(telemetry.color, TeamColor::Red);
        assert_eq!(telemetry.robot_id, 2);
    }

    #[test]
    fn telemetry_source_reports_empty_when_no_frame_available() {
        let transport = MockSerialPort::default();
        let codec = legacy_codec_from_serial_config(&SerialConfig::default());
        let mut source = LegacyTelemetrySource::new(transport, codec);

        let error = source.recv().unwrap_err();
        assert!(error.downcast_ref::<IoError>().is_some());
    }

    #[test]
    fn command_sink_writes_encoded_frame_to_mock_serial() {
        let transport = MockSerialPort::default();
        let codec = legacy_codec_from_serial_config(&SerialConfig::default());
        let mut sink = LegacyCommandSink::new(transport.clone(), codec);
        let command = AimCommand {
            detect_flag: true,
            fire: true,
            target_position: TargetPosition { x: 5, y: -8 },
            ..AimCommand::default()
        };

        sink.send(&command).unwrap();
        let frames = transport.take_tx_frames();

        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].len(), 12);
        assert_eq!(frames[0][0], 0x43);
        assert_eq!(frames[0][11], 0xFF);
        assert_eq!(frames[0][1], 1);
        assert_eq!(frames[0][2], 1);
    }

    #[test]
    fn real_serial_port_rejects_empty_device_names() {
        let mut config = SerialConfig::default();
        config.read_device.clear();

        let error = RealSerialPort::open(&config).unwrap_err();
        assert!(error.downcast_ref::<IoError>().is_some());
    }

    #[test]
    fn build_real_serial_io_rejects_zero_chunk_size() {
        let mut config = SerialConfig::default();
        config.read_chunk_size = 0;

        let error = build_real_legacy_serial_io(&config).unwrap_err();
        assert!(error.downcast_ref::<IoError>().is_some());
    }
}
