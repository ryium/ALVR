#[cfg(target_os = "android")]
use std::ffi::c_void;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum YuvColorSpace {
    Rec601,
    Rec709,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum YuvColorRange {
    Narrow,
    Full,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct YuvColor {
    pub space: YuvColorSpace,
    pub range: YuvColorRange,
}

impl Default for YuvColor {
    fn default() -> Self {
        Self {
            space: YuvColorSpace::Rec709,
            range: YuvColorRange::Narrow,
        }
    }
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug)]
pub struct DmaBufPlane {
    pub fd: std::os::fd::RawFd,
    pub offset: u32,
    pub pitch: u32,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Debug)]
pub struct DmaBufImage {
    pub buffer_id: u64,
    pub width: u32,
    pub height: u32,
    pub drm_fourcc: u32,
    pub modifier: Option<u64>,
    pub planes: Vec<DmaBufPlane>,
    pub color: YuvColor,
    /// Keeps exported descriptors and the dequeued V4L2 buffer alive until GPU use ends.
    pub lease: Option<std::sync::Arc<DmaBufLease>>,
}

#[cfg(target_os = "linux")]
pub struct DmaBufLease(pub Box<dyn Fn() + Send + Sync>);
#[cfg(target_os = "linux")]
impl std::fmt::Debug for DmaBufLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DmaBufLease")
    }
}
#[cfg(target_os = "linux")]
impl Drop for DmaBufLease {
    fn drop(&mut self) {
        (self.0)();
    }
}

#[derive(Debug)]
pub enum VideoFrame {
    #[cfg(target_os = "android")]
    HardwareBuffer(*mut c_void),
    #[cfg(target_os = "linux")]
    DmaBuf(DmaBufImage),
}

// DRM fourcc codes
pub const DRM_FORMAT_NV12: u32 = 0x3231564e; // 'NV12'
pub const DRM_FORMAT_MOD_LINEAR: u64 = 0;

// EGL extension constants for EGL_EXT_image_dma_buf_import
pub const EGL_LINUX_DMA_BUF_EXT: u32 = 0x3270;
pub const EGL_LINUX_DRM_FOURCC_EXT: i32 = 0x3271;
pub const EGL_DMA_BUF_PLANE0_FD_EXT: i32 = 0x3272;
pub const EGL_DMA_BUF_PLANE0_OFFSET_EXT: i32 = 0x3273;
pub const EGL_DMA_BUF_PLANE0_PITCH_EXT: i32 = 0x3274;
pub const EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT: i32 = 0x3443;
pub const EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT: i32 = 0x3444;
pub const EGL_DMA_BUF_PLANE1_FD_EXT: i32 = 0x3275;
pub const EGL_DMA_BUF_PLANE1_OFFSET_EXT: i32 = 0x3276;
pub const EGL_DMA_BUF_PLANE1_PITCH_EXT: i32 = 0x3277;
pub const EGL_DMA_BUF_PLANE1_MODIFIER_LO_EXT: i32 = 0x3445;
pub const EGL_DMA_BUF_PLANE1_MODIFIER_HI_EXT: i32 = 0x3446;
pub const EGL_YUV_COLOR_SPACE_HINT_EXT: i32 = 0x327b;
pub const EGL_SAMPLE_RANGE_HINT_EXT: i32 = 0x327c;
pub const EGL_YUV_FULL_RANGE_EXT: i32 = 0x3282;
pub const EGL_YUV_NARROW_RANGE_EXT: i32 = 0x3283;
pub const EGL_ITU_REC601_EXT: i32 = 0x327f;
pub const EGL_ITU_REC709_EXT: i32 = 0x3280;
pub const EGL_WIDTH: i32 = 0x3057;
pub const EGL_HEIGHT: i32 = 0x3056;
pub const EGL_NONE: i32 = 0x3038;

