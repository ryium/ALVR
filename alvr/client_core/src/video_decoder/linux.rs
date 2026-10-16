use super::{VideoDecoderConfig, v4l2_sys::*};
use alvr_common::{
    RelaxedAtomic,
    anyhow::{Context, Result, anyhow, bail},
    error, info, warn,
    parking_lot::Mutex,
};
use alvr_graphics::{DRM_FORMAT_NV12, DmaBufImage, DmaBufLease, DmaBufPlane, VideoFrame, YuvColor};
use alvr_session::CodecType;
use std::{
    collections::{HashMap, VecDeque},
    fs::File,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
        unix::fs::OpenOptionsExt,
    },
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

struct OutputBuffer {
    ptr: *mut u8,
    size: usize,
}
unsafe impl Send for OutputBuffer {}
impl Drop for OutputBuffer {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.ptr.cast(), self.size);
        }
    }
}

static NEXT_BUFFER_ID: AtomicU64 = AtomicU64::new(1);

struct CaptureBuffer {
    planes: Vec<DmaBufPlane>,
    _fds: Vec<OwnedFd>,
    memory_planes: u32,
    id: u64,
    width: u32,
    height: u32,
}
unsafe impl Send for CaptureBuffer {}

struct V4l2DeviceInner {
    file: File,
    output_buffers: Vec<OutputBuffer>,
    capture_buffers: Vec<CaptureBuffer>,
    capture_started: bool,
    config: VideoDecoderConfig,
    timestamp_map: HashMap<u32, Duration>,
    next_sequence: u32,
    unused_output_indices: VecDeque<u32>,
    releasing_buffer_indices: VecDeque<u32>,
}

unsafe impl Send for V4l2DeviceInner {}
impl Drop for V4l2DeviceInner {
    fn drop(&mut self) {
        for mut kind in [
            V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE,
            V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE,
        ] {
            unsafe {
                libc::ioctl(self.file.as_raw_fd(), VIDIOC_STREAMOFF as _, &mut kind);
            }
        }
    }
}

pub struct VideoDecoderSink {
    inner: Arc<Mutex<Option<V4l2DeviceInner>>>,
    config_sent: bool,
}

unsafe impl Send for VideoDecoderSink {}

impl VideoDecoderSink {
    pub fn push_frame_nal(&mut self, timestamp: Duration, data: &[u8]) -> Result<bool> {
        let Some(inner) = &mut *self.inner.lock() else {
            return Ok(false);
        };

        let fd = inner.file.as_raw_fd();

        // Check if there is an empty output buffer available
        let mut planes = [v4l2_plane::default(); 8];
        let mut buf = v4l2_buffer {
            type_: V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE,
            memory: V4L2_MEMORY_MMAP,
            length: 1,
            m: v4l2_buffer_m {
                planes: planes.as_mut_ptr(),
            },
            ..Default::default()
        };

        let buffer_index = if let Some(index) = inner.unused_output_indices.pop_front() {
            index
        } else {
            if unsafe { libc::ioctl(fd, VIDIOC_DQBUF as _, &mut buf) } == 0 {
                buf.index
            } else {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::WouldBlock {
                    return Ok(false);
                }
                return Err(error).context("V4L2 DQBUF(OUTPUT)");
            }
        };

        let out_buf = &inner.output_buffers[buffer_index as usize];
        let mut total_len = data.len();

        let need_config = !self.config_sent && !inner.config.config_buffer.is_empty();
        if need_config {
            total_len += inner.config.config_buffer.len();
        }

        if total_len > out_buf.size {
            bail!(
                "Encoded NAL ({total_len} bytes) exceeds V4L2 output buffer size ({})",
                out_buf.size
            );
        }

        let mut offset = 0;
        if need_config {
            unsafe {
                std::ptr::copy_nonoverlapping(
                    inner.config.config_buffer.as_ptr(),
                    out_buf.ptr,
                    inner.config.config_buffer.len(),
                );
            }
            offset += inner.config.config_buffer.len();
        }

        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), out_buf.ptr.add(offset), data.len());
        }

        let seq = inner.next_sequence;

        planes[0].bytesused = total_len as u32;
        planes[0].data_offset = 0;

        let mut q_buf = v4l2_buffer {
            index: buffer_index,
            type_: V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE,
            memory: V4L2_MEMORY_MMAP,
            length: 1,
            m: v4l2_buffer_m {
                planes: planes.as_mut_ptr(),
            },
            flags: V4L2_BUF_FLAG_TIMESTAMP_COPY,
            timestamp: v4l2_timeval {
                tv_sec: (seq / 1_000_000) as _,
                tv_usec: (seq % 1_000_000) as _,
            },
            ..Default::default()
        };

        let q_res = unsafe { libc::ioctl(fd, VIDIOC_QBUF as _, &mut q_buf) };
        if q_res != 0 {
            inner.unused_output_indices.push_front(buffer_index);
            bail!(
                "V4L2 VIDIOC_QBUF(OUTPUT) failed: {}",
                std::io::Error::last_os_error()
            );
        }

        self.config_sent = true;
        inner.next_sequence = inner.next_sequence.wrapping_add(1);
        inner.timestamp_map.insert(seq, timestamp);

        Ok(true)
    }
}

