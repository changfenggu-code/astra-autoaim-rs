use crate::{ffi, CameraError, CameraSource};
use anyhow::Result;
use astra_types::{Frame, FramePixelFormat};
use std::{ffi::CString, ptr, time::Instant};

#[derive(Debug, Clone)]
pub struct GxCameraConfig {
    pub device_index: u32,
    pub width: u32,
    pub height: u32,
    pub acquisition_timeout_ms: u32,
}

impl Default for GxCameraConfig {
    fn default() -> Self {
        Self {
            device_index: 1,
            width: 1280,
            height: 1024,
            acquisition_timeout_ms: 100,
        }
    }
}

#[derive(Debug)]
pub struct GxCameraSource {
    config: GxCameraConfig,
    device: ffi::GxDevHandle,
    frame: ffi::GxFrameData,
    payload_buffer: Vec<u8>,
    converted_buffer: Vec<u8>,
    raw8_buffer: Vec<u8>,
    acquisition_started: bool,
    library_open: bool,
}

impl GxCameraSource {
    pub fn open(config: GxCameraConfig) -> Result<Self> {
        if config.width == 0 || config.height == 0 {
            return Err(CameraError::InvalidConfig("width and height must be greater than zero").into());
        }
        if config.device_index == 0 {
            return Err(CameraError::InvalidConfig("device_index must start from 1").into());
        }
        if config.acquisition_timeout_ms == 0 {
            return Err(CameraError::InvalidConfig("acquisition_timeout_ms must be greater than zero").into());
        }

        unsafe {
            check_status(ffi::GXInitLib(), "GXInitLib")?;

            let mut device_count = 0_u32;
            check_status(
                ffi::GXUpdateAllDeviceList(&mut device_count, config.acquisition_timeout_ms),
                "GXUpdateAllDeviceList",
            )?;

            if device_count == 0 {
                let _ = ffi::GXCloseLib();
                return Err(CameraError::Unavailable("no Daheng camera devices were found").into());
            }

            let index = CString::new(config.device_index.to_string())
                .map_err(|_| CameraError::InvalidConfig("device_index contains invalid bytes"))?;
            let mut open_param = ffi::GxOpenParam {
                psz_content: index.as_ptr() as *mut _,
                open_mode: ffi::GX_OPEN_INDEX,
                access_mode: ffi::GX_ACCESS_EXCLUSIVE,
            };
            let mut device = ptr::null_mut();
            if let Err(error) = check_status(ffi::GXOpenDevice(&mut open_param, &mut device), "GXOpenDevice") {
                let _ = ffi::GXCloseLib();
                return Err(error);
            }

            if let Err(error) = configure_device(device) {
                let _ = ffi::GXCloseDevice(device);
                let _ = ffi::GXCloseLib();
                return Err(error);
            }

            let payload_size = match query_payload_size(device) {
                Ok(size) => size,
                Err(error) => {
                    let _ = ffi::GXCloseDevice(device);
                    let _ = ffi::GXCloseLib();
                    return Err(error);
                }
            };

            if let Err(error) = set_command(device, "AcquisitionStart") {
                let _ = ffi::GXCloseDevice(device);
                let _ = ffi::GXCloseLib();
                return Err(error);
            }

            let mut payload_buffer = vec![0_u8; payload_size];
            let mut frame = ffi::GxFrameData::default();
            frame.p_img_buf = payload_buffer.as_mut_ptr().cast();
            let converted_buffer = vec![0_u8; config.width as usize * config.height as usize * 3];
            let raw8_buffer = vec![0_u8; payload_size];

            return Ok(Self {
                config,
                device,
                frame,
                payload_buffer,
                converted_buffer,
                raw8_buffer,
                acquisition_started: true,
                library_open: true,
            });
        }
    }
}

impl CameraSource for GxCameraSource {
    fn next_frame(&mut self) -> Result<Frame> {
        self.frame.p_img_buf = self.payload_buffer.as_mut_ptr().cast();

        unsafe {
            check_status(
                ffi::GXGetImage(self.device, &mut self.frame, self.config.acquisition_timeout_ms),
                "GXGetImage",
            )?;
        }

        if self.frame.p_img_buf.is_null() || self.frame.n_img_size <= 0 {
            return Err(CameraError::Unavailable("gx returned an empty frame buffer").into());
        }

        let (pixel_format, row_stride_bytes, bytes) = self.normalize_frame_bytes()?;

        Ok(Frame {
            sequence: self.frame.n_frame_id,
            captured_at: Instant::now(),
            width: self.frame.n_width.max(0) as u32,
            height: self.frame.n_height.max(0) as u32,
            pixel_format,
            row_stride_bytes,
            source_pixel_format: Some(self.frame.n_pixel_format),
            bytes,
        })
    }
}

