use crate::{preprocess_frame, Detector, VisionError};
use anyhow::Result;
use astra_config::DetectorConfig;
use astra_types::{DetectionBatch, Frame};
use ndarray::Array4;
use ort::{inputs, session::Session, value::TensorRef};
use std::{path::{Path, PathBuf}, sync::OnceLock};

#[derive(Debug, Clone)]
pub struct OrtDetectorConfig {
    pub model_path: String,
    pub confidence_threshold: f32,
    pub input_width: u32,
    pub input_height: u32,
}

impl From<&DetectorConfig> for OrtDetectorConfig {
    fn from(value: &DetectorConfig) -> Self {
        Self {
            model_path: value.model_path.clone(),
            confidence_threshold: value.confidence_threshold,
            input_width: value.input_width,
            input_height: value.input_height,
        }
    }
}

#[derive(Debug)]
pub struct OrtDetector {
    config: OrtDetectorConfig,
    session: Session,
}

impl OrtDetector {
    pub fn new(config: OrtDetectorConfig) -> Result<Self> {
        if config.input_width == 0 || config.input_height == 0 {
            anyhow::bail!("detector input dimensions must be greater than zero");
        }

        if !Path::new(&config.model_path).exists() {
            anyhow::bail!("detector model path does not exist: {}", config.model_path);
        }

        ensure_ort_initialized()?;

        let session = Session::builder()?.commit_from_file(&config.model_path)?;

        Ok(Self { config, session })
    }

    pub fn config(&self) -> &OrtDetectorConfig {
        &self.config
    }
}

fn ensure_ort_initialized() -> Result<()> {
    static ORT_INIT_RESULT: OnceLock<Result<(), String>> = OnceLock::new();

    ORT_INIT_RESULT
        .get_or_init(|| {
            let dll_path = resolve_ort_dylib_path()
                .ok_or_else(|| "could not resolve onnxruntime.dll path".to_string())?;
            prepend_dll_directory_to_path(&dll_path)
                .map_err(|error| format!("failed to update PATH for ORT DLL: {error}"))?;
            ort::init_from(&dll_path)
                .map_err(|error| format!("failed to create ORT environment builder: {error}"))?
                .commit();
            Ok(())
        })
        .clone()
        .map_err(anyhow::Error::msg)
}

fn resolve_ort_dylib_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("ORT_DYLIB_PATH") {
        let path = PathBuf::from(path);
        if path.exists() {
            return Some(path);
        }
    }

    let candidates = [
        PathBuf::from(r"D:\projects\astra-autoaim-rs\third_party\onnxruntime\lib\onnxruntime.dll"),
        PathBuf::from(r"D:\projects\onnxruntime-win-x64-1.24.3\lib\onnxruntime.dll"),
        PathBuf::from("onnxruntime.dll"),
        PathBuf::from("bin/onnxruntime.dll"),
        PathBuf::from("models/onnxruntime.dll"),
        PathBuf::from(r"C:\Windows\System32\onnxruntime.dll"),
    ];

    candidates.into_iter().find(|path| path.exists())
}

fn prepend_dll_directory_to_path(dll_path: &Path) -> Result<()> {
    let Some(directory) = dll_path.parent() else {
        return Ok(());
    };

    let current_path = std::env::var_os("PATH").unwrap_or_default();
    let mut paths: Vec<PathBuf> = std::env::split_paths(&current_path).collect();
    if !paths.iter().any(|existing| existing == directory) {
        paths.insert(0, directory.to_path_buf());
        let joined = std::env::join_paths(paths)?;
        std::env::set_var("PATH", joined);
    }

    Ok(())
}

impl Detector for OrtDetector {
    fn detect(&mut self, frame: &Frame) -> Result<DetectionBatch, VisionError> {
        let processed = preprocess_frame(frame)?;
        let input = prepare_input_tensor(&processed, self.config.input_width, self.config.input_height)
            .map_err(|error| VisionError::Backend(error.to_string()))?;
        let input_value = TensorRef::from_array_view(input.view())
            .map_err(|error| VisionError::Backend(error.to_string()))?;
        let outputs = self
            .session
            .run(inputs![input_value])
            .map_err(|error| VisionError::Backend(error.to_string()))?;
        let (shape, data) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|error| VisionError::Backend(error.to_string()))?;

