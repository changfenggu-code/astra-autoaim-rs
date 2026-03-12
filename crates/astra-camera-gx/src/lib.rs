mod error;

#[cfg(feature = "gx")]
mod ffi;

#[cfg(feature = "gx")]
mod gx;

#[cfg(feature = "mock")]
mod mock;

use anyhow::Result;
use astra_types::Frame;

pub use error::CameraError;

#[cfg(feature = "gx")]
pub use gx::{GxCameraConfig, GxCameraSource};

#[cfg(feature = "mock")]
pub use mock::MockCameraSource;

pub trait CameraSource {
    fn next_frame(&mut self) -> Result<Frame>;
}

pub const MOCK_FEATURE_ENABLED: bool = cfg!(feature = "mock");
pub const GX_FEATURE_ENABLED: bool = cfg!(feature = "gx");

#[cfg(test)]
mod tests {
    use super::{GX_FEATURE_ENABLED, MOCK_FEATURE_ENABLED};

    #[test]
    fn default_feature_state_is_reported() {
        assert!(MOCK_FEATURE_ENABLED);
        assert!(!GX_FEATURE_ENABLED);
    }
}
