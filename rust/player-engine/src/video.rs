use crate::sched::{DecodedFrame, Packet, VideoDecoder, VideoOutput};
use player_media::{
    HwRequest, MediaSession, VideoFrame, VideoSession, AVCOL_RANGE_JPEG,
    AVCOL_SPC_BT2020_NCL, AVCOL_SPC_BT470BG, AVCOL_SPC_BT709, AVCOL_SPC_SMPTE170M,
    AVCOL_SPC_UNSPECIFIED, AV_PIX_FMT_NV12, AV_PIX_FMT_NV21, AV_PIX_FMT_RGB24, AV_PIX_FMT_RGBA,
    AV_PIX_FMT_YUV420P, AV_PIX_FMT_YUV422P, AV_PIX_FMT_YUV444P,
};
use player_output::{
    render_frame, ColorSpec, PixelLayout, Plane, Renderer, Gpu, PL_COLOR_LEVELS_FULL,
    PL_COLOR_LEVELS_LIMITED, PL_COLOR_SYSTEM_BT_2020_NC, PL_COLOR_SYSTEM_BT_601,
    PL_COLOR_SYSTEM_BT_709,
};
use std::collections::VecDeque;
use std::sync::{mpsc, Arc, Mutex};

const NO_TIMESTAMP: i64 = i64::MIN;
const MAX_QUEUED_FRAMES: usize = 64;

#[derive(Debug, Clone)]
pub struct PresentedFrame {
    pub pts: f64,
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub(crate) struct SharedVideo {
    session: VideoSession,
    queue: VecDeque<(u64, f64, VideoFrame)>,
    next_id: u64,
    drained: u64,
}

pub(crate) type SharedHandle = Arc<Mutex<Option<SharedVideo>>>;

pub struct EngineVideo {
    shared: SharedHandle,
}

impl EngineVideo {
    pub fn open(session: &MediaSession, stream: usize) -> Result<Self, String> {
        let decoder = VideoSession::open_shared(session, stream, HwRequest::None)?;
        Ok(EngineVideo {
            shared: Arc::new(Mutex::new(Some(SharedVideo {
                session: decoder,
                queue: VecDeque::new(),
                next_id: 0,
                drained: 0,
            }))),
        })
    }

    pub fn passive() -> Self {
        EngineVideo {
            shared: Arc::new(Mutex::new(None)),
        }
    }

    pub(crate) fn shared(&self) -> SharedHandle {
        self.shared.clone()
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Option<SharedVideo>>, String> {
        self.shared
            .lock()
            .map_err(|_| "video decoder lock poisoned".to_string())
    }
}

fn frame_seconds(frame: &VideoFrame, time_base: (i32, i32), fallback: f64) -> f64 {
    let ticks = frame.best_effort_ticks();
    if ticks == NO_TIMESTAMP {
        return fallback;
    }
    let (num, den) = time_base;
    if den == 0 {
        return fallback;
    }
    ticks as f64 * num as f64 / den as f64
}

fn push_frame(shared: &mut SharedVideo, frame: VideoFrame, pts: f64) {
    let id = shared.next_id;
    shared.next_id += 1;
    shared.queue.push_back((id, pts, frame));
    while shared.queue.len() > MAX_QUEUED_FRAMES {
        shared.queue.pop_front();
    }
}

fn harvest(shared: &mut SharedVideo, fallback: f64) -> Result<(), String> {
    let time_base = shared.session.time_base();
    while let Some(frame) = shared.session.receive()? {
        let pts = frame_seconds(&frame, time_base, fallback);
        push_frame(shared, frame, pts);
    }
    Ok(())
}

fn collect_new(shared: &SharedVideo) -> Vec<DecodedFrame> {
    shared
        .queue
        .iter()
        .filter(|(id, _, _)| *id >= shared.drained)
        .map(|(id, pts, _)| DecodedFrame { pts: *pts, id: *id })
        .collect()
}

impl VideoDecoder for EngineVideo {
    fn send(&mut self, packet: Packet) -> Result<(), String> {
        let mut guard = self.lock()?;
        let Some(shared) = guard.as_mut() else {
            return Err("video packet with no video stream".to_string());
        };
        let fallback = packet.pts.unwrap_or(0.0);
        let mut attempts = 0;
        while !shared.session.send(&packet.payload, packet.pts)? {
            match shared.session.receive()? {
                Some(frame) => {
                    let time_base = shared.session.time_base();
                    let pts = frame_seconds(&frame, time_base, fallback);
                    push_frame(shared, frame, pts);
                }
                None => return Err("video decoder stalled".to_string()),
            }
            attempts += 1;
            if attempts > MAX_QUEUED_FRAMES {
                return Err("video decoder stalled".to_string());
            }
        }
        harvest(shared, fallback)?;
        Ok(())
    }

    fn drain(&mut self) -> Result<Vec<DecodedFrame>, String> {
        let mut guard = self.lock()?;
        let Some(shared) = guard.as_mut() else {
            return Ok(Vec::new());
        };
        harvest(shared, 0.0)?;
        let out = collect_new(shared);
        shared.drained = shared.next_id;
        Ok(out)
    }

    fn finish(&mut self) -> Result<Vec<DecodedFrame>, String> {
        let mut guard = self.lock()?;
        let Some(shared) = guard.as_mut() else {
            return Ok(Vec::new());
        };
        shared.session.drain()?;
        harvest(shared, 0.0)?;
        let out = collect_new(shared);
        shared.drained = shared.next_id;
        Ok(out)
    }

