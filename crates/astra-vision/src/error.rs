use astra_types::FramePixelFormat;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VisionError {
    #[error("detector is not configured")]
    NotConfigured,

    #[error("unsupported frame pixel format: {0:?}")]
    UnsupportedPixelFormat(FramePixelFormat),

    #[error("invalid frame layout for format {pixel_format:?}: width={width}, height={height}, stride={stride}, bytes={bytes}")]
    InvalidFrameLayout {
        pixel_format: FramePixelFormat,
        width: u32,
        height: u32,
        stride: usize,
        bytes: usize,
    },

    #[error("detector backend is unavailable: {0}")]
    BackendUnavailable(&'static str),

    #[error("detector backend error: {0}")]
    Backend(String),
}
