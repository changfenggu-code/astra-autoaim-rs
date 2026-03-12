use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub app: AppSection,
    pub camera: CameraConfig,
    pub serial: SerialConfig,
    pub detector: DetectorConfig,
    pub runtime: RuntimeConfig,
}

impl AppConfig {
    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self> {
        let raw = fs::read_to_string(path)?;
        Ok(serde_yaml::from_str(&raw)?)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSection {
    pub mode: String,
    pub log_level: String,
}

impl Default for AppSection {
    fn default() -> Self {
        Self {
            mode: "mock".to_string(),
            log_level: "info".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CameraConfig {
    pub source: String,
    pub device_index: u32,
    pub width: u32,
    pub height: u32,
    pub acquisition_timeout_ms: u32,
}

impl Default for CameraConfig {
    fn default() -> Self {
        Self {
            source: "mock".to_string(),
            device_index: 1,
            width: 1280,
            height: 1024,
            acquisition_timeout_ms: 100,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SerialConfig {
    pub backend: Option<String>,
    pub debug: u8,
    pub read_device: String,
    pub write_device: String,
    pub baud_read: u32,
    pub baud_write: u32,
    pub read_timeout_ms: u64,
    pub write_timeout_ms: u64,
    pub read_chunk_size: usize,
    pub protocol: SerialProtocolConfig,
}

impl Default for SerialConfig {
    fn default() -> Self {
        Self {
            backend: None,
            debug: 0,
            read_device: "/dev/ttyUSB0".to_string(),
            write_device: "/dev/ttyUSB0".to_string(),
            baud_read: 115200,
            baud_write: 115200,
            read_timeout_ms: 10,
            write_timeout_ms: 10,
            read_chunk_size: 64,
            protocol: SerialProtocolConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SerialProtocolConfig {
    pub tx_header: u8,
    pub rx_header: u8,
    pub frame_footer: u8,
    pub tx_payload_len: usize,
    pub rx_payload_len: usize,
}

impl Default for SerialProtocolConfig {
    fn default() -> Self {
        Self {
            tx_header: 0x43,
            rx_header: 0x45,
            frame_footer: 0xFF,
            tx_payload_len: 10,
            rx_payload_len: 6,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DetectorConfig {
    pub backend: Option<String>,
    pub model_path: String,
    pub confidence_threshold: f32,
    pub input_width: u32,
    pub input_height: u32,
}

impl Default for DetectorConfig {
    fn default() -> Self {
        Self {
            backend: None,
            model_path: "models/best.onnx".to_string(),
            confidence_threshold: 0.5,
            input_width: 640,
            input_height: 640,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RuntimeConfig {
    pub frame_buffer_depth: usize,
    pub detection_channel_depth: usize,
    pub command_slot_depth: usize,
    pub mock_cycles: usize,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            frame_buffer_depth: 3,
            detection_channel_depth: 2,
            command_slot_depth: 1,
            mock_cycles: 8,
        }
    }
}