pub struct QueuedDmaBufFrame {
    pub timestamp: Duration,
    pub image: DmaBufImage,
}

pub struct VideoDecoderSource {
    running: Arc<RelaxedAtomic>,
    dequeue_thread: Option<JoinHandle<()>>,
    image_queue: Arc<Mutex<VecDeque<QueuedDmaBufFrame>>>,
    inner: Arc<Mutex<Option<V4l2DeviceInner>>>,
}

unsafe impl Send for VideoDecoderSource {}

impl VideoDecoderSource {
    pub fn dequeue_frame(&mut self) -> Option<(Duration, VideoFrame)> {
        let mut queue = self.image_queue.lock();
        if queue.is_empty() {
            return None;
        }
        // Always take the freshest frame to prevent latency buildup during network/decode jitter.
        // Dropping older frames releases their DmaBufLease, recycling buffers back to V4L2.
        let freshest = queue.pop_back().unwrap();
        if !queue.is_empty() {
            queue.clear();
        }
        Some((freshest.timestamp, VideoFrame::DmaBuf(freshest.image)))
    }
}

impl Drop for VideoDecoderSource {
    fn drop(&mut self) {
        self.running.set(false);
        if let Some(thread) = self.dequeue_thread.take() {
            let _ = thread.join();
        }
        self.image_queue.lock().clear();
        if let Some(inner) = &mut *self.inner.lock() {
            for mut kind in [
                V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE,
                V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE,
            ] {
                unsafe {
                    libc::ioctl(inner.file.as_raw_fd(), VIDIOC_STREAMOFF as _, &mut kind);
                }
            }
            inner.capture_started = false;
        }
    }
}