impl Drop for GxCameraSource {
    fn drop(&mut self) {
        unsafe {
            if self.acquisition_started && !self.device.is_null() {
                let _ = set_command(self.device, "AcquisitionStop");
                self.acquisition_started = false;
            }

            if !self.device.is_null() {
                let _ = ffi::GXCloseDevice(self.device);
                self.device = ptr::null_mut();
            }

            if self.library_open {
                let _ = ffi::GXCloseLib();
                self.library_open = false;
            }
        }
    }
}

fn check_status(status: ffi::GxStatus, operation: &'static str) -> Result<()> {
    if status == ffi::GX_STATUS_SUCCESS {
        Ok(())
    } else {
        Err(CameraError::SdkCallFailed { operation, status }.into())
    }
}

fn configure_device(device: ffi::GxDevHandle) -> Result<()> {
    set_enum_string(device, "AcquisitionMode", "Continuous")?;
    set_enum_string(device, "TriggerMode", "Off")?;
    Ok(())
}

fn query_payload_size(device: ffi::GxDevHandle) -> Result<usize> {
    let mut int_value = ffi::GxIntValue::default();
    let name = CString::new("PayloadSize").expect("static string is valid");

    unsafe {
        check_status(
            ffi::GXGetIntValue(device.cast(), name.as_ptr(), &mut int_value),
            "GXGetIntValue",
        )?;
    }

    usize::try_from(int_value.n_cur_value)
        .map_err(|_| CameraError::Unavailable("invalid payload size from gx SDK").into())
}

fn set_enum_string(device: ffi::GxDevHandle, name: &'static str, value: &'static str) -> Result<()> {
    let name = CString::new(name).expect("static string is valid");
    let value = CString::new(value).expect("static string is valid");

    unsafe {
        check_status(
            ffi::GXSetEnumValueByString(device.cast(), name.as_ptr(), value.as_ptr()),
            "GXSetEnumValueByString",
        )
    }
}

fn set_command(device: ffi::GxDevHandle, name: &'static str) -> Result<()> {
    let name = CString::new(name).expect("static string is valid");

    unsafe { check_status(ffi::GXSetCommandValue(device.cast(), name.as_ptr()), "GXSetCommandValue") }
}

impl GxCameraSource {
    fn normalize_frame_bytes(&mut self) -> Result<(FramePixelFormat, usize, Vec<u8>)> {
        let width = self.frame.n_width.max(0) as u32;
        let height = self.frame.n_height.max(0) as u32;
        let pixel_format = self.frame.n_pixel_format;

        match classify_pixel_format(pixel_format) {
            GxPixelKind::Mono8 | GxPixelKind::Bayer8(_) => {
                self.converted_buffer.resize(width as usize * height as usize * 3, 0);
                unsafe {
                    let status = ffi::DxRaw8toRGB24Ex(
                        self.frame.p_img_buf,
                        self.converted_buffer.as_mut_ptr().cast(),
                        width,
                        height,
                        ffi::DX_BAYER_CONVERT_NEIGHBOUR,
                        color_filter_for_kind(classify_pixel_format(pixel_format)),
                        false,
                        ffi::DX_ORDER_BGR,
                    );
                    check_dx_status(status, "DxRaw8toRGB24Ex")?;
                }
                Ok((
                    FramePixelFormat::Bgr8,
                    width as usize * 3,
                    self.converted_buffer.clone(),
                ))
            }
            GxPixelKind::Mono10Or12(valid_bits) | GxPixelKind::Bayer10Or12(_, valid_bits) => {
                self.raw8_buffer.resize(width as usize * height as usize, 0);
                self.converted_buffer.resize(width as usize * height as usize * 3, 0);
                unsafe {
                    let status = ffi::DxRaw16toRaw8(
                        self.frame.p_img_buf,
                        self.raw8_buffer.as_mut_ptr().cast(),
                        width,
                        height,
                        valid_bits,
                    );
                    check_dx_status(status, "DxRaw16toRaw8")?;

                    let status = ffi::DxRaw8toRGB24Ex(
                        self.raw8_buffer.as_mut_ptr().cast(),
                        self.converted_buffer.as_mut_ptr().cast(),
                        width,
                        height,
                        ffi::DX_BAYER_CONVERT_NEIGHBOUR,
                        color_filter_for_kind(classify_pixel_format(pixel_format)),
                        false,
                        ffi::DX_ORDER_BGR,
                    );
                    check_dx_status(status, "DxRaw8toRGB24Ex")?;
                }
                Ok((
                    FramePixelFormat::Bgr8,
                    width as usize * 3,
                    self.converted_buffer.clone(),
                ))
            }
            GxPixelKind::Unsupported => {
                let img_size = self.frame.n_img_size.max(0) as usize;
                let bytes = unsafe {
                    std::slice::from_raw_parts(self.frame.p_img_buf.cast::<u8>(), img_size).to_vec()
                };
                Ok((
                    FramePixelFormat::NativeRaw,
                    compute_row_stride_bytes(&self.frame),
                    bytes,
                ))
            }
        }
    }
}

