use astra_types::{DetectionBatch, Telemetry, TrackState};

pub trait Tracker {
    fn update(&mut self, detections: &DetectionBatch, telemetry: &Telemetry) -> Option<TrackState>;
}

#[derive(Debug, Default)]
pub struct StubTracker;

impl Tracker for StubTracker {
    fn update(&mut self, detections: &DetectionBatch, _telemetry: &Telemetry) -> Option<TrackState> {
        if detections.detections.is_empty() {
            None
        } else {
            Some(TrackState::default())
        }
    }
}

#[derive(Debug, Default)]
pub struct SimpleTracker;

impl Tracker for SimpleTracker {
    fn update(&mut self, detections: &DetectionBatch, _telemetry: &Telemetry) -> Option<TrackState> {
        let detection = detections.detections.first()?;
        Some(TrackState {
            target_id: detection.class_id as u64,
            center_px: detection.center,
            velocity_px: Default::default(),
            confidence: detection.confidence,
            updated_at: detection.observed_at,
        })
    }
}
