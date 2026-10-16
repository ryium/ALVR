use alvr_client_core::video_decoder::{VideoDecoderConfig, annexb, create_decoder};
use alvr_common::{
    anyhow::{Context, Result, anyhow, bail, ensure},
    parking_lot::Mutex,
};
use alvr_graphics::GraphicsContext;
use alvr_session::CodecType;
use glow::HasContext;
use serde::Serialize;
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

#[derive(Debug, Serialize, Default)]
pub struct DecodeTestReport {
    pub file_path: String,
    pub file_size_bytes: usize,
    pub total_input_nals: usize,
    pub total_frames_sent: usize,
    pub total_frames_decoded: usize,
    pub total_frames_rendered: usize,
    pub total_frames_dropped: usize,
    pub target_fps: f32,
    pub achieved_fps: f32,
    pub avg_latency_ms: f32,
    pub min_latency_ms: f32,
    pub max_latency_ms: f32,
    pub application_full_frame_cpu_copies: usize,
    pub pixel_readback_bytes: usize,
    pub pixel_samples_vary: bool,
    pub decoded_width: u32,
    pub decoded_height: u32,
    pub drm_fourcc: String,
    pub planes_count: usize,
    pub success: bool,
    pub error: Option<String>,
}
pub struct DecodeTestOptions {
    pub file_path: PathBuf,
    pub max_frames: Option<usize>,
    pub fps: f32,
    pub output_path: Option<PathBuf>,
    pub verify_pixels: bool,
    pub repeat: usize,
}

pub fn run_decode_test(options: DecodeTestOptions) -> Result<()> {
    let mut report = DecodeTestReport {
        file_path: options.file_path.display().to_string(),
        target_fps: options.fps,
        ..Default::default()
    };
    let result = decode(&options, &mut report);
    report.total_frames_dropped = report
        .total_frames_sent
        .saturating_sub(report.total_frames_decoded);
    if let Err(e) = &result {
        report.error = Some(format!("{e:#}"));
    }
    let json = serde_json::to_string_pretty(&report)?;
    println!("{json}");
    if let Some(path) = &options.output_path {
        fs::write(path, &json)?;
    }
    result
}

fn decode(options: &DecodeTestOptions, report: &mut DecodeTestReport) -> Result<()> {
    let data = fs::read(&options.file_path).context("Read HEVC Annex-B file")?;
    report.file_size_bytes = data.len();
    report.total_input_nals = annexb::split_annex_b(&data).len();
    let frames = annexb::hevc_access_units(&data);
    let available = frames
        .len()
        .checked_mul(options.repeat)
        .context("Frame count overflow")?;
    let count = options.max_frames.unwrap_or(available).min(available);
    ensure!(count > 0, "No HEVC access units to decode");
    let error = Arc::new(Mutex::new(None));
    let (mut sink, mut source) = create_decoder(
        VideoDecoderConfig {
            decoder_name: Some("v4l2@/dev/video-dec0".into()),
            // The stateful decoder parses actual dimensions from the first access unit.
            encoded_width: 640,
            encoded_height: 360,
            codec: CodecType::Hevc,
            max_buffering_frames: 2.0,
            ..Default::default()
        },
        {
            let error = Arc::clone(&error);
            move |result| {
                if let Err(e) = result {
                    *error.lock() = Some(format!("{e:#}"));
                }
            }
        },
    )?;
    let gfx = GraphicsContext::new_gl();
    gfx.make_current();
    let verifier = GpuVerifier::new(&gfx)?;
    let interval = if options.fps > 0.0 {
        Duration::from_secs_f32(1.0 / options.fps)
    } else {
        Duration::ZERO
    };
    let start = Instant::now();
    let mut last_progress = start;
    let mut sent = HashMap::new();
    let mut latencies = Vec::new();
    let mut samples = Vec::new();
    while report.total_frames_decoded < count {
        if let Some(e) = error.lock().take() {
            bail!("{e}");
        }
        let now = Instant::now();
        let index = report.total_frames_sent;
        if index < count && now.duration_since(start) >= interval * index as u32 {
            // Unique timestamps even for unthrottled feed; the decoder must return them exactly.
            let pts = Duration::from_micros(index as u64 + 1);
            if sink.push_nal(pts, &frames[index % frames.len()]) {
                sent.insert(pts, now);
                report.total_frames_sent += 1;
                last_progress = now;
            }
        }
        if let Some((pts, alvr_graphics::VideoFrame::DmaBuf(frame))) = source.get_frame() {
            report.total_frames_decoded += 1;
            last_progress = Instant::now();
            let queued = sent
                .remove(&pts)
                .context("Decoded timestamp was not submitted")?;
            latencies.push(queued.elapsed().as_secs_f32() * 1000.0);
            report.decoded_width = frame.width;
            report.decoded_height = frame.height;
            report.drm_fourcc = format!("0x{:08x}", frame.drm_fourcc);
            report.planes_count = frame.planes.len();
            verifier.render(&frame)?;
            report.total_frames_rendered += 1;
            if options.verify_pixels {
                let pixels = verifier.read_pixels()?;
                report.pixel_readback_bytes += pixels.len();
                samples.extend(pixels.chunks_exact(4).map(|p| [p[0], p[1], p[2]]));
            }
        }
        ensure!(
            last_progress.elapsed() < Duration::from_secs(5),
            "Decoder made no progress for five seconds"
        );
        thread::sleep(Duration::from_micros(500));
    }
    report.total_frames_dropped = report
        .total_frames_sent
        .saturating_sub(report.total_frames_decoded);
    report.achieved_fps = report.total_frames_decoded as f32 / start.elapsed().as_secs_f32();
    if !latencies.is_empty() {
        report.avg_latency_ms = latencies.iter().sum::<f32>() / latencies.len() as f32;
        report.min_latency_ms = latencies.iter().copied().fold(f32::INFINITY, f32::min);
        report.max_latency_ms = latencies.iter().copied().fold(0.0, f32::max);
    }
    report.pixel_samples_vary = samples
        .first()
        .is_some_and(|first| samples.iter().any(|p| p != first));
    ensure!(
        !options.verify_pixels || report.pixel_samples_vary,
        "GPU readback is uniform; use a non-uniform test pattern"
    );
    report.success = report.total_frames_decoded == count && report.total_frames_rendered == count;
    Ok(())
}

