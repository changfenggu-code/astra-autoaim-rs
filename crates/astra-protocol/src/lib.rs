use astra_types::{AimCommand, GimbalPose, OperatingMode, TeamColor, Telemetry};
use std::time::Instant;
use thiserror::Error;

pub const DEFAULT_TX_PAYLOAD_LEN: usize = 10;
pub const DEFAULT_RX_PAYLOAD_LEN: usize = 6;
pub const DEFAULT_TX_FRAME_LEN: usize = DEFAULT_TX_PAYLOAD_LEN + 2;
pub const DEFAULT_RX_FRAME_LEN: usize = DEFAULT_RX_PAYLOAD_LEN + 2;

#[derive(Debug, Error)]
pub enum CodecError {
    #[error("payload is empty")]
    EmptyPayload,

    #[error("invalid frame length: expected {expected}, got {actual}")]
    InvalidFrameLength { expected: usize, actual: usize },

    #[error("invalid payload length: expected {expected}, got {actual}")]
    InvalidPayloadLength { expected: usize, actual: usize },

    #[error("invalid frame header: expected 0x{expected:02X}, got 0x{actual:02X}")]
    InvalidHeader { expected: u8, actual: u8 },

    #[error("invalid frame footer: expected 0x{expected:02X}, got 0x{actual:02X}")]
    InvalidFooter { expected: u8, actual: u8 },
}

pub trait FrameCodec {
    fn encode_command(&self, command: &AimCommand) -> Result<Vec<u8>, CodecError>;
    fn decode_telemetry(&self, payload: &[u8]) -> Result<Telemetry, CodecError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyProtocolConfig {
    pub tx_header: u8,
    pub rx_header: u8,
    pub frame_footer: u8,
    pub tx_payload_len: usize,
    pub rx_payload_len: usize,
}

impl Default for LegacyProtocolConfig {
    fn default() -> Self {
        Self {
            tx_header: 0x43,
            rx_header: 0x45,
            frame_footer: 0xFF,
            tx_payload_len: DEFAULT_TX_PAYLOAD_LEN,
            rx_payload_len: DEFAULT_RX_PAYLOAD_LEN,
        }
    }
}

impl LegacyProtocolConfig {
    pub fn tx_frame_len(&self) -> usize {
        self.tx_payload_len + 2
    }