        parse_output_to_detections(frame.sequence, &self.config, shape.as_ref(), &data)
    }
}

fn prepare_input_tensor(
    processed: &crate::PreprocessedFrame,
    target_width: u32,
    target_height: u32,
) -> Result<Array4<f32>> {
    let src_width = processed.width as usize;
    let src_height = processed.height as usize;
    let dst_width = target_width as usize;
    let dst_height = target_height as usize;
    let plane_size = dst_width * dst_height;
    let mut tensor = vec![0.0_f32; 3 * plane_size];

    for y in 0..dst_height {
        let src_y = y * src_height / dst_height;
        for x in 0..dst_width {
            let src_x = x * src_width / dst_width;
            let src_idx = (src_y * src_width + src_x) * 3;
            let dst_idx = y * dst_width + x;
            tensor[dst_idx] = processed.data[src_idx + 2];
            tensor[plane_size + dst_idx] = processed.data[src_idx + 1];
            tensor[plane_size * 2 + dst_idx] = processed.data[src_idx];
        }
    }

    Ok(Array4::from_shape_vec((1, 3, dst_height, dst_width), tensor)?)
}

fn parse_output_to_detections(
    frame_sequence: u64,
    config: &OrtDetectorConfig,
    shape: &[i64],
    data: &[f32],
) -> Result<DetectionBatch, VisionError> {
    let (num_boxes, elements_per_box) = match shape {
        [1, num_boxes, elements_per_box] => (*num_boxes as usize, *elements_per_box as usize),
        [num_boxes, elements_per_box] => (*num_boxes as usize, *elements_per_box as usize),
        _ => {
            return Err(VisionError::Backend(format!(
                "unsupported ORT output shape: {shape:?}"
            )))
        }
    };

    if elements_per_box < 6 {
        return Err(VisionError::Backend(format!(
            "unsupported ORT output layout, expected at least 6 values per box and got {elements_per_box}"
        )));
    }

    let mut detections = Vec::new();
    for index in 0..num_boxes {
        let offset = index * elements_per_box;
        if offset + elements_per_box > data.len() {
            break;
        }

        let x1 = data[offset];
        let y1 = data[offset + 1];
        let x2 = data[offset + 2];
        let y2 = data[offset + 3];
        let confidence = data[offset + 4];
        let class_id = data[offset + 5] as i32;

        if confidence < config.confidence_threshold {
            continue;
        }

        let width = (x2 - x1).max(0.0);
        let height = (y2 - y1).max(0.0);
        detections.push(astra_types::Detection {
            bbox: astra_types::BoundingBox {
                x: x1,
                y: y1,
                width,
                height,
            },
            confidence,
            class_id,
            center: astra_types::Point2 {
                x: x1 + width * 0.5,
                y: y1 + height * 0.5,
            },
            observed_at: std::time::Instant::now(),
        });
    }

    Ok(DetectionBatch {
        frame_sequence,
        detections,
    })
}

#[cfg(test)]
mod tests {
    use super::{parse_output_to_detections, OrtDetector, OrtDetectorConfig};
    use crate::Detector;
    use astra_types::{Frame, FramePixelFormat};
    use image::ImageReader;
    use std::{path::{Path, PathBuf}, time::Instant};

    #[test]
    fn ort_detector_rejects_missing_model() {
        let error = OrtDetector::new(OrtDetectorConfig {
            model_path: "missing-model.onnx".to_string(),
            confidence_threshold: 0.5,
            input_width: 640,
            input_height: 640,
        })
        .unwrap_err();

        assert!(error.to_string().contains("model path does not exist"));
    }

    #[test]
    fn ort_detector_initializes_session_when_model_exists() {
        let Some(model_path) = first_existing_path(&[r"D:\projects\astra-autoaim-rs\models\best.onnx"]) else {
            return;
        };

        println!("[ORT TEST] model path: {}", model_path.display());
        let detector = OrtDetector::new(OrtDetectorConfig {
            model_path: model_path.display().to_string(),
            confidence_threshold: 0.25,
            input_width: 640,
            input_height: 640,
        });

        assert!(detector.is_ok(), "ORT session initialization should succeed: {detector:?}");
    }

