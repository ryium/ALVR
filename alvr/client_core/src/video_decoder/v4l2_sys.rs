//! V4L2 FFI structures and constants for Linux 64-bit (aarch64 / x86_64).
#![allow(non_camel_case_types, non_snake_case, dead_code)]

pub const V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE: u32 = 9;
pub const V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE: u32 = 10;

pub const V4L2_MEMORY_MMAP: u32 = 1;
pub const V4L2_MEMORY_DMABUF: u32 = 4;

pub const V4L2_BUF_FLAG_KEYFRAME: u32 = 0x00000008;
pub const V4L2_BUF_FLAG_TIMESTAMP_COPY: u32 = 0x00004000;

pub const V4L2_EVENT_EOS: u32 = 2;
pub const V4L2_EVENT_SOURCE_CHANGE: u32 = 5;
pub const V4L2_EVENT_SRC_CH_RESOLUTION: u32 = 1;

pub const V4L2_CID_MPEG_VIDEO_DEC_DISPLAY_DELAY: u32 = 0x00990b8d;
pub const V4L2_CID_MPEG_VIDEO_DEC_DISPLAY_DELAY_ENABLE: u32 = 0x00990b8e;

// FourCCs
pub const V4L2_PIX_FMT_HEVC: u32 = 0x43564548; // 'HEVC'
pub const V4L2_PIX_FMT_H264: u32 = 0x34363248; // 'H264'
pub const V4L2_PIX_FMT_NV12: u32 = 0x3231564e; // 'NV12'
pub const V4L2_PIX_FMT_AV1: u32 = 0x31305641; // 'AV01'

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct v4l2_timeval {
    pub tv_sec: i64,
    pub tv_usec: i64,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct v4l2_timespec {
    pub tv_sec: i64,
    pub tv_nsec: i64,
}

// IOCTL command numbers
pub const VIDIOC_G_FMT: u64 = 0xc0d05604;
pub const VIDIOC_S_FMT: u64 = 0xc0d05605;
pub const VIDIOC_REQBUFS: u64 = 0xc0145608;
pub const VIDIOC_QUERYBUF: u64 = 0xc0585609;
pub const VIDIOC_QBUF: u64 = 0xc058560f;
pub const VIDIOC_DQBUF: u64 = 0xc0585611;
pub const VIDIOC_STREAMON: u64 = 0x40045612;
pub const VIDIOC_STREAMOFF: u64 = 0x40045613;
pub const VIDIOC_EXPBUF: u64 = 0xc0405610;
pub const VIDIOC_SUBSCRIBE_EVENT: u64 = 0x4020565a;
pub const VIDIOC_DQEVENT: u64 = 0x80885659;
pub const VIDIOC_S_CTRL: u64 = 0xc008561c;
pub const VIDIOC_G_SELECTION: u64 = 0xc040565e;

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct v4l2_rect {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
}
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct v4l2_selection {
    pub type_: u32,
    pub target: u32,
    pub flags: u32,
    pub r: v4l2_rect,
    pub reserved: [u32; 9],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct v4l2_plane_pix_format {
    pub sizeimage: u32,
    pub bytesperline: u32,
    pub reserved: [u16; 6],
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct v4l2_pix_format_mplane {
    pub width: u32,
    pub height: u32,
    pub pixelformat: u32,
    pub field: u32,
    pub colorspace: u32,
    pub plane_fmt: [v4l2_plane_pix_format; 8],
    pub num_planes: u8,
    pub flags: u8,
    pub ycbcr_enc: u8,
    pub quantization: u8,
    pub xfer_func: u8,
    pub reserved: [u8; 7],
}

impl Default for v4l2_pix_format_mplane {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct v4l2_format {
    pub type_: u32,
    pub _padding: [u8; 4],
    pub fmt: v4l2_format_union,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub union v4l2_format_union {
    pub pix_mp: v4l2_pix_format_mplane,
    pub raw_data: [u8; 200],
}

impl Default for v4l2_format {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct v4l2_requestbuffers {
    pub count: u32,
    pub type_: u32,
    pub memory: u32,
    pub capabilities: u32,
    pub flags: u8,
    pub reserved: [u8; 3],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct v4l2_plane {
    pub bytesused: u32,
    pub length: u32,
    pub m: v4l2_plane_mem,
    pub data_offset: u32,
    pub reserved: [u32; 11],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub union v4l2_plane_mem {
    pub mem_offset: u32,
    pub userptr: u64,
    pub fd: i32,
}

impl Default for v4l2_plane {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct v4l2_buffer {
    pub index: u32,
    pub type_: u32,
    pub bytesused: u32,
    pub flags: u32,
    pub field: u32,
    pub timestamp: v4l2_timeval,
    pub timecode: [u8; 16],
    pub sequence: u32,
    pub memory: u32,
    pub m: v4l2_buffer_m,
    pub length: u32,
    pub reserved2: u32,
    pub request_fd: i32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub union v4l2_buffer_m {
    pub offset: u32,
    pub userptr: u64,
    pub planes: *mut v4l2_plane,
    pub fd: i32,
}

impl Default for v4l2_buffer {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct v4l2_exportbuffer {
    pub type_: u32,
    pub index: u32,
    pub plane: u32,
    pub flags: u32,
    pub fd: i32,
    pub reserved: [u32; 11],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct v4l2_event_subscription {
    pub type_: u32,
    pub id: u32,
    pub flags: u32,
    pub reserved: [u32; 5],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct v4l2_event {
    pub type_: u32,
    pub _union_alignment: u32,
    pub u: [u8; 64],
    pub pending: u32,
    pub sequence: u32,
    pub timestamp: v4l2_timespec,
    pub id: u32,
    pub reserved: [u32; 8],
}

impl Default for v4l2_event {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct v4l2_control {
    pub id: u32,
    pub value: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_v4l2_struct_sizes() {
        assert_eq!(std::mem::size_of::<v4l2_format>(), 208);
        assert_eq!(std::mem::size_of::<v4l2_buffer>(), 88);
        assert_eq!(std::mem::size_of::<v4l2_plane>(), 64);
        assert_eq!(std::mem::size_of::<v4l2_requestbuffers>(), 20);
        assert_eq!(std::mem::size_of::<v4l2_exportbuffer>(), 64);
        assert_eq!(std::mem::size_of::<v4l2_event_subscription>(), 32);
        assert_eq!(std::mem::size_of::<v4l2_event>(), 136);
        assert_eq!(std::mem::offset_of!(v4l2_event, u), 8);
        assert_eq!(std::mem::offset_of!(v4l2_event, timestamp), 80);
        assert_eq!(std::mem::size_of::<v4l2_selection>(), 64);
    }
}