    pub fn rx_frame_len(&self) -> usize {
        self.rx_payload_len + 2
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct LegacyFrameCodec {
    config: LegacyProtocolConfig,
}

impl LegacyFrameCodec {
    pub fn new(config: LegacyProtocolConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> LegacyProtocolConfig {
        self.config
    }

    pub fn encode_command_payload(&self, command: &AimCommand) -> Result<Vec<u8>, CodecError> {
        let mut payload = vec![0u8; self.config.tx_payload_len];
        if payload.len() != DEFAULT_TX_PAYLOAD_LEN {
            return Err(CodecError::InvalidPayloadLength {
                expected: DEFAULT_TX_PAYLOAD_LEN,
                actual: payload.len(),
            });
        }

        let yaw_encoder = encode_angle_rad(command.yaw_rad);
        let pitch_encoder = encode_angle_rad(command.pitch_rad);
        let target_x = command.target_position.x as u16;
        let target_y = command.target_position.y as u16;

        payload[0] = command.detect_flag as u8;
        payload[1] = command.fire as u8;
        payload[2..4].copy_from_slice(&yaw_encoder.to_le_bytes());
        payload[4..6].copy_from_slice(&pitch_encoder.to_le_bytes());
        payload[6..8].copy_from_slice(&target_x.to_le_bytes());
        payload[8..10].copy_from_slice(&target_y.to_le_bytes());
        Ok(payload)
    }

    pub fn encode_command_frame(&self, command: &AimCommand) -> Result<Vec<u8>, CodecError> {
        let payload = self.encode_command_payload(command)?;
        let mut frame = vec![0u8; self.config.tx_frame_len()];
        frame[0] = self.config.tx_header;
        frame[1..1 + payload.len()].copy_from_slice(&payload);
        frame[self.config.tx_frame_len() - 1] = self.config.frame_footer;
        Ok(frame)
    }

    pub fn decode_telemetry_frame(&self, frame: &[u8]) -> Result<Telemetry, CodecError> {
        if frame.len() != self.config.rx_frame_len() {
            return Err(CodecError::InvalidFrameLength {
                expected: self.config.rx_frame_len(),
                actual: frame.len(),
            });
        }
        if frame[0] != self.config.rx_header {
            return Err(CodecError::InvalidHeader {
                expected: self.config.rx_header,
                actual: frame[0],
            });
        }
        if frame[frame.len() - 1] != self.config.frame_footer {
            return Err(CodecError::InvalidFooter {
                expected: self.config.frame_footer,
                actual: frame[frame.len() - 1],
            });
        }

        self.decode_telemetry(&frame[1..frame.len() - 1])
    }

    pub fn try_extract_telemetry_frame(&self, raw: &mut Vec<u8>) -> Option<Vec<u8>> {
        let frame_len = self.config.rx_frame_len();
        let mut start = 0;

        while start < raw.len() {
            if raw[start] != self.config.rx_header {
                start += 1;
                continue;
            }

            if raw.len() - start < frame_len {
                if start > 0 {
                    raw.drain(..start);
                }
                return None;
            }

            let end = start + frame_len - 1;
            if raw[end] == self.config.frame_footer {
                let frame = raw[start..start + frame_len].to_vec();
                raw.drain(..start + frame_len);
                return Some(frame);
            }

            start += 1;
        }

        raw.clear();
        None
    }
}

impl FrameCodec for LegacyFrameCodec {
    fn encode_command(&self, command: &AimCommand) -> Result<Vec<u8>, CodecError> {
        self.encode_command_frame(command)
    }

    fn decode_telemetry(&self, payload: &[u8]) -> Result<Telemetry, CodecError> {
        if payload.is_empty() {
            return Err(CodecError::EmptyPayload);
        }
        if payload.len() != self.config.rx_payload_len {
            return Err(CodecError::InvalidPayloadLength {
                expected: self.config.rx_payload_len,
                actual: payload.len(),
            });
        }

        let raw_mode = payload[0];
        let yaw_encoder = u16::from_le_bytes([payload[1], payload[2]]);
        let pitch_encoder = u16::from_le_bytes([payload[3], payload[4]]);
        let (mode, color, robot_id) = decode_mode(raw_mode);

        Ok(Telemetry {
            raw_mode,
            mode,
            robot_id,
            color,
            shoot_speed: payload[5] as f32,
            pose: GimbalPose {
                pitch_rad: decode_angle_rad(pitch_encoder),
                yaw_rad: decode_angle_rad(yaw_encoder),
                roll_rad: 0.0,
            },
            updated_at: Instant::now(),
        })
    }
}

fn encode_angle_rad(angle_rad: f32) -> u16 {
    let encoded = angle_rad.to_degrees() * 100.0;
    if encoded >= 0.0 {
        encoded.trunc() as u16
    } else {
        (65535.0 + encoded).trunc() as u16
    }
}

fn decode_angle_rad(raw: u16) -> f32 {
    let mut angle = raw as f32;
    if angle > 30000.0 {
        angle = -(angle - 65535.0) / 100.0;
    } else {
        angle = -angle / 100.0;
    }
    angle.to_radians()
}

fn decode_mode(raw_mode: u8) -> (OperatingMode, TeamColor, u8) {
    match raw_mode / 10 {
        0 => (OperatingMode::Idle, TeamColor::Unknown, 0),
        1 => (OperatingMode::Armor, TeamColor::Red, raw_mode % 10),
        2 => (OperatingMode::Armor, TeamColor::Blue, raw_mode % 10),
        3 => {
            let color = match raw_mode % 10 {
                0 => TeamColor::Red,
                1 => TeamColor::Blue,
                _ => TeamColor::Unknown,
            };
            (OperatingMode::Energy, color, 0)
        }
        _ => (OperatingMode::Unknown, TeamColor::Unknown, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::{FrameCodec, LegacyFrameCodec};
    use astra_types::{AimCommand, OperatingMode, TargetPosition, TeamColor};

    #[test]
    fn encodes_command_into_legacy_frame() {
        let codec = LegacyFrameCodec::default();
        let command = AimCommand {
            detect_flag: true,
            fire: true,
            yaw_rad: 1.0,
            pitch_rad: -0.5,
            target_position: TargetPosition { x: 12, y: -34 },
            ..AimCommand::default()
        };

        let frame = codec.encode_command(&command).unwrap();
        assert_eq!(frame.len(), 12);
        assert_eq!(frame[0], 0x43);
        assert_eq!(frame[11], 0xFF);
        assert_eq!(frame[1], 1);
        assert_eq!(frame[2], 1);
    }

    #[test]
    fn decodes_legacy_telemetry_frame() {
        let codec = LegacyFrameCodec::default();
        let frame = vec![0x45, 12, 0x10, 0x27, 0x20, 0x4E, 30, 0xFF];

        let telemetry = codec.decode_telemetry_frame(&frame).unwrap();
        assert_eq!(telemetry.raw_mode, 12);
        assert_eq!(telemetry.mode, OperatingMode::Armor);
        assert_eq!(telemetry.color, TeamColor::Red);
        assert_eq!(telemetry.robot_id, 2);
        assert_eq!(telemetry.shoot_speed, 30.0);
    }

    #[test]
    fn extracts_valid_frame_from_noisy_buffer() {
        let codec = LegacyFrameCodec::default();
        let mut raw = vec![0x00, 0x11, 0x45, 1, 2, 3, 4, 5, 6, 0xFF, 0x99];

        let frame = codec.try_extract_telemetry_frame(&mut raw).unwrap();
        assert_eq!(frame, vec![0x45, 1, 2, 3, 4, 5, 6, 0xFF]);
        assert_eq!(raw, vec![0x99]);
    }
}
