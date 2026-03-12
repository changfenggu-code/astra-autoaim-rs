use crate::VisionError;
use astra_types::{Frame, FramePixelFormat};

#[derive(Debug, Clone)]
pub struct PreprocessedFrame {
    pub width: u32,
    pub height: u32,
    pub channels: usize,
    pub source_format: FramePixelFormat,
    pub data: Vec<f32>,
}

impl PreprocessedFrame {
    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

pub fn preprocess_frame(frame: &Frame) -> Result<PreprocessedFrame, VisionError> {
    if !frame.validate_layout() {
        return Err(VisionError::InvalidFrameLayout {
            pixel_format: frame.pixel_format,
            width: frame.width,
            height: frame.height,
            stride: frame.row_stride_bytes,
            bytes: frame.bytes.len(),
        });
    }

    match frame.pixel_format {
        FramePixelFormat::Bgr8 => Ok(preprocess_bgr8(frame)),
        format => Err(VisionError::UnsupportedPixelFormat(format)),
    }
}

fn preprocess_bgr8(frame: &Frame) -> PreprocessedFrame {
    let data = frame
        .bytes
        .iter()
        .map(|byte| *byte as f32 / 255.0)
        .collect();

    PreprocessedFrame {
        width: frame.width,
        height: frame.height,
        channels: 3,
        source_format: frame.pixel_format,
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::preprocess_frame;
    use crate::VisionError;
    use astra_types::{Frame, FramePixelFormat};

    #[test]
    fn preprocess_accepts_bgr8_frame() {
        let frame = Frame {
            width: 2,
            height: 1,
            pixel_format: FramePixelFormat::Bgr8,
            row_stride_bytes: 6,
            bytes: vec![0, 127, 255, 10, 20, 30],
            ..Frame::default()
        };

        let processed = preprocess_frame(&frame).unwrap();
        assert_eq!(processed.channels, 3);
        assert_eq!(processed.len(), 6);
    }

    #[test]
    fn preprocess_rejects_native_raw() {
        let frame = Frame {
            width: 2,
            height: 2,
            pixel_format: FramePixelFormat::NativeRaw,
            row_stride_bytes: 2,
            bytes: vec![0; 4],
            ..Frame::default()
        };

        let error = preprocess_frame(&frame).unwrap_err();
        assert!(matches!(error, VisionError::UnsupportedPixelFormat(FramePixelFormat::NativeRaw)));
    }
}
