use alvr_common::anyhow::Result;
use serde::{Deserialize, Serialize};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct SystemProbe {
    pub os_release: HashMap<String, String>,
    pub kernel_release: String,
    pub architecture: String,
    pub glibc_version: String,
    pub current_uid: u32,
    pub current_gid: u32,
    pub groups: Vec<u32>,
    pub device_permissions: HashMap<String, DevicePermInfo>,
    pub battery_capacity: Option<u32>,
    pub battery_status: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct DevicePermInfo {
    pub exists: bool,
    pub readable: bool,
    pub writable: bool,
    pub mode_octal: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct OpenXrProbe {
    pub runtime_name: Option<String>,
    pub runtime_version: Option<String>,
    pub available_extensions: Vec<String>,
    pub available_layers: Vec<String>,
    pub supports_mndx_egl: bool,
    pub supports_khr_opengles: bool,
    pub system_name: Option<String>,
    pub tracking_properties: Option<String>,
    pub recommended_resolution: Option<[u32; 2]>,
    pub max_resolution: Option<[u32; 2]>,
    pub supported_refresh_rates: Vec<f32>,
    pub swapchain_formats: Vec<u32>,
    pub session_creation_result: String,
    pub error: Option<String>,
    pub clock_conversion_ok: bool,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct V4l2DeviceProbe {
    pub path: String,
    pub driver: String,
    pub card: String,
    pub bus_info: String,
    pub capabilities: u32,
    pub is_decoder: bool,
    pub output_formats: Vec<V4l2FormatInfo>,
    pub capture_formats: Vec<V4l2FormatInfo>,
    pub has_display_delay_control: bool,
    pub try_fmt_4352x2176_hevc: Option<bool>,
    pub try_fmt_5120x2560_hevc: Option<bool>,
    pub supports_av1: bool,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct V4l2FormatInfo {
    pub fourcc: String,
    pub fourcc_hex: String,
    pub description: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct EglProbe {
    pub client_extensions: Vec<String>,
    pub display_extensions: Vec<String>,
    pub supports_dmabuf_import: bool,
    pub supports_dmabuf_modifiers: bool,
    pub gl_renderer: Option<String>,
    pub gl_vendor: Option<String>,
    pub gl_version: Option<String>,
    pub gl_extensions: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AudioProbe {
    pub host: String,
    pub output_devices: Vec<String>,
    pub input_devices: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ProbeReport {
    pub timestamp: String,
    pub stage1_ok: bool,
    pub system: SystemProbe,
    pub openxr: OpenXrProbe,
    pub v4l2: Vec<V4l2DeviceProbe>,
    pub egl: EglProbe,
    pub audio: AudioProbe,
    pub summary_notes: Vec<String>,
}

pub fn run_probe(output_path: Option<&Path>) -> Result<()> {
    println!("=== FrameConnect SteamOS Native Client Probe ===");

    let mut report = ProbeReport {
        timestamp: humantime_now(),
        stage1_ok: false,
        ..Default::default()
    };

    // 1. Probe System
    println!("[1/5] Probing System...");
    report.system = probe_system();

    // 2. Probe V4L2
    println!("[2/5] Probing V4L2 Decoders...");
    report.v4l2 = probe_v4l2();

    // 3. Probe EGL / GLES
    println!("[3/5] Probing EGL and GLES...");
    report.egl = probe_egl();

    // 4. Probe OpenXR
    println!("[4/5] Probing OpenXR Runtime...");
    report.openxr = probe_openxr();

    // 5. Probe Audio
    println!("[5/5] Probing Audio Subsystem...");
    report.audio = probe_audio();

    // Evaluate Stage 1 success
    let v4l2_ok = report.v4l2.iter().any(|d| d.is_decoder);
    let openxr_ok = report.openxr.session_creation_result == "Session OK";
    report.stage1_ok = v4l2_ok && openxr_ok;

    if v4l2_ok {
        report
            .summary_notes
            .push("Found hardware V4L2 decoder".into());
    } else {
        report
            .summary_notes
            .push("WARNING: No V4L2 decoder found".into());
    }
    if openxr_ok {
        report.summary_notes.push(format!(
            "OpenXR runtime detected: {}",
            report.openxr.runtime_name.as_deref().unwrap_or("unknown")
        ));
    } else {
        report
            .summary_notes
            .push("WARNING: OpenXR runtime not detected".into());
    }

    // Determine target output path
    let default_output_dir = dirs_fallback().join("diagnostics");
    fs::create_dir_all(&default_output_dir)?;
    let out_file = output_path.map(PathBuf::from).unwrap_or_else(|| {
        let ts_str = humantime_now().replace([':', ' ', '-'], "_");
        default_output_dir.join(format!("probe_{ts_str}.json"))
    });

    let json_bytes = serde_json::to_string_pretty(&report)?;
    fs::write(&out_file, json_bytes)?;
    println!("\nProbe report written to: {}", out_file.display());
    println!("Stage 1 Check Passed: {}", report.stage1_ok);

    Ok(())
}

fn humantime_now() -> String {
    let now = SystemTime::now();
    let dur = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{}", dur.as_secs(), dur.subsec_millis())
}

fn dirs_fallback() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".local/share/frameconnect")
    } else {
        PathBuf::from("/tmp/frameconnect")
    }
}

fn probe_system() -> SystemProbe {
    let mut sys = SystemProbe::default();

    // OS Release
    if let Ok(content) = fs::read_to_string("/etc/os-release") {
        for line in content.lines() {
            if let Some((k, v)) = line.split_once('=') {
                sys.os_release
                    .insert(k.trim().to_string(), v.trim().trim_matches('"').to_string());
            }
        }
    }

    #[cfg(target_os = "linux")]
    unsafe {
        let mut uts = std::mem::zeroed::<libc::utsname>();
        if libc::uname(&mut uts) == 0 {
            sys.kernel_release = std::ffi::CStr::from_ptr(uts.release.as_ptr())
                .to_string_lossy()
                .into_owned();
            sys.architecture = std::ffi::CStr::from_ptr(uts.machine.as_ptr())
                .to_string_lossy()
                .into_owned();
        }
        let libc_ver = libc::gnu_get_libc_version();
        if !libc_ver.is_null() {
            sys.glibc_version = std::ffi::CStr::from_ptr(libc_ver)
                .to_string_lossy()
                .into_owned();
        }
        sys.current_uid = libc::getuid();
        sys.current_gid = libc::getgid();

        let mut groups = [0u32; 64];
        let ngroups = libc::getgroups(64, groups.as_mut_ptr() as *mut libc::gid_t);
        if ngroups > 0 {
            sys.groups = groups[..ngroups as usize].to_vec();
        }
    }

    // Check device nodes
    let check_paths = [
        "/dev/video-dec0",
        "/dev/video22",
        "/dev/dri/card0",
        "/dev/dri/renderD128",
        "/dev/media0",
    ];
    for p in check_paths {
        let path = Path::new(p);
        let exists = path.exists();
        #[allow(unused_mut)]
        let mut readable = false;
        #[allow(unused_mut)]
        let mut writable = false;
        #[allow(unused_mut)]
        let mut mode_octal = String::new();
        if exists {
            #[cfg(target_os = "linux")]
            unsafe {
                let c_path = std::ffi::CString::new(p).unwrap();
                readable = libc::access(c_path.as_ptr(), libc::R_OK) == 0;
                writable = libc::access(c_path.as_ptr(), libc::W_OK) == 0;
            }
            #[cfg(unix)]
            if let Ok(meta) = fs::metadata(path) {
                mode_octal = format!("{:o}", meta.permissions().mode());
            }
        }
        sys.device_permissions.insert(
            p.to_string(),
            DevicePermInfo {
                exists,
                readable,
                writable,
                mode_octal,
            },
        );
    }

    // Battery
    let bat_path = "/sys/class/power_supply/max1720x_bat_7-36";
    if let Ok(cap_s) = fs::read_to_string(format!("{bat_path}/capacity")) {
        sys.battery_capacity = cap_s.trim().parse().ok();
    }
    if let Ok(stat_s) = fs::read_to_string(format!("{bat_path}/status")) {
        sys.battery_status = Some(stat_s.trim().to_string());
    }

    sys
}

fn probe_v4l2() -> Vec<V4l2DeviceProbe> {
    let mut probes = Vec::new();
    let video_dir = Path::new("/dev");
    if let Ok(entries) = fs::read_dir(video_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with("video") {
                if let Some(dev_probe) = probe_single_v4l2(&path) {
                    probes.push(dev_probe);
                }
            }
        }
    }
    probes.sort_by(|a, b| a.path.cmp(&b.path));
    probes
}

#[allow(unused_variables)]
fn probe_single_v4l2(path: &Path) -> Option<V4l2DeviceProbe> {
    #[cfg(target_os = "linux")]
    {
        use alvr_client_core::video_decoder::v4l2_sys::*;
        use std::os::fd::AsRawFd;

        let file = fs::File::options().read(true).write(true).open(path).ok()?;
        let fd = file.as_raw_fd();

        #[repr(C)]
        #[derive(Default)]
        struct v4l2_capability {
            driver: [u8; 16],
            card: [u8; 32],
            bus_info: [u8; 32],
            version: u32,
            capabilities: u32,
            device_caps: u32,
            reserved: [u32; 3],
        }
        const VIDIOC_QUERYCAP: libc::c_ulong = 0x80685600;

        let mut cap = v4l2_capability::default();
        if unsafe { libc::ioctl(fd, VIDIOC_QUERYCAP as _, &mut cap) } != 0 {
            return None;
        }

        let driver = c_buf_to_string(&cap.driver);
        let card = c_buf_to_string(&cap.card);
        let bus_info = c_buf_to_string(&cap.bus_info);
        let device_caps = if cap.capabilities & 0x80000000 != 0 {
            cap.device_caps
        } else {
            cap.capabilities
        };

        let has_output_mplane = device_caps & 0x00001000 != 0;
        let has_capture_mplane = device_caps & 0x00002000 != 0;
        let is_decoder = device_caps & 0x00004000 != 0 || (has_output_mplane && has_capture_mplane);

        let mut dev_probe = V4l2DeviceProbe {
            path: path.to_string_lossy().into_owned(),
            driver,
            card,
            bus_info,
            capabilities: device_caps,
            is_decoder,
            ..Default::default()
        };

        if is_decoder {
            // Enumerate output formats
            dev_probe.output_formats = enum_formats(fd, V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE);
            // Enumerate capture formats
            dev_probe.capture_formats = enum_formats(fd, V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE);
            dev_probe.is_decoder = dev_probe
                .output_formats
                .iter()
                .any(|f| matches!(f.fourcc.as_str(), "HEVC" | "H264" | "VP90" | "AV01"));
            if !dev_probe.is_decoder {
                return Some(dev_probe);
            }

            // Check AV1 presence
            dev_probe.supports_av1 = dev_probe
                .output_formats
                .iter()
                .any(|f| f.fourcc == "AV01" || f.fourcc == "AV1F");

            // Check display delay control
            let mut ctrl = v4l2_control {
                id: V4L2_CID_MPEG_VIDEO_DEC_DISPLAY_DELAY,
                value: 0,
            };
            dev_probe.has_display_delay_control =
                unsafe { libc::ioctl(fd, VIDIOC_S_CTRL as _, &mut ctrl) } == 0;

            // Try 4352x2176 HEVC
            dev_probe.try_fmt_4352x2176_hevc = Some(try_hevc_resolution(fd, 4352, 2176));
            // Try 5120x2560 HEVC
            dev_probe.try_fmt_5120x2560_hevc = Some(try_hevc_resolution(fd, 5120, 2560));
        }

        Some(dev_probe)
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

#[cfg(target_os = "linux")]
fn c_buf_to_string(buf: &[u8]) -> String {
    let nul_pos = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..nul_pos]).into_owned()
}

#[cfg(target_os = "linux")]
fn enum_formats(fd: std::os::fd::RawFd, buf_type: u32) -> Vec<V4l2FormatInfo> {
    #[repr(C)]
    struct v4l2_fmtdesc {
        index: u32,
        type_: u32,
        flags: u32,
        description: [u8; 32],
        pixelformat: u32,
        mbus_code: u32,
        reserved: [u32; 3],
    }
    const VIDIOC_ENUM_FMT: libc::c_ulong = 0xc0405602;

    let mut formats = Vec::new();
    for i in 0..64 {
        let mut desc = v4l2_fmtdesc {
            index: i,
            type_: buf_type,
            flags: 0,
            description: [0; 32],
            pixelformat: 0,
            mbus_code: 0,
            reserved: [0; 3],
        };
        if unsafe { libc::ioctl(fd, VIDIOC_ENUM_FMT as _, &mut desc) } != 0 {
            break;
        }
        let fourcc_bytes = desc.pixelformat.to_le_bytes();
        let fourcc = String::from_utf8_lossy(&fourcc_bytes).into_owned();
        formats.push(V4l2FormatInfo {
            fourcc,
            fourcc_hex: format!("0x{:08x}", desc.pixelformat),
            description: c_buf_to_string(&desc.description),
        });
    }
    formats
}

#[cfg(target_os = "linux")]
fn try_hevc_resolution(fd: std::os::fd::RawFd, w: u32, h: u32) -> bool {
    use alvr_client_core::video_decoder::v4l2_sys::*;

    const VIDIOC_TRY_FMT: libc::c_ulong = 0xc0d05640;
    let mut fmt = v4l2_format {
        type_: V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE,
        ..Default::default()
    };
    unsafe {
        let pix = &mut fmt.fmt.pix_mp;
        pix.width = w;
        pix.height = h;
        pix.pixelformat = V4L2_PIX_FMT_HEVC;
        pix.num_planes = 1;
        pix.plane_fmt[0].sizeimage = w * h;
    }
    unsafe {
        libc::ioctl(fd, VIDIOC_TRY_FMT as _, &mut fmt) == 0
            && fmt.fmt.pix_mp.pixelformat == V4L2_PIX_FMT_HEVC
            && fmt.fmt.pix_mp.width == w
            && fmt.fmt.pix_mp.height == h
    }
}

fn probe_egl() -> EglProbe {
    #[allow(unused_mut)]
    let mut probe = EglProbe::default();

    #[cfg(target_os = "linux")]
    {
        use khronos_egl as egl;
        let egl_inst = match unsafe { egl::DynamicInstance::<egl::EGL1_4>::load_required() } {
            Ok(inst) => inst,
            Err(e) => {
                alvr_common::error!("Failed to load EGL library: {e}");
                return probe;
            }
        };

        if let Ok(exts) = egl_inst.query_string(None, egl::EXTENSIONS) {
            probe.client_extensions = exts
                .to_string_lossy()
                .split_whitespace()
                .map(String::from)
                .collect();
        }

        let dpy = unsafe { egl_inst.get_display(egl::DEFAULT_DISPLAY) };
        if let Some(dpy) = dpy {
            if egl_inst.initialize(dpy).is_ok() {
                if let Ok(exts) = egl_inst.query_string(Some(dpy), egl::EXTENSIONS) {
                    let dpy_exts_s = exts.to_string_lossy();
                    probe.display_extensions =
                        dpy_exts_s.split_whitespace().map(String::from).collect();
                    probe.supports_dmabuf_import =
                        dpy_exts_s.contains("EGL_EXT_image_dma_buf_import");
                    probe.supports_dmabuf_modifiers =
                        dpy_exts_s.contains("EGL_EXT_image_dma_buf_import_modifiers");
                }
            }
        }
    }

    probe
}

fn probe_openxr() -> OpenXrProbe {
    use openxr as xr;

    let mut probe = OpenXrProbe::default();

    let xr_entry = match unsafe { xr::Entry::load() } {
        Ok(entry) => entry,
        Err(e) => {
            probe.error = Some(format!("Failed to load OpenXR entry: {e}"));
            return probe;
        }
    };

    if let Ok(exts) = xr_entry.enumerate_extensions() {
        probe.available_extensions = exts
            .names()
            .iter()
            .map(|s| {
                std::ffi::CStr::from_bytes_with_nul(s)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        probe.supports_khr_opengles = exts.khr_opengl_es_enable;
        probe.supports_mndx_egl = exts.mndx_egl_enable;
    }

    if let Ok(layers) = xr_entry.enumerate_layers() {
        probe.available_layers = layers.iter().map(|l| l.layer_name.to_string()).collect();
    }

    // Try instance creation
    let mut selected_exts = xr::ExtensionSet::default();
    selected_exts.khr_opengl_es_enable = probe.supports_khr_opengles;
    selected_exts.mndx_egl_enable = probe.supports_mndx_egl;
    selected_exts.fb_display_refresh_rate = probe
        .available_extensions
        .iter()
        .any(|e| e == "XR_FB_display_refresh_rate");
    selected_exts.khr_convert_timespec_time = probe
        .available_extensions
        .iter()
        .any(|e| e == "XR_KHR_convert_timespec_time");

    let inst_res = xr_entry.create_instance(
        &xr::ApplicationInfo {
            application_name: "FrameConnectProbe",
            application_version: 1,
            engine_name: "ALVR",
            engine_version: 0,
            api_version: xr::Version::new(1, 0, 34),
        },
        &selected_exts,
        &[],
    );

    let instance = match inst_res {
        Ok(inst) => inst,
        Err(e) => {
            probe.session_creation_result = format!("CreateInstance failed: {e:?}");
            return probe;
        }
    };

    if let Ok(props) = instance.properties() {
        probe.runtime_name = Some(props.runtime_name);
        probe.runtime_version = Some(format!("{}", props.runtime_version));
    }
    if selected_exts.khr_convert_timespec_time {
        if let Ok(first) = instance.now() {
            std::thread::sleep(std::time::Duration::from_millis(2));
            probe.clock_conversion_ok = instance.now().is_ok_and(|later| later.as_nanos() > first.as_nanos());
        }
    }

    let system_id = match instance.system(xr::FormFactor::HEAD_MOUNTED_DISPLAY) {
        Ok(sys) => sys,
        Err(e) => {
            probe.session_creation_result = format!("GetSystem failed: {e:?}");
            return probe;
        }
    };

    if let Ok(props) = instance.system_properties(system_id) {
        probe.system_name = Some(props.system_name);
        probe.tracking_properties = Some(format!(
            "orientation={}, position={}",
            props.tracking_properties.orientation_tracking,
            props.tracking_properties.position_tracking
        ));
    }

    if let Ok(views) = instance
        .enumerate_view_configuration_views(system_id, xr::ViewConfigurationType::PRIMARY_STEREO)
    {
        if !views.is_empty() {
            probe.recommended_resolution = Some([
                views[0].recommended_image_rect_width,
                views[0].recommended_image_rect_height,
            ]);
            probe.max_resolution = Some([
                views[0].max_image_rect_width,
                views[0].max_image_rect_height,
            ]);
        }
    }

    if !probe.supports_mndx_egl || !probe.supports_khr_opengles {
        probe.session_creation_result = "Required EGL/GLES binding unavailable".into();
        return probe;
    }
    use alvr_client_openxr::graphics::{ClientGraphics, session_create_info};
    if let Err(e) = instance.graphics_requirements::<ClientGraphics>(system_id) {
        probe.session_creation_result = format!("Graphics requirements failed: {e}");
        return probe;
    }
    let gfx = alvr_graphics::GraphicsContext::new_gl();
    gfx.make_current();
    match unsafe {
        instance.create_session::<ClientGraphics>(system_id, &session_create_info(&gfx))
    } {
        Ok((session, _, _)) => {
            probe.swapchain_formats = session.enumerate_swapchain_formats().unwrap_or_default();
            if selected_exts.fb_display_refresh_rate {
                probe.supported_refresh_rates = session
                    .enumerate_display_refresh_rates()
                    .unwrap_or_default();
            }
            probe.session_creation_result = "Session OK".into();
        }
        Err(e) => {
            probe.session_creation_result = format!("CreateSession failed: {e}");
        }
    }
    probe
}

fn probe_audio() -> AudioProbe {
    #[allow(unused_mut)]
    let mut probe = AudioProbe::default();

    #[cfg(target_os = "linux")]
    {
        use cpal::traits::{DeviceTrait, HostTrait};
        let host = cpal::default_host();
        probe.host = host.id().name().to_string();

        if let Ok(devices) = host.output_devices() {
            for dev in devices {
                if let Ok(name) = dev.name() {
                    probe.output_devices.push(name);
                }
            }
        }

        if let Ok(devices) = host.input_devices() {
            for dev in devices {
                if let Ok(name) = dev.name() {
                    probe.input_devices.push(name);
                }
            }
        }
    }

    probe
}