#[cfg(target_os = "linux")]
pub fn build_egl_dma_buf_attribs(image: &DmaBufImage) -> Vec<i32> {
    let mut attribs = vec![
        EGL_WIDTH,
        image.width as i32,
        EGL_HEIGHT,
        image.height as i32,
        EGL_LINUX_DRM_FOURCC_EXT,
        image.drm_fourcc as i32,
    ];

    let plane_keys = [
        (
            EGL_DMA_BUF_PLANE0_FD_EXT,
            EGL_DMA_BUF_PLANE0_OFFSET_EXT,
            EGL_DMA_BUF_PLANE0_PITCH_EXT,
            EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT,
            EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT,
        ),
        (
            EGL_DMA_BUF_PLANE1_FD_EXT,
            EGL_DMA_BUF_PLANE1_OFFSET_EXT,
            EGL_DMA_BUF_PLANE1_PITCH_EXT,
            EGL_DMA_BUF_PLANE1_MODIFIER_LO_EXT,
            EGL_DMA_BUF_PLANE1_MODIFIER_HI_EXT,
        ),
    ];

    for (i, plane) in image.planes.iter().enumerate().take(2) {
        let (fd_key, offset_key, pitch_key, mod_lo_key, mod_hi_key) = plane_keys[i];
        attribs.extend_from_slice(&[
            fd_key,
            plane.fd,
            offset_key,
            plane.offset as i32,
            pitch_key,
            plane.pitch as i32,
        ]);
        if let Some(m) = image.modifier {
            attribs.extend_from_slice(&[
                mod_lo_key,
                (m & 0xFFFF_FFFF) as i32,
                mod_hi_key,
                ((m >> 32) & 0xFFFF_FFFF) as i32,
            ]);
        }
    }

    match image.color.space {
        YuvColorSpace::Rec601 => {
            attribs.extend_from_slice(&[EGL_YUV_COLOR_SPACE_HINT_EXT, EGL_ITU_REC601_EXT]);
        }
        YuvColorSpace::Rec709 => {
            attribs.extend_from_slice(&[EGL_YUV_COLOR_SPACE_HINT_EXT, EGL_ITU_REC709_EXT]);
        }
    }

    match image.color.range {
        YuvColorRange::Narrow => {
            attribs.extend_from_slice(&[EGL_SAMPLE_RANGE_HINT_EXT, EGL_YUV_NARROW_RANGE_EXT]);
        }
        YuvColorRange::Full => {
            attribs.extend_from_slice(&[EGL_SAMPLE_RANGE_HINT_EXT, EGL_YUV_FULL_RANGE_EXT]);
        }
    }

    attribs.push(EGL_NONE);
    attribs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn test_dma_buf_attribs() {
        let img = DmaBufImage {
            buffer_id: 1,
            width: 4352,
            height: 2176,
            drm_fourcc: DRM_FORMAT_NV12,
            modifier: None,
            planes: vec![
                DmaBufPlane {
                    fd: 10,
                    offset: 0,
                    pitch: 4352,
                },
                DmaBufPlane {
                    fd: 10,
                    offset: 4352 * 2176,
                    pitch: 4352,
                },
            ],
            color: YuvColor::default(),
            lease: None,
        };

        let attrs = build_egl_dma_buf_attribs(&img);
        assert_eq!(*attrs.last().unwrap(), EGL_NONE);
        assert_eq!(
            attrs,
            vec![
                0x3057,
                4352,
                0x3056,
                2176,
                0x3271,
                DRM_FORMAT_NV12 as i32,
                0x3272,
                10,
                0x3273,
                0,
                0x3274,
                4352,
                0x3275,
                10,
                0x3276,
                4352 * 2176,
                0x3277,
                4352,
                0x327b,
                0x3280,
                0x327c,
                0x3283,
                0x3038,
            ]
        );
        let attrs = build_egl_dma_buf_attribs(&DmaBufImage {
            modifier: Some(0x1234_5678_9abc_def0),
            ..img
        });
        let pairs: std::collections::HashMap<i32, i32> = attrs[..attrs.len() - 1]
            .chunks_exact(2)
            .map(|p| (p[0], p[1]))
            .collect();
        assert_eq!(pairs[&0x3443], 0x9abc_def0u32 as i32);
        assert_eq!(pairs[&0x3444], 0x1234_5678);
        assert_eq!(pairs[&0x3445], 0x9abc_def0u32 as i32);
        assert_eq!(pairs[&0x3275], 10);
    }
}