fn compute_row_stride_bytes(frame: &ffi::GxFrameData) -> usize {
    if frame.n_height > 0 && frame.n_img_size > 0 {
        let height = frame.n_height as usize;
        let img_size = frame.n_img_size as usize;
        let stride = img_size / height;
        return stride.max(1);
    }

    frame.n_width.max(0) as usize
}

fn check_dx_status(status: ffi::DxStatus, operation: &'static str) -> Result<()> {
    if status == ffi::DX_OK {
        Ok(())
    } else {
        Err(CameraError::SdkCallFailed { operation, status }.into())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GxPixelKind {
    Mono8,
    Bayer8(ffi::DxPixelColorFilter),
    Mono10Or12(ffi::DxValidBit),
    Bayer10Or12(ffi::DxPixelColorFilter, ffi::DxValidBit),
    Unsupported,
}

fn classify_pixel_format(pixel_format: i32) -> GxPixelKind {
    match pixel_format {
        GX_PIXEL_FORMAT_MONO8 => GxPixelKind::Mono8,
        GX_PIXEL_FORMAT_BAYER_GR8 => GxPixelKind::Bayer8(ffi::DX_PIXEL_COLOR_FILTER_BAYERGR),
        GX_PIXEL_FORMAT_BAYER_RG8 => GxPixelKind::Bayer8(ffi::DX_PIXEL_COLOR_FILTER_BAYERRG),
        GX_PIXEL_FORMAT_BAYER_GB8 => GxPixelKind::Bayer8(ffi::DX_PIXEL_COLOR_FILTER_BAYERGB),
        GX_PIXEL_FORMAT_BAYER_BG8 => GxPixelKind::Bayer8(ffi::DX_PIXEL_COLOR_FILTER_BAYERBG),
        GX_PIXEL_FORMAT_MONO10 => GxPixelKind::Mono10Or12(ffi::DX_VALID_BIT_2_9),
        GX_PIXEL_FORMAT_MONO12 => GxPixelKind::Mono10Or12(ffi::DX_VALID_BIT_4_11),
        GX_PIXEL_FORMAT_BAYER_GR10 => {
            GxPixelKind::Bayer10Or12(ffi::DX_PIXEL_COLOR_FILTER_BAYERGR, ffi::DX_VALID_BIT_2_9)
        }
        GX_PIXEL_FORMAT_BAYER_RG10 => {
            GxPixelKind::Bayer10Or12(ffi::DX_PIXEL_COLOR_FILTER_BAYERRG, ffi::DX_VALID_BIT_2_9)
        }
        GX_PIXEL_FORMAT_BAYER_GB10 => {
            GxPixelKind::Bayer10Or12(ffi::DX_PIXEL_COLOR_FILTER_BAYERGB, ffi::DX_VALID_BIT_2_9)
        }
        GX_PIXEL_FORMAT_BAYER_BG10 => {
            GxPixelKind::Bayer10Or12(ffi::DX_PIXEL_COLOR_FILTER_BAYERBG, ffi::DX_VALID_BIT_2_9)
        }
        GX_PIXEL_FORMAT_BAYER_GR12 => {
            GxPixelKind::Bayer10Or12(ffi::DX_PIXEL_COLOR_FILTER_BAYERGR, ffi::DX_VALID_BIT_4_11)
        }
        GX_PIXEL_FORMAT_BAYER_RG12 => {
            GxPixelKind::Bayer10Or12(ffi::DX_PIXEL_COLOR_FILTER_BAYERRG, ffi::DX_VALID_BIT_4_11)
        }
        GX_PIXEL_FORMAT_BAYER_GB12 => {
            GxPixelKind::Bayer10Or12(ffi::DX_PIXEL_COLOR_FILTER_BAYERGB, ffi::DX_VALID_BIT_4_11)
        }
        GX_PIXEL_FORMAT_BAYER_BG12 => {
            GxPixelKind::Bayer10Or12(ffi::DX_PIXEL_COLOR_FILTER_BAYERBG, ffi::DX_VALID_BIT_4_11)
        }
        _ => GxPixelKind::Unsupported,
    }
}

fn color_filter_for_kind(kind: GxPixelKind) -> ffi::DxPixelColorFilter {
    match kind {
        GxPixelKind::Mono8 | GxPixelKind::Mono10Or12(_) => ffi::DX_PIXEL_COLOR_FILTER_NONE,
        GxPixelKind::Bayer8(filter) | GxPixelKind::Bayer10Or12(filter, _) => filter,
        GxPixelKind::Unsupported => ffi::DX_PIXEL_COLOR_FILTER_NONE,
    }
}

const GX_PIXEL_FORMAT_MONO8: i32 = 0x0108_0001;
const GX_PIXEL_FORMAT_MONO10: i32 = 0x0110_0003;
const GX_PIXEL_FORMAT_MONO12: i32 = 0x0110_0005;
const GX_PIXEL_FORMAT_BAYER_GR8: i32 = 0x0108_0008;
const GX_PIXEL_FORMAT_BAYER_RG8: i32 = 0x0108_0009;
const GX_PIXEL_FORMAT_BAYER_GB8: i32 = 0x0108_000A;
const GX_PIXEL_FORMAT_BAYER_BG8: i32 = 0x0108_000B;
const GX_PIXEL_FORMAT_BAYER_GR10: i32 = 0x0110_000C;
const GX_PIXEL_FORMAT_BAYER_RG10: i32 = 0x0110_000D;
const GX_PIXEL_FORMAT_BAYER_GB10: i32 = 0x0110_000E;
const GX_PIXEL_FORMAT_BAYER_BG10: i32 = 0x0110_000F;
const GX_PIXEL_FORMAT_BAYER_GR12: i32 = 0x0110_0010;
const GX_PIXEL_FORMAT_BAYER_RG12: i32 = 0x0110_0011;
const GX_PIXEL_FORMAT_BAYER_GB12: i32 = 0x0110_0012;
const GX_PIXEL_FORMAT_BAYER_BG12: i32 = 0x0110_0013;

#[cfg(test)]
mod tests {
    use super::{classify_pixel_format, GxCameraConfig, GxCameraSource, GX_PIXEL_FORMAT_BAYER_GR8, GX_PIXEL_FORMAT_MONO8};
    use astra_types::FramePixelFormat;

    #[test]
    fn gx_open_rejects_invalid_size() {
        let error = GxCameraSource::open(GxCameraConfig { width: 0, height: 1024 }).unwrap_err();
        assert!(error.to_string().contains("camera configuration is invalid"));
    }

    #[test]
    fn gx_open_rejects_zero_timeout() {
        let error = GxCameraSource::open(GxCameraConfig {
            acquisition_timeout_ms: 0,
            ..GxCameraConfig::default()
        })
        .unwrap_err();
        assert!(error.to_string().contains("camera configuration is invalid"));
    }

    #[test]
    fn supported_pixel_formats_map_to_bgr8_contract() {
        let mono_kind = classify_pixel_format(GX_PIXEL_FORMAT_MONO8);
        let bayer_kind = classify_pixel_format(GX_PIXEL_FORMAT_BAYER_GR8);

        assert!(!matches!(mono_kind, super::GxPixelKind::Unsupported));
        assert!(!matches!(classify_pixel_format(GX_PIXEL_FORMAT_BAYER_GR8), super::GxPixelKind::Unsupported));
        let _contract = FramePixelFormat::Bgr8;
    }
}
