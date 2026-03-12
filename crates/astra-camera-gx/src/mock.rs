use crate::CameraSource;
use anyhow::Result;
use astra_types::{Frame, FramePixelFormat};
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct MockCameraSource {
    sequence: u64,
    width: u32,
    height: u32,
}

impl MockCameraSource {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            sequence: 0,
            width,
            height,
        }
    }
}

impl Default for MockCameraSource {
    fn default() -> Self {
        Self::new(1280, 1024)
    }
}

impl CameraSource for MockCameraSource {
    fn next_frame(&mut self) -> Result<Frame> {
        self.sequence += 1;
        let row_stride_bytes = self.width as usize * 3;
        let bytes_len = row_stride_bytes * self.height as usize;
        Ok(Frame {
            sequence: self.sequence,
            captured_at: Instant::now(),
            width: self.width,
            height: self.height,
            pixel_format: FramePixelFormat::Bgr8,
            row_stride_bytes,
            source_pixel_format: None,
            bytes: vec![self.sequence as u8; bytes_len],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::MockCameraSource;
    use crate::CameraSource;

    #[test]
    fn mock_camera_produces_incrementing_frames() {
        let mut camera = MockCameraSource::new(64, 48);

        let first = camera.next_frame().unwrap();
        let second = camera.next_frame().unwrap();

        assert_eq!(first.sequence, 1);
        assert_eq!(second.sequence, 2);
        assert_eq!(second.width, 64);
        assert_eq!(second.height, 48);
        assert_eq!(second.pixel_format, astra_types::FramePixelFormat::Bgr8);
        assert_eq!(second.row_stride_bytes, 64 * 3);
        assert!(second.validate_layout());
    }
}
