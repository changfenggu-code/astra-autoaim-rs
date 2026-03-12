use crate::{preprocess_frame, Detector, VisionError};
use astra_types::{BoundingBox, Detection, DetectionBatch, Frame, Point2};
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct MockDetector {
    pub confidence_threshold: f32,
}

impl Default for MockDetector {
    fn default() -> Self {
        Self {
            confidence_threshold: 0.5,
        }
    }
}

impl Detector for MockDetector {
    fn detect(&mut self, frame: &Frame) -> Result<DetectionBatch, VisionError> {
        let processed = preprocess_frame(frame)?;
        let width = processed.width.max(1) as f32;
        let height = processed.height.max(1) as f32;
        let center = Point2 {
            x: (frame.sequence % width as u64) as f32,
            y: ((frame.sequence * 2) % height as u64) as f32,
        };

        let detection = Detection {
            bbox: BoundingBox {
                x: center.x,
                y: center.y,
                width: 32.0,
                height: 16.0,
            },
            confidence: self.confidence_threshold.max(0.8),
            class_id: 1,
            center,
            observed_at: Instant::now(),
        };

        Ok(DetectionBatch {
            frame_sequence: frame.sequence,
            detections: vec![detection],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::MockDetector;
    use crate::Detector;
    use astra_types::{Frame, FramePixelFormat};

    #[test]
    fn mock_detector_uses_preprocess_contract() {
        let frame = Frame {
            width: 8,
            height: 4,
            pixel_format: FramePixelFormat::Bgr8,
            row_stride_bytes: 24,
            bytes: vec![42; 96],
            ..Frame::default()
        };
        let mut detector = MockDetector::default();

        let batch = detector.detect(&frame).unwrap();
        assert_eq!(batch.frame_sequence, 0);
        assert_eq!(batch.detections.len(), 1);
    }
}
