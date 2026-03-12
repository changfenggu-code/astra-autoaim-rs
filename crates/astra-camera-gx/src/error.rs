use thiserror::Error;

#[derive(Debug, Error)]
pub enum CameraError {
    #[error("camera source is not available: {0}")]
    Unavailable(&'static str),

    #[error("camera configuration is invalid: {0}")]
    InvalidConfig(&'static str),

    #[error("camera SDK call failed: {operation} returned status {status}")]
    SdkCallFailed { operation: &'static str, status: i32 },
}
