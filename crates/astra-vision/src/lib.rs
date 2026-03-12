mod error;
mod preprocess;

#[cfg(feature = "mock")]
mod mock;

#[cfg(feature = "ort")]
mod ort;

use astra_types::{DetectionBatch, Frame};

pub use error::VisionError;
pub use preprocess::{preprocess_frame, PreprocessedFrame};

#[cfg(feature = "mock")]
pub use mock::MockDetector;

#[cfg(feature = "ort")]
pub use ort::{OrtDetector, OrtDetectorConfig};

pub trait Detector {
    fn detect(&mut self, frame: &Frame) -> Result<DetectionBatch, VisionError>;
}

#[derive(Debug, Clone)]
pub struct StubDetector {
    pub confidence_threshold: f32,
}

impl Default for StubDetector {
    fn default() -> Self {
        Self {
            confidence_threshold: 0.5,
        }
    }
}

impl Detector for StubDetector {
    fn detect(&mut self, frame: &Frame) -> Result<DetectionBatch, VisionError> {
        Ok(DetectionBatch {
            frame_sequence: frame.sequence,
            detections: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn feature_state_is_consistent() {
        assert!(cfg!(feature = "mock"));
        let _ort_enabled = cfg!(feature = "ort");
    }
}
