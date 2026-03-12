use std::time::Instant;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TeamColor {
    #[default]
    Unknown,
    Red,
    Blue,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OperatingMode {
    #[default]
    Unknown,
    Idle,
    Armor,
    Energy,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Point2 {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BoundingBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FramePixelFormat {
    #[default]
    Unknown,
    Gray8,
    Bgr8,
    NativeRaw,
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub sequence: u64,
    pub captured_at: Instant,
    pub width: u32,
    pub height: u32,
    pub pixel_format: FramePixelFormat,
    pub row_stride_bytes: usize,
    pub source_pixel_format: Option<i32>,
    pub bytes: Vec<u8>,
}

impl Frame {
    pub fn expected_data_len(&self) -> Option<usize> {
        let channels = match self.pixel_format {
            FramePixelFormat::Gray8 => 1,
            FramePixelFormat::Bgr8 => 3,
            FramePixelFormat::Unknown | FramePixelFormat::NativeRaw => return None,
        };

        usize::try_from(self.width).ok()?.checked_mul(usize::try_from(self.height).ok()?)?.checked_mul(channels)
    }

    pub fn validate_layout(&self) -> bool {
        if self.width == 0 || self.height == 0 {
            return self.bytes.is_empty();
        }

        match self.expected_data_len() {
            Some(expected_len) => self.bytes.len() == expected_len && self.row_stride_bytes >= expected_len / self.height as usize,
            None => self.row_stride_bytes > 0 && !self.bytes.is_empty(),
        }
    }
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            sequence: 0,
            captured_at: Instant::now(),
            width: 0,
            height: 0,
            pixel_format: FramePixelFormat::Unknown,
            row_stride_bytes: 0,
            source_pixel_format: None,
            bytes: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Frame, FramePixelFormat};

    #[test]
    fn frame_layout_validation_accepts_dense_bgr8() {
        let frame = Frame {
            width: 4,
            height: 2,
            pixel_format: FramePixelFormat::Bgr8,
            row_stride_bytes: 12,
            bytes: vec![0; 24],
            ..Frame::default()
        };

        assert_eq!(frame.expected_data_len(), Some(24));
        assert!(frame.validate_layout());
    }

    #[test]
    fn frame_layout_validation_rejects_short_buffer() {
        let frame = Frame {
            width: 4,
            height: 2,
            pixel_format: FramePixelFormat::Gray8,
            row_stride_bytes: 4,
            bytes: vec![0; 7],
            ..Frame::default()
        };

        assert!(!frame.validate_layout());
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GimbalPose {
    pub pitch_rad: f32,
    pub yaw_rad: f32,
    pub roll_rad: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TargetPosition {
    pub x: i16,
    pub y: i16,
}

#[derive(Debug, Clone)]
pub struct Detection {
    pub bbox: BoundingBox,
    pub confidence: f32,
    pub class_id: i32,
    pub center: Point2,
    pub observed_at: Instant,
}

impl Default for Detection {
    fn default() -> Self {
        Self {
            bbox: BoundingBox::default(),
            confidence: 0.0,
            class_id: 0,
            center: Point2::default(),
            observed_at: Instant::now(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DetectionBatch {
    pub frame_sequence: u64,
    pub detections: Vec<Detection>,
}

#[derive(Debug, Clone)]
pub struct Telemetry {
    pub raw_mode: u8,
    pub mode: OperatingMode,
    pub robot_id: u8,
    pub color: TeamColor,
    pub shoot_speed: f32,
    pub pose: GimbalPose,
    pub updated_at: Instant,
}

impl Default for Telemetry {
    fn default() -> Self {
        Self {
            raw_mode: 0,
            mode: OperatingMode::Idle,
            robot_id: 0,
            color: TeamColor::Unknown,
            shoot_speed: 0.0,
            pose: GimbalPose::default(),
            updated_at: Instant::now(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TrackState {
    pub target_id: u64,
    pub center_px: Point2,
    pub velocity_px: Point2,
    pub confidence: f32,
    pub updated_at: Instant,
}

impl Default for TrackState {
    fn default() -> Self {
        Self {
            target_id: 0,
            center_px: Point2::default(),
            velocity_px: Point2::default(),
            confidence: 0.0,
            updated_at: Instant::now(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AimSolution {
    pub pitch_rad: f32,
    pub yaw_rad: f32,
    pub fire: bool,
}

#[derive(Debug, Clone)]
pub struct AimCommand {
    pub detect_flag: bool,
    pub pitch_rad: f32,
    pub yaw_rad: f32,
    pub target_position: TargetPosition,
    pub fire: bool,
    pub generated_at: Instant,
}

impl Default for AimCommand {
    fn default() -> Self {
        Self {
            detect_flag: false,
            pitch_rad: 0.0,
            yaw_rad: 0.0,
            target_position: TargetPosition::default(),
            fire: false,
            generated_at: Instant::now(),
        }
    }
}