pub fn video_decoder_split(
    config: VideoDecoderConfig,
    report_frame_decoded: impl Fn(Result<Duration>) + Send + Sync + 'static,
) -> Result<(VideoDecoderSink, VideoDecoderSource)> {
    let dev_path = config
        .decoder_name
        .as_deref()
        .and_then(|n| n.split('@').nth(1))
        .unwrap_or("/dev/video-dec0");

    info!("Opening V4L2 decoder at {dev_path}");
    let file = File::options()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(dev_path)
        .with_context(|| format!("Failed to open V4L2 decoder at {dev_path}"))?;
    let fd = file.as_raw_fd();

    // 1. Set bitstream input format (OUTPUT_MPLANE)
    let fourcc = match config.codec {
        CodecType::Hevc => V4L2_PIX_FMT_HEVC,
        CodecType::H264 => V4L2_PIX_FMT_H264,
        CodecType::AV1 => V4L2_PIX_FMT_AV1,
    };

    let mut fmt = v4l2_format {
        type_: V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE,
        ..Default::default()
    };
    unsafe {
        let pix = &mut fmt.fmt.pix_mp;
        pix.width = config.encoded_width;
        pix.height = config.encoded_height;
        pix.pixelformat = fourcc;
        pix.num_planes = 1;
        pix.plane_fmt[0].sizeimage =
            (config.encoded_width * config.encoded_height).max(2 * 1024 * 1024);
    }
    if unsafe { libc::ioctl(fd, VIDIOC_S_FMT as _, &mut fmt) } != 0 {
        bail!(
            "V4L2 VIDIOC_S_FMT(OUTPUT_MPLANE) failed: {}",
            std::io::Error::last_os_error()
        );
    }
    if unsafe { fmt.fmt.pix_mp.pixelformat } != fourcc {
        bail!("V4L2 substituted the requested codec; no codec fallback is allowed");
    }

    // 2. Try setting low display delay control (for minimal streaming latency)
    let mut ctrl = v4l2_control {
        id: V4L2_CID_MPEG_VIDEO_DEC_DISPLAY_DELAY_ENABLE,
        value: 1,
    };
    unsafe { libc::ioctl(fd, VIDIOC_S_CTRL as _, &mut ctrl) };

    let mut ctrl = v4l2_control {
        id: V4L2_CID_MPEG_VIDEO_DEC_DISPLAY_DELAY,
        value: 0,
    };
    unsafe { libc::ioctl(fd, VIDIOC_S_CTRL as _, &mut ctrl) };

    // 3. Allocate OUTPUT buffers (MMAP)
    let num_output_bufs = 16u32;
    let mut req = v4l2_requestbuffers {
        count: num_output_bufs,
        type_: V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE,
        memory: V4L2_MEMORY_MMAP,
        ..Default::default()
    };
    if unsafe { libc::ioctl(fd, VIDIOC_REQBUFS as _, &mut req) } != 0 {
        bail!(
            "V4L2 VIDIOC_REQBUFS(OUTPUT) failed: {}",
            std::io::Error::last_os_error()
        );
    }

    let mut output_buffers = Vec::new();
    for i in 0..req.count {
        let mut planes = [v4l2_plane::default(); 8];
        let mut query = v4l2_buffer {
            index: i,
            type_: V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE,
            memory: V4L2_MEMORY_MMAP,
            length: 1,
            m: v4l2_buffer_m {
                planes: planes.as_mut_ptr(),
            },
            ..Default::default()
        };
        if unsafe { libc::ioctl(fd, VIDIOC_QUERYBUF as _, &mut query) } != 0 {
            bail!(
                "V4L2 VIDIOC_QUERYBUF(OUTPUT) failed: {}",
                std::io::Error::last_os_error()
            );
        }

        let plane_len = planes[0].length as usize;
        let plane_offset = unsafe { planes[0].m.mem_offset };

        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                plane_len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                plane_offset as libc::off_t,
            )
        };
        if ptr == libc::MAP_FAILED {
            bail!(
                "V4L2 mmap(OUTPUT) failed: {}",
                std::io::Error::last_os_error()
            );
        }
        output_buffers.push(OutputBuffer {
            ptr: ptr as *mut u8,
            size: plane_len,
        });
    }

    // 4. Subscribe to events (SOURCE_CHANGE, EOS)
    let sub = v4l2_event_subscription {
        type_: V4L2_EVENT_SOURCE_CHANGE,
        ..Default::default()
    };
    if unsafe { libc::ioctl(fd, VIDIOC_SUBSCRIBE_EVENT as _, &sub) } != 0 {
        bail!(
            "Cannot subscribe to V4L2 SOURCE_CHANGE: {}",
            std::io::Error::last_os_error()
        );
    }

    let sub = v4l2_event_subscription {
        type_: V4L2_EVENT_EOS,
        ..Default::default()
    };
    unsafe { libc::ioctl(fd, VIDIOC_SUBSCRIBE_EVENT as _, &sub) };

    // 5. Stream on OUTPUT
    let mut stream_type = V4L2_BUF_TYPE_VIDEO_OUTPUT_MPLANE;
    if unsafe { libc::ioctl(fd, VIDIOC_STREAMON as _, &mut stream_type) } != 0 {
        bail!(
            "V4L2 VIDIOC_STREAMON(OUTPUT) failed: {}",
            std::io::Error::last_os_error()
        );
    }

    let inner = Arc::new(Mutex::new(Some(V4l2DeviceInner {
        file,
        output_buffers,
        capture_buffers: Vec::new(),
        capture_started: false,
        config: config.clone(),
        timestamp_map: HashMap::new(),
        next_sequence: 0,
        unused_output_indices: (0..req.count).collect(),
        releasing_buffer_indices: VecDeque::new(),
    })));

    let running = Arc::new(RelaxedAtomic::new(true));
    let image_queue = Arc::new(Mutex::new(VecDeque::new()));

    // Spawn background dequeue and event handling thread
    let dequeue_thread = thread::spawn({
        let inner = Arc::clone(&inner);
        let running = Arc::clone(&running);
        let image_queue = Arc::clone(&image_queue);
        let report_frame_decoded = Arc::new(report_frame_decoded);

        move || {
            let mut fds = [libc::pollfd {
                fd,
                events: libc::POLLIN | libc::POLLPRI,
                revents: 0,
            }];

            while running.value() {
                // Buffers are recycled only after the last frame lease is released.
                if let Some(dev) = &mut *inner.lock() {
                    while let Some(index) = dev.releasing_buffer_indices.pop_front() {
                        let mut planes = [v4l2_plane::default(); 8];
                        let mut buf = v4l2_buffer {
                            index,
                            type_: V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE,
                            memory: V4L2_MEMORY_MMAP,
                            length: dev.capture_buffers[index as usize].memory_planes,
                            m: v4l2_buffer_m {
                                planes: planes.as_mut_ptr(),
                            },
                            ..Default::default()
                        };
                        if unsafe { libc::ioctl(fd, VIDIOC_QBUF as _, &mut buf) } != 0 {
                            report_frame_decoded(Err(anyhow!(
                                "V4L2 recycle CAPTURE: {}",
                                std::io::Error::last_os_error()
                            )));
                            running.set(false);
                            break;
                        }
                    }
                }
                let poll_ret = unsafe { libc::poll(fds.as_mut_ptr(), 1, 20) };
                if poll_ret <= 0 {
                    continue;
                }

                let revents = fds[0].revents;

                // Handle events (SOURCE_CHANGE)
                if revents & libc::POLLPRI != 0 {
                    let mut evt = v4l2_event::default();
                    if unsafe { libc::ioctl(fd, VIDIOC_DQEVENT as _, &mut evt) } == 0 {
                        if evt.type_ == V4L2_EVENT_SOURCE_CHANGE {
                            info!("V4L2 SOURCE_CHANGE event received; configuring capture buffers");
                            if let Err(e) = setup_capture_buffers(&inner, fd) {
                                error!("Failed to configure capture buffers: {e}");
                                report_frame_decoded(Err(e));
                                break;
                            }
                        }
                    }
                }

                // Handle decoded frames (CAPTURE DQBUF)
                if revents & libc::POLLIN != 0 {
                    let mut planes = [v4l2_plane::default(); 8];
                    let mut buf = v4l2_buffer {
                        type_: V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE,
                        memory: V4L2_MEMORY_MMAP,
                        length: 8,
                        m: v4l2_buffer_m {
                            planes: planes.as_mut_ptr(),
                        },
                        ..Default::default()
                    };

                    if unsafe { libc::ioctl(fd, VIDIOC_DQBUF as _, &mut buf) } == 0 {
                        let seq = (buf.timestamp.tv_sec as u32) * 1_000_000
                            + (buf.timestamp.tv_usec as u32);
                        let mut inner_lock = inner.lock();
                        if let Some(inner_dev) = &mut *inner_lock {
                            let memory_planes = inner_dev
                                .capture_buffers
                                .get(buf.index as usize)
                                .map(|c| c.memory_planes)
                                .unwrap_or(1);

                            let Some(timestamp) = inner_dev.timestamp_map.remove(&seq) else {
                                warn!("V4L2 returned unknown timestamp {seq}, recycling buffer {}", buf.index);
                                let mut planes = [v4l2_plane::default(); 8];
                                let mut q_buf = v4l2_buffer {
                                    index: buf.index,
                                    type_: V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE,
                                    memory: V4L2_MEMORY_MMAP,
                                    length: memory_planes,
                                    m: v4l2_buffer_m {
                                        planes: planes.as_mut_ptr(),
                                    },
                                    ..Default::default()
                                };
                                unsafe {
                                    libc::ioctl(fd, VIDIOC_QBUF as _, &mut q_buf);
                                }
                                continue;
                            };

                            if buf.flags & 0x40 != 0 {
                                warn!("V4L2 decoded frame marked as corrupt (0x40), recycling buffer {}", buf.index);
                                let mut planes = [v4l2_plane::default(); 8];
                                let mut q_buf = v4l2_buffer {
                                    index: buf.index,
                                    type_: V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE,
                                    memory: V4L2_MEMORY_MMAP,
                                    length: memory_planes,
                                    m: v4l2_buffer_m {
                                        planes: planes.as_mut_ptr(),
                                    },
                                    ..Default::default()
                                };
                                unsafe {
                                    libc::ioctl(fd, VIDIOC_QBUF as _, &mut q_buf);
                                }
                                continue;
                            }

                            report_frame_decoded(Ok(timestamp));

                            if let Some(cap) = inner_dev.capture_buffers.get(buf.index as usize) {
                                let dma_image = DmaBufImage {
                                    buffer_id: cap.id,
                                    width: cap.width,
                                    height: cap.height,
                                    drm_fourcc: DRM_FORMAT_NV12,
                                    modifier: None,
                                    planes: cap.planes.clone(),
                                    color: YuvColor::default(),
                                    lease: Some(Arc::new(DmaBufLease(Box::new({
                                        let inner = Arc::clone(&inner);
                                        let index = buf.index;
                                        move || {
                                            if let Some(dev) = &mut *inner.lock() {
                                                if dev.capture_started {
                                                    dev.releasing_buffer_indices.push_back(index);
                                                }
                                            }
                                        }
                                    })))),
                                };

                                image_queue.lock().push_back(QueuedDmaBufFrame {
                                    timestamp,
                                    image: dma_image,
                                });
                            }
                        }
                    }
                }
            }
        }
    });

    Ok((
        VideoDecoderSink {
            inner: Arc::clone(&inner),
            config_sent: false,
        },
        VideoDecoderSource {
            running,
            dequeue_thread: Some(dequeue_thread),
            image_queue,
            inner,
        },
    ))
}