    fn flush(&mut self) {
        if let Ok(mut guard) = self.shared.lock() {
            if let Some(shared) = guard.as_mut() {
                shared.session.reset();
                shared.queue.clear();
            }
        }
    }
}

pub struct EngineVideoOutput {
    shared: SharedHandle,
    gpu: Option<GpuRenderer>,
    discard: bool,
    sink: mpsc::SyncSender<PresentedFrame>,
}

struct GpuRenderer {
    // Field order is the drop order: the renderer must be destroyed before
    // the GPU device that owns it.
    renderer: Renderer,
    device: Gpu,
}

impl EngineVideoOutput {
    pub fn open(shared: SharedHandle, sink: mpsc::SyncSender<PresentedFrame>) -> Result<Self, String> {
        let device = Gpu::vulkan()?;
        let renderer = device.create_renderer()?;
        Ok(EngineVideoOutput {
            shared,
            gpu: Some(GpuRenderer { renderer, device }),
            discard: false,
            sink,
        })
    }

    pub fn passive(shared: SharedHandle, sink: mpsc::SyncSender<PresentedFrame>) -> Self {
        EngineVideoOutput {
            shared,
            gpu: None,
            discard: false,
            sink,
        }
    }

    pub fn null(shared: SharedHandle, sink: mpsc::SyncSender<PresentedFrame>) -> Self {
        EngineVideoOutput {
            shared,
            gpu: None,
            discard: true,
            sink,
        }
    }

    fn take_frame(&self, id: u64) -> Option<VideoFrame> {
        let mut guard = self.shared.lock().ok()?;
        let shared = guard.as_mut()?;
        let position = shared
            .queue
            .iter()
            .position(|(queued, _, _)| *queued == id)?;
        Some(shared.queue.remove(position).expect("found").2)
    }
}

fn layout_of(format: i32) -> Result<PixelLayout, String> {
    match format {
        AV_PIX_FMT_YUV420P => Ok(PixelLayout::Yuv420),
        AV_PIX_FMT_YUV422P => Ok(PixelLayout::Yuv422),
        AV_PIX_FMT_YUV444P => Ok(PixelLayout::Yuv444),
        AV_PIX_FMT_NV12 => Ok(PixelLayout::Nv12),
        AV_PIX_FMT_NV21 => Ok(PixelLayout::Nv21),
        AV_PIX_FMT_RGB24 => Ok(PixelLayout::Rgb24),
        AV_PIX_FMT_RGBA => Ok(PixelLayout::Rgba8),
        _ => Err(format!("unsupported pixel format {format}")),
    }
}

fn plane_dimensions(layout: PixelLayout, width: i32, height: i32) -> Vec<(i32, i32)> {
    let half_width = (width + 1) / 2;
    let half_height = (height + 1) / 2;
    match layout {
        PixelLayout::Yuv420 => vec![(width, height), (half_width, half_height), (half_width, half_height)],
        PixelLayout::Yuv422 => vec![(width, height), (half_width, height), (half_width, height)],
        PixelLayout::Yuv444 => vec![(width, height), (width, height), (width, height)],
        PixelLayout::Nv12 | PixelLayout::Nv21 => vec![(width, height), (half_width, half_height)],
        PixelLayout::Rgb24 | PixelLayout::Rgba8 => vec![(width, height)],
    }
}

fn color_of(space: i32, range: i32, height: i32) -> ColorSpec {
    let system = if space == AVCOL_SPC_BT709 {
        PL_COLOR_SYSTEM_BT_709
    } else if space == AVCOL_SPC_BT470BG || space == AVCOL_SPC_SMPTE170M {
        PL_COLOR_SYSTEM_BT_601
    } else if space == AVCOL_SPC_BT2020_NCL {
        PL_COLOR_SYSTEM_BT_2020_NC
    } else if space != AVCOL_SPC_UNSPECIFIED {
        PL_COLOR_SYSTEM_BT_709
    } else if height > 576 {
        PL_COLOR_SYSTEM_BT_709
    } else {
        PL_COLOR_SYSTEM_BT_601
    };
    let levels = if range == AVCOL_RANGE_JPEG {
        PL_COLOR_LEVELS_FULL
    } else {
        PL_COLOR_LEVELS_LIMITED
    };
    ColorSpec { system, levels }
}

impl VideoOutput for EngineVideoOutput {
    fn present(&mut self, pts: f64, id: u64) -> Result<(), String> {
        let frame = match self.take_frame(id) {
            Some(frame) => frame,
            None => return Ok(()),
        };
        if self.discard {
            drop(frame);
            return Ok(());
        }
        let gpu = self
            .gpu
            .as_ref()
            .ok_or_else(|| "present with no renderer".to_string())?;
        let mut frame = frame;
        if frame.is_hardware() {
            frame = frame.transfer_to_software()?;
        }
        let layout = layout_of(frame.format())?;
        let width = frame.width();
        let height = frame.height();
        let color = color_of(frame.color_space(), frame.color_range(), height);
        let dimensions = plane_dimensions(layout, width, height);
        let mut planes = Vec::with_capacity(dimensions.len());
        for (index, (w, h)) in dimensions.iter().enumerate() {
            let (data, stride) = frame
                .plane(index)
                .ok_or_else(|| format!("video plane {index} missing"))?;
            planes.push(Plane {
                data,
                stride,
                width: *w,
                height: *h,
            });
        }
        let rgba = render_frame(
            &gpu.device, &gpu.renderer, layout, color, &planes, width, height, None,
        )?;
        let _ = self.sink.send(PresentedFrame {
            pts,
            id,
            width: width.max(0) as u32,
            height: height.max(0) as u32,
            rgba,
        });
        Ok(())
    }
}
