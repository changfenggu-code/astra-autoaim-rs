use std::ffi::{c_char, c_void};

pub type GxStatus = i32;
pub type GxFrameStatus = i32;
pub type GxDevHandle = *mut c_void;
pub type GxOpenModeCmd = i32;
pub type GxAccessModeCmd = i32;
pub type GxPortHandle = *mut c_void;
pub type DxStatus = i32;
pub type DxPixelColorFilter = i32;
pub type DxBayerConvertType = i32;
pub type DxValidBit = i32;
pub type DxRgbChannelOrder = i32;

pub const GX_STATUS_SUCCESS: GxStatus = 0;
pub const GX_OPEN_INDEX: GxOpenModeCmd = 3;
pub const GX_ACCESS_EXCLUSIVE: GxAccessModeCmd = 4;
pub const DX_OK: DxStatus = 0;
pub const DX_PIXEL_COLOR_FILTER_NONE: DxPixelColorFilter = 0;
pub const DX_PIXEL_COLOR_FILTER_BAYERRG: DxPixelColorFilter = 1;
pub const DX_PIXEL_COLOR_FILTER_BAYERGB: DxPixelColorFilter = 2;
pub const DX_PIXEL_COLOR_FILTER_BAYERGR: DxPixelColorFilter = 3;
pub const DX_PIXEL_COLOR_FILTER_BAYERBG: DxPixelColorFilter = 4;
pub const DX_BAYER_CONVERT_NEIGHBOUR: DxBayerConvertType = 0;
pub const DX_VALID_BIT_2_9: DxValidBit = 2;
pub const DX_VALID_BIT_4_11: DxValidBit = 4;
pub const DX_ORDER_BGR: DxRgbChannelOrder = 1;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GxOpenParam {
    pub psz_content: *mut c_char,
    pub open_mode: GxOpenModeCmd,
    pub access_mode: GxAccessModeCmd,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GxFrameData {
    pub n_status: GxFrameStatus,
    pub p_img_buf: *mut c_void,
    pub n_width: i32,
    pub n_height: i32,
    pub n_pixel_format: i32,
    pub n_img_size: i32,
    pub n_frame_id: u64,
    pub n_timestamp: u64,
    pub n_offset_x: i32,
    pub n_offset_y: i32,
    pub reserved: [i32; 1],
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GxIntValue {
    pub n_cur_value: i64,
    pub n_min: i64,
    pub n_max: i64,
    pub n_inc: i64,
    pub reserved: [i32; 16],
}

impl Default for GxIntValue {
    fn default() -> Self {
        Self {
            n_cur_value: 0,
            n_min: 0,
            n_max: 0,
            n_inc: 0,
            reserved: [0; 16],
        }
    }
}

impl Default for GxFrameData {
    fn default() -> Self {
        Self {
            n_status: 0,
            p_img_buf: std::ptr::null_mut(),
            n_width: 0,
            n_height: 0,
            n_pixel_format: 0,
            n_img_size: 0,
            n_frame_id: 0,
            n_timestamp: 0,
            n_offset_x: 0,
            n_offset_y: 0,
            reserved: [0],
        }
    }
}

#[link(name = "gxiapi")]
extern "C" {
    pub fn GXInitLib() -> GxStatus;
    pub fn GXCloseLib() -> GxStatus;
    pub fn GXUpdateAllDeviceList(device_count: *mut u32, timeout_ms: u32) -> GxStatus;
    pub fn GXOpenDevice(open_param: *mut GxOpenParam, device: *mut GxDevHandle) -> GxStatus;
    pub fn GXCloseDevice(device: GxDevHandle) -> GxStatus;
    pub fn GXGetIntValue(port: GxPortHandle, name: *const c_char, int_value: *mut GxIntValue) -> GxStatus;
    pub fn GXSetEnumValueByString(port: GxPortHandle, name: *const c_char, value: *const c_char) -> GxStatus;
    pub fn GXSetCommandValue(port: GxPortHandle, name: *const c_char) -> GxStatus;
    #[allow(dead_code)]
    pub fn GXGetImage(device: GxDevHandle, frame_data: *mut GxFrameData, timeout_ms: u32) -> GxStatus;

    pub fn DxRaw8toRGB24Ex(
        input: *mut c_void,
        output: *mut c_void,
        width: u32,
        height: u32,
        convert_type: DxBayerConvertType,
        bayer_type: DxPixelColorFilter,
        flip: bool,
        channel_order: DxRgbChannelOrder,
    ) -> DxStatus;

    pub fn DxRaw16toRaw8(
        input: *mut c_void,
        output: *mut c_void,
        width: u32,
        height: u32,
        valid_bits: DxValidBit,
    ) -> DxStatus;
}