fn setup_capture_buffers(inner: &Arc<Mutex<Option<V4l2DeviceInner>>>, fd: RawFd) -> Result<()> {
    if inner.lock().as_ref().is_some_and(|d| d.capture_started) {
        bail!("Mid-stream resolution changes require decoder recreation");
    }
    // 1. Query format determined by parser
    let mut fmt = v4l2_format {
        type_: V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE,
        ..Default::default()
    };
    if unsafe { libc::ioctl(fd, VIDIOC_G_FMT as _, &mut fmt) } != 0 {
        bail!(
            "V4L2 VIDIOC_G_FMT(CAPTURE) failed: {}",
            std::io::Error::last_os_error()
        );
    }

    unsafe {
        let pix = &mut fmt.fmt.pix_mp;
        info!(
            "V4L2 parsed format: {}x{}, planes={}",
            pix.width, pix.height, pix.num_planes
        );
        pix.pixelformat = V4L2_PIX_FMT_NV12;
    }
    if unsafe { libc::ioctl(fd, VIDIOC_S_FMT as _, &mut fmt) } != 0 {
        bail!(
            "V4L2 VIDIOC_S_FMT(CAPTURE -> NV12) failed: {}",
            std::io::Error::last_os_error()
        );
    }

    let (width, height, num_planes) = unsafe {
        let pix = &fmt.fmt.pix_mp;
        (pix.width, pix.height, pix.num_planes as usize)
    };
    if unsafe { fmt.fmt.pix_mp.pixelformat } != V4L2_PIX_FMT_NV12 || num_planes != 1 {
        bail!("Expected linear single-memory-plane NV12; received a different capture layout");
    }
    let pitch = unsafe { fmt.fmt.pix_mp.plane_fmt[0].bytesperline };
    if pitch < width {
        bail!("Invalid NV12 pitch {pitch} for width {width}");
    }
    let mut selection = v4l2_selection {
        type_: V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE,
        target: 0,
        ..Default::default()
    };
    if unsafe { libc::ioctl(fd, VIDIOC_G_SELECTION as _, &mut selection) } != 0 {
        bail!(
            "Cannot query the visible NV12 crop: {}",
            std::io::Error::last_os_error()
        );
    }
    let visible = selection.r;
    if visible.left != 0
        || visible.top != 0
        || visible.width == 0
        || visible.height == 0
        || visible.width > width
        || visible.height > height
    {
        bail!("Unsupported NV12 visible rectangle {visible:?}");
    }

    // 2. Request capture buffers (MMAP)
    let num_capture = 20u32;
    let mut req = v4l2_requestbuffers {
        count: num_capture,
        type_: V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE,
        memory: V4L2_MEMORY_MMAP,
        ..Default::default()
    };
    if unsafe { libc::ioctl(fd, VIDIOC_REQBUFS as _, &mut req) } != 0 {
        bail!(
            "V4L2 VIDIOC_REQBUFS(CAPTURE) failed: {}",
            std::io::Error::last_os_error()
        );
    }

    let mut capture_buffers = Vec::new();

    // 3. Export each plane as DMA-BUF fd via VIDIOC_EXPBUF
    for i in 0..req.count {
        let mut planes_vec = Vec::new();
        let mut owned_fds = Vec::new();
        for p in 0..num_planes {
            let mut exp = v4l2_exportbuffer {
                type_: V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE,
                index: i,
                plane: p as u32,
                flags: libc::O_CLOEXEC as u32 | libc::O_RDWR as u32,
                ..Default::default()
            };
            if unsafe { libc::ioctl(fd, VIDIOC_EXPBUF as _, &mut exp) } != 0 {
                bail!(
                    "V4L2 VIDIOC_EXPBUF failed for buffer {i} plane {p}: {}",
                    std::io::Error::last_os_error()
                );
            }

            owned_fds.push(unsafe { OwnedFd::from_raw_fd(exp.fd) });
            planes_vec.push(DmaBufPlane {
                fd: exp.fd,
                offset: 0,
                pitch,
            });
        }
        // V4L2 NV12 has one memory plane containing two DRM image planes.
        planes_vec.push(DmaBufPlane {
            fd: planes_vec[0].fd,
            offset: pitch * height,
            pitch,
        });

        // Initial QBUF for this capture buffer
        let mut planes = [v4l2_plane::default(); 8];
        let mut q_buf = v4l2_buffer {
            index: i,
            type_: V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE,
            memory: V4L2_MEMORY_MMAP,
            length: num_planes as u32,
            m: v4l2_buffer_m {
                planes: planes.as_mut_ptr(),
            },
            ..Default::default()
        };
        if unsafe { libc::ioctl(fd, VIDIOC_QBUF as _, &mut q_buf) } != 0 {
            bail!(
                "V4L2 initial VIDIOC_QBUF(CAPTURE) failed: {}",
                std::io::Error::last_os_error()
            );
        }

        capture_buffers.push(CaptureBuffer {
            planes: planes_vec,
            _fds: owned_fds,
            memory_planes: num_planes as u32,
            id: NEXT_BUFFER_ID.fetch_add(1, Ordering::Relaxed),
            width: visible.width,
            height: visible.height,
        });
    }

    // 4. Stream on CAPTURE
    let mut stream_type = V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE;
    if unsafe { libc::ioctl(fd, VIDIOC_STREAMON as _, &mut stream_type) } != 0 {
        bail!(
            "V4L2 VIDIOC_STREAMON(CAPTURE) failed: {}",
            std::io::Error::last_os_error()
        );
    }

    if let Some(inner_dev) = &mut *inner.lock() {
        inner_dev.capture_buffers = capture_buffers;
        inner_dev.capture_started = true;
    }

    info!(
        "V4L2 capture queue initialized with {} buffers ({}x{})",
        req.count, width, height
    );
    Ok(())
}