    #[test]
    fn parser_converts_yolo_like_output() {
        let config = OrtDetectorConfig {
            model_path: "model.onnx".to_string(),
            confidence_threshold: 0.5,
            input_width: 640,
            input_height: 640,
        };
        let shape = [1, 2, 6];
        let data = [10.0, 20.0, 50.0, 60.0, 0.8, 3.0, 0.0, 0.0, 1.0, 1.0, 0.1, 0.0];

        let batch = parse_output_to_detections(7, &config, &shape, &data).unwrap();

        assert_eq!(batch.frame_sequence, 7);
        assert_eq!(batch.detections.len(), 1);
        assert_eq!(batch.detections[0].class_id, 3);
    }

    #[test]
    fn ort_detector_runs_on_single_image_fixture_when_available() {
        let Some(model_path) = first_existing_path(&[
            r"D:\projects\astra-autoaim-rs\models\best.onnx",
        ]) else {
            return;
        };

        let image_path = std::env::var("ASTRA_ORT_TEST_IMAGE")
            .ok()
            .map(PathBuf::from)
            .filter(|path| path.exists())
            .or_else(|| {
                first_existing_path(&[
                    r"D:\projects\astra-autoaim-rs\samples\\ort_input.jpg",
                    r"D:\projects\Astra-autoaim(demo)\slib\Detector\sample\test.jpg",
                ])
            });

        let Some(image_path) = image_path else {
            return;
        };

        println!("[ORT TEST] model path: {}", model_path.display());
        println!("[ORT TEST] image path: {}", image_path.display());
        println!("[ORT TEST] loading image...");
        let frame = load_frame_from_image(&image_path).expect("image fixture should load");
        println!(
            "[ORT TEST] image loaded: {}x{}, bytes={}",
            frame.width,
            frame.height,
            frame.bytes.len()
        );
        println!("[ORT TEST] initializing ORT session...");
        let mut detector = OrtDetector::new(OrtDetectorConfig {
            model_path: model_path.display().to_string(),
            confidence_threshold: 0.25,
            input_width: 640,
            input_height: 640,
        })
        .expect("ort detector should initialize");

        println!("[ORT TEST] running detect()...");
        let result = detector.detect(&frame);
        println!("[ORT TEST] detect() finished: success={}", result.is_ok());
        assert!(result.is_ok(), "single-image ORT inference should execute: {result:?}");
        let batch = result.unwrap();
        println!("[ORT TEST] detections returned: {}", batch.detections.len());
        for (index, detection) in batch.detections.iter().enumerate() {
            println!(
                "[ORT TEST] detection #{index}: class_id={}, confidence={:.4}, bbox=({:.1}, {:.1}, {:.1}, {:.1}), center=({:.1}, {:.1})",
                detection.class_id,
                detection.confidence,
                detection.bbox.x,
                detection.bbox.y,
                detection.bbox.width,
                detection.bbox.height,
                detection.center.x,
                detection.center.y,
            );
        }
        assert_eq!(batch.frame_sequence, frame.sequence);
    }

    fn load_frame_from_image(path: &Path) -> Result<Frame, Box<dyn std::error::Error>> {
        let image = ImageReader::open(path)?.with_guessed_format()?.decode()?.to_rgb8();
        let (width, height) = image.dimensions();
        let rgb = image.into_raw();
        let mut bgr = Vec::with_capacity(rgb.len());
        for chunk in rgb.chunks_exact(3) {
            bgr.extend([chunk[2], chunk[1], chunk[0]]);
        }

        Ok(Frame {
            sequence: 1,
            captured_at: Instant::now(),
            width,
            height,
            pixel_format: FramePixelFormat::Bgr8,
            row_stride_bytes: width as usize * 3,
            source_pixel_format: None,
            bytes: bgr,
        })
    }

    fn first_existing_path(paths: &[&str]) -> Option<PathBuf> {
        paths.iter().map(PathBuf::from).find(|path| path.exists())
    }
}