/// A small GPU render target verifies sampling, not merely EGL image creation.
/// Optional 32x32 readback is diagnostic only; the full video frame stays on the GPU.
struct GpuVerifier<'a> {
    gfx: &'a GraphicsContext,
    external: glow::Texture,
    color: glow::Texture,
    framebuffer: glow::Framebuffer,
    program: glow::Program,
    vao: glow::VertexArray,
}
impl<'a> GpuVerifier<'a> {
    fn new(gfx: &'a GraphicsContext) -> Result<Self> {
        let gl = &gfx.gl_context;
        unsafe {
            let program = gl.create_program().map_err(|e| anyhow!(e))?;
            for (kind, text) in [
                (
                    glow::VERTEX_SHADER,
                    "#version 300 es\nout vec2 uv; void main(){vec2 p=vec2(float((gl_VertexID<<1)&2),float(gl_VertexID&2));uv=p;gl_Position=vec4(p*2.0-1.0,0,1);}",
                ),
                (
                    glow::FRAGMENT_SHADER,
                    "#version 300 es\n#extension GL_OES_EGL_image_external_essl3 : require\nprecision mediump float;in vec2 uv;uniform samplerExternalOES video;out vec4 color;void main(){color=texture(video,uv);}",
                ),
            ] {
                let shader = gl.create_shader(kind).map_err(|e| anyhow!(e))?;
                gl.shader_source(shader, text);
                gl.compile_shader(shader);
                ensure!(
                    gl.get_shader_compile_status(shader),
                    "{}",
                    gl.get_shader_info_log(shader)
                );
                gl.attach_shader(program, shader);
                gl.delete_shader(shader);
            }
            gl.link_program(program);
            ensure!(
                gl.get_program_link_status(program),
                "{}",
                gl.get_program_info_log(program)
            );
            let external = gl.create_texture().map_err(|e| anyhow!(e))?;
            let color = gl.create_texture().map_err(|e| anyhow!(e))?;
            gl.bind_texture(glow::TEXTURE_2D, Some(color));
            gl.tex_storage_2d(glow::TEXTURE_2D, 1, glow::RGBA8, 32, 32);
            let framebuffer = gl.create_framebuffer().map_err(|e| anyhow!(e))?;
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(color),
                0,
            );
            ensure!(
                gl.check_framebuffer_status(glow::FRAMEBUFFER) == glow::FRAMEBUFFER_COMPLETE,
                "Incomplete GPU verification framebuffer"
            );
            let vao = gl.create_vertex_array().map_err(|e| anyhow!(e))?;
            Ok(Self {
                gfx,
                external,
                color,
                framebuffer,
                program,
                vao,
            })
        }
    }
    fn render(&self, frame: &alvr_graphics::DmaBufImage) -> Result<()> {
        let gl = &self.gfx.gl_context;
        unsafe {
            gl.active_texture(glow::TEXTURE0);
            self.gfx
                .render_dmabuf_using_texture(frame, self.external, || {
                    gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.framebuffer));
                    gl.viewport(0, 0, 32, 32);
                    gl.disable(glow::SCISSOR_TEST);
                    gl.use_program(Some(self.program));
                    gl.bind_vertex_array(Some(self.vao));
                    gl.draw_arrays(glow::TRIANGLES, 0, 3);
                })?;
            ensure!(gl.get_error() == glow::NO_ERROR, "GL rendering failed");
        }
        Ok(())
    }
    fn read_pixels(&self) -> Result<Vec<u8>> {
        let mut pixels = vec![0u8; 32 * 32 * 4];
        unsafe {
            self.gfx.gl_context.read_pixels(
                0,
                0,
                32,
                32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut pixels)),
            );
            ensure!(
                self.gfx.gl_context.get_error() == glow::NO_ERROR,
                "GL readback failed"
            );
        }
        Ok(pixels)
    }
}
impl Drop for GpuVerifier<'_> {
    fn drop(&mut self) {
        self.gfx.clear_dma_buf_cache();
        unsafe {
            let gl = &self.gfx.gl_context;
            gl.delete_program(self.program);
            gl.delete_vertex_array(self.vao);
            gl.delete_framebuffer(self.framebuffer);
            gl.delete_texture(self.color);
            gl.delete_texture(self.external);
        }
    }
}
