use anyhow::Result;
use astra_types::{AimSolution, Telemetry, TrackState};

pub trait AimSolver {
    fn solve(&self, track: &TrackState, telemetry: &Telemetry) -> Result<AimSolution>;
}

#[derive(Debug, Default)]
pub struct StubAimSolver;

impl AimSolver for StubAimSolver {
    fn solve(&self, _track: &TrackState, _telemetry: &Telemetry) -> Result<AimSolution> {
        Ok(AimSolution::default())
    }
}

#[derive(Debug, Default)]
pub struct SimpleAimSolver;

impl AimSolver for SimpleAimSolver {
    fn solve(&self, track: &TrackState, _telemetry: &Telemetry) -> Result<AimSolution> {
        Ok(AimSolution {
            pitch_rad: track.center_px.y * 0.001,
            yaw_rad: track.center_px.x * 0.001,
            fire: track.confidence >= 0.8,
        })
    }
}
