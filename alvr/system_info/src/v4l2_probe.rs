use alvr_common::frameconnect::{
    CAPABILITY_VERSION, DecodeMode, FrameConnectCapabilities, HardwareDecoder,
};
use std::collections::HashMap;

#[derive(Clone, Debug, Default)]
pub struct V4l2DeviceInfo {
    pub card: String,
    pub path: String,
    /// Exact dimensions accepted by TRY_FMT for each coded format with NV12 capture.
    pub dimensions: HashMap<u32, Vec<[u32; 2]>>,
}
pub const V4L2_PIX_FMT_HEVC: u32 = u32::from_le_bytes(*b"HEVC");
pub const V4L2_PIX_FMT_H264: u32 = u32::from_le_bytes(*b"H264");
pub const V4L2_PIX_FMT_AV1: u32 = u32::from_le_bytes(*b"AV01");
const V4L2_PIX_FMT_NV12: u32 = u32::from_le_bytes(*b"NV12");

/// Rates are requested display rates, not a V4L2 throughput declaration.
/// Real delivered cadence must be monitored by the sender.
pub fn build_capabilities_from_v4l2_info(
    devices: &[V4l2DeviceInfo],
    display_rates: &[f32],
) -> FrameConnectCapabilities {
    let mut decoders = Vec::new();
    for dev in devices {
        for (codec, mime) in [
            (V4L2_PIX_FMT_HEVC, "video/hevc"),
            (V4L2_PIX_FMT_H264, "video/avc"),
            (V4L2_PIX_FMT_AV1, "video/av01"),
        ] {
            let Some(sizes) = dev.dimensions.get(&codec) else {
                continue;
            };
            let modes = sizes
                .iter()
                .flat_map(|&[width, height]| {
                    display_rates
                        .iter()
                        .copied()
                        .filter(|rate| rate.is_finite() && *rate > 0.0)
                        .map(move |fps| DecodeMode { width, height, fps })
                })
                .collect::<Vec<_>>();
            if !modes.is_empty() {
                decoders.push(HardwareDecoder {
                    name: format!("v4l2:{}@{}", dev.card, dev.path),
                    mime: mime.into(),
                    // The implemented import path is 8-bit NV12 only.
                    ten_bit: false,
                    modes,
                    rate_source: "v4l2-size-only".into(),
                });
            }
        }
    }
    FrameConnectCapabilities {
        version: CAPABILITY_VERSION,
        decoders,
        ..Default::default()
    }
}

#[cfg(target_os = "linux")]
pub fn probe_v4l2(display_rates: &[f32]) -> FrameConnectCapabilities {
    let device = ["/dev/video-dec0", "/dev/video22"]
        .into_iter()
        .find_map(probe_device);
    if let Some(device) = device {
        build_capabilities_from_v4l2_info(&[device], display_rates)
    } else {
        FrameConnectCapabilities {
            version: CAPABILITY_VERSION,
            probe_error: Some(
                "No V4L2 decoder with the supported NV12 output layout was found".into(),
            ),
            ..Default::default()
        }
    }
}

#[cfg(target_os = "linux")]
fn probe_device(path: &str) -> Option<V4l2DeviceInfo> {
    use std::{
        fs::File,
        os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    };
    #[repr(C)]
    struct Capability {
        driver: [u8; 16],
        card: [u8; 32],
        bus: [u8; 32],
        version: u32,
        capabilities: u32,
        device_caps: u32,
        reserved: [u32; 3],
    }
    #[repr(C)]
    struct FormatDescription {
        index: u32,
        kind: u32,
        flags: u32,
        description: [u8; 32],
        fourcc: u32,
        mbus_code: u32,
        reserved: [u32; 3],
    }
    #[repr(C, align(8))]
    struct Format {
        kind: u32,
        padding: u32,
        data: [u32; 50],
    }
    fn format(codec: u32, width: u32, height: u32) -> Format {
        let mut f = Format {
            kind: 10,
            padding: 0,
            data: [0; 50],
        };
        f.data[0] = width;
        f.data[1] = height;
        f.data[2] = codec;
        f
    }
    fn fourccs(fd: i32, kind: u32) -> Vec<u32> {
        let mut result = Vec::new();
        for index in 0..64 {
            let mut desc = FormatDescription {
                index,
                kind,
                flags: 0,
                description: [0; 32],
                fourcc: 0,
                mbus_code: 0,
                reserved: [0; 3],
            };
            if unsafe { libc::ioctl(fd, 0xc0405602 as libc::c_ulong, &mut desc) } != 0 {
                break;
            }
            result.push(desc.fourcc);
        }
        result
    }
    let file = File::options()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    let fd = file.as_raw_fd();
    let mut cap: Capability = unsafe { std::mem::zeroed() };
    if unsafe { libc::ioctl(fd, 0x80685600 as libc::c_ulong, &mut cap) } != 0 {
        return None;
    }
    let caps = if cap.capabilities & 0x80000000 != 0 {
        cap.device_caps
    } else {
        cap.capabilities
    };
    if caps & 0x4000 == 0 {
        return None;
    }
    let mut dimensions = HashMap::new();
    for codec in fourccs(fd, 10) {
        if ![V4L2_PIX_FMT_HEVC, V4L2_PIX_FMT_H264, V4L2_PIX_FMT_AV1].contains(&codec) {
            continue;
        }
        // CAPTURE formats depend on the selected OUTPUT codec.
        let mut selected = format(codec, 640, 360);
        if unsafe { libc::ioctl(fd, 0xc0d05605 as libc::c_ulong, &mut selected) } != 0
            || selected.data[2] != codec
            || !fourccs(fd, 9).contains(&V4L2_PIX_FMT_NV12)
        {
            continue;
        }
        let mut sizes = Vec::new();
        for eye_size in [2176, 2560, 2880] {
            let mut candidate = format(codec, eye_size * 2, eye_size);
            if unsafe { libc::ioctl(fd, 0xc0d05640 as libc::c_ulong, &mut candidate) } == 0
                && candidate.data[2] == codec
                && candidate.data[0] == eye_size * 2
                && candidate.data[1] == eye_size
            {
                sizes.push([eye_size * 2, eye_size]);
            }
        }
        dimensions.insert(codec, sizes);
    }
    if dimensions.is_empty() {
        return None;
    }
    let card = String::from_utf8_lossy(&cap.card)
        .trim_end_matches('\0')
        .into();
    Some(V4l2DeviceInfo {
        card,
        path: path.into(),
        dimensions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_probed_sizes_and_actual_display_rates_are_candidates() {
        let device = V4l2DeviceInfo {
            card: "iris_decoder".into(),
            path: "/dev/video-dec0".into(),
            dimensions: HashMap::from([(V4L2_PIX_FMT_HEVC, vec![[4352, 2176]])]),
        };
        let caps = build_capabilities_from_v4l2_info(&[device], &[144.0]);
        assert!(
            caps.decoder("video/hevc", 4352, 2176, 144.0, false)
                .is_some()
        );
        assert!(
            caps.decoder("video/hevc", 4352, 2176, 120.0, false)
                .is_none()
        );
        assert!(
            caps.decoder("video/hevc", 5120, 2560, 144.0, false)
                .is_none()
        );
        assert!(
            caps.decoder("video/hevc", 4352, 2176, 144.0, true)
                .is_none()
        );
        assert!(
            caps.decoder("video/av01", 4352, 2176, 144.0, false)
                .is_none()
        );
        assert_eq!(caps.decoders[0].rate_source, "v4l2-size-only");
    }
}
