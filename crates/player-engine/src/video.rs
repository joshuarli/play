use crate::sched::{DecodedFrame, Packet, PresentOutcome, VideoDecoder, VideoOutput};
use player_media::{
    HwRequest, MediaSession, VideoFrame, VideoSession, AVCOL_RANGE_JPEG,
    AVCOL_SPC_BT2020_NCL, AVCOL_SPC_BT470BG, AVCOL_SPC_BT709, AVCOL_SPC_SMPTE170M,
    AVCOL_SPC_UNSPECIFIED, AV_PIX_FMT_NV12, AV_PIX_FMT_NV21, AV_PIX_FMT_RGB24, AV_PIX_FMT_RGBA,
    AV_PIX_FMT_YUV420P, AV_PIX_FMT_YUV422P, AV_PIX_FMT_YUV444P,
    AV_PIX_FMT_YUV420P10LE, AV_PIX_FMT_YUV422P10LE, AV_PIX_FMT_YUV444P10LE,
};
use player_output::{
    ColorSpec, PixelLayout, Plane, PL_COLOR_LEVELS_FULL,
    PL_COLOR_LEVELS_LIMITED, PL_COLOR_SYSTEM_BT_2020_NC, PL_COLOR_SYSTEM_BT_601,
    PL_COLOR_SYSTEM_BT_709,
};
use std::collections::VecDeque;
use std::sync::{mpsc, Arc, Mutex};

const NO_TIMESTAMP: i64 = i64::MIN;
const MAX_QUEUED_FRAMES: usize = 64;

#[derive(Debug)]
pub struct PresentedFrame {
    frame: VideoFrame,
    pub pts: f64,
    pub id: u64,
    pub layout: PixelLayout,
    pub color: ColorSpec,
    pub width: u32,
    pub height: u32,
}

impl PresentedFrame {
    pub fn from_frame(frame: VideoFrame, pts: f64, id: u64) -> Result<Self, String> {
        let frame = if frame.is_hardware() { frame.transfer_to_software()? } else { frame };
        let layout = layout_of(frame.format())?;
        let width = frame.width().max(0) as u32;
        let height = frame.height().max(0) as u32;
        let color = color_of(frame.color_space(), frame.color_range(), height as i32);
        Ok(Self { frame, pts, id, layout, color, width, height })
    }

    pub fn planes(&self) -> Result<Vec<Plane<'_>>, String> {
        let dimensions = plane_dimensions(self.layout, self.frame.width(), self.frame.height());
        let mut planes = Vec::with_capacity(dimensions.len());
        for (index, (width, height)) in dimensions.iter().enumerate() {
            let (data, stride) = self
                .frame
                .plane(index)
                .ok_or_else(|| format!("video plane {index} missing"))?;
            planes.push(Plane {
                data,
                stride,
                width: *width,
                height: *height,
            });
        }
        Ok(planes)
    }
}

pub(crate) struct SharedVideo {
    session: VideoSession,
    queue: VecDeque<(u64, f64, VideoFrame)>,
    next_id: u64,
    drained: u64,
    decoded: u64,
    evicted: u64,
}

pub(crate) type SharedHandle = Arc<Mutex<Option<SharedVideo>>>;

pub struct EngineVideo {
    shared: SharedHandle,
}

impl EngineVideo {
    pub fn open(session: &MediaSession, stream: usize) -> Result<Self, String> {
        Self::open_with_hw(session, stream, HwRequest::None).map(|(video, _)| video)
    }

    pub fn open_with_hw(session: &MediaSession, stream: usize, hw: HwRequest) -> Result<(Self, Option<String>), String> {
        let (decoder, fallback) = match VideoSession::open_shared(session, stream, hw) {
            Ok(decoder) => (decoder, None),
            Err(error) if hw != HwRequest::None => {
                (VideoSession::open_shared(session, stream, HwRequest::None)?, Some(error))
            }
            Err(error) => return Err(error),
        };
        Ok((EngineVideo {
            shared: Arc::new(Mutex::new(Some(SharedVideo {
                session: decoder,
                queue: VecDeque::new(),
                next_id: 0,
                drained: 0,
                decoded: 0,
                evicted: 0,
            }))),
        }, fallback))
    }

    pub fn active_hardware(&self) -> Option<HwRequest> {
        self.shared.lock().ok()?.as_ref().map(|shared| shared.session.active_hardware())
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
    shared.decoded += 1;
    shared.queue.push_back((id, pts, frame));
    while shared.queue.len() > MAX_QUEUED_FRAMES {
        let (id, _, _) = shared.queue.pop_front().expect("over capacity");
        if id >= shared.drained {
            shared.evicted += 1;
        }
    }
}

impl SharedVideo {
    fn undrained(&self) -> u64 {
        self.queue.iter().filter(|(id, _, _)| *id >= self.drained).count() as u64
    }

    fn release(&mut self, id: u64) {
        if let Some(position) = self.queue.iter().position(|(queued, _, _)| *queued == id) {
            self.queue.remove(position);
        }
    }

    fn take(&mut self, id: u64) -> Option<VideoFrame> {
        let position = self.queue.iter().position(|(queued, _, _)| *queued == id)?;
        Some(self.queue.remove(position).expect("found").2)
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
                shared.drained = shared.next_id;
            }
        }
    }

    fn release(&mut self, id: u64) {
        if let Ok(mut guard) = self.shared.lock() {
            if let Some(shared) = guard.as_mut() {
                shared.release(id);
            }
        }
    }

    fn decoded(&self) -> u64 {
        self.shared
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(|shared| shared.decoded))
            .unwrap_or(0)
    }

    fn evicted(&self) -> u64 {
        self.shared
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(|shared| shared.evicted))
            .unwrap_or(0)
    }

    fn undrained(&self) -> u64 {
        self.shared
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(SharedVideo::undrained))
            .unwrap_or(0)
    }
}

pub struct EngineVideoOutput {
    shared: SharedHandle,
    discard: bool,
    sink: mpsc::SyncSender<PresentedFrame>,
}

impl EngineVideoOutput {
    pub fn open(shared: SharedHandle, sink: mpsc::SyncSender<PresentedFrame>) -> Self {
        EngineVideoOutput {
            shared,
            discard: false,
            sink,
        }
    }

    pub fn passive(shared: SharedHandle, sink: mpsc::SyncSender<PresentedFrame>) -> Self {
        EngineVideoOutput {
            shared,
            discard: false,
            sink,
        }
    }

    pub fn null(shared: SharedHandle, sink: mpsc::SyncSender<PresentedFrame>) -> Self {
        EngineVideoOutput {
            shared,
            discard: true,
            sink,
        }
    }

    fn take_frame(&self, id: u64) -> Option<VideoFrame> {
        let mut guard = self.shared.lock().ok()?;
        let shared = guard.as_mut()?;
        shared.take(id)
    }
}

fn layout_of(format: i32) -> Result<PixelLayout, String> {
    match format {
        AV_PIX_FMT_YUV420P10LE => Ok(PixelLayout::Yuv420p10),
        AV_PIX_FMT_YUV422P10LE => Ok(PixelLayout::Yuv422p10),
        AV_PIX_FMT_YUV444P10LE => Ok(PixelLayout::Yuv444p10),
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
        PixelLayout::Yuv420 | PixelLayout::Yuv420p10 => vec![(width, height), (half_width, half_height), (half_width, half_height)],
        PixelLayout::Yuv422 | PixelLayout::Yuv422p10 => vec![(width, height), (half_width, height), (half_width, height)],
        PixelLayout::Yuv444 | PixelLayout::Yuv444p10 => vec![(width, height), (width, height), (width, height)],
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
    fn present(&mut self, pts: f64, id: u64) -> Result<PresentOutcome, String> {
        let frame = match self.take_frame(id) {
            Some(frame) => frame,
            None => return Ok(PresentOutcome::Missing),
        };
        if self.discard {
            drop(frame);
            return Ok(PresentOutcome::Discarded);
        }
        let presented = PresentedFrame::from_frame(frame, pts, id)?;
        let sent = self.sink.send(presented).is_ok();
        Ok(if sent {
            PresentOutcome::Delivered
        } else {
            PresentOutcome::Discarded
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use player_media::{MediaSession, StreamKind};

    fn first_frame() -> PresentedFrame {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("workspace root");
        let path = root.join(".work/fixtures/h264.mp4");
        assert!(path.is_file(), "run tests/fixtures/gen.sh first");
        let path = path.to_string_lossy().into_owned();
        let session = MediaSession::open(&path).expect("fixture opens");
        let stream = session
            .streams()
            .into_iter()
            .find(|stream| stream.kind == StreamKind::Video)
            .expect("video stream");
        let mut decoder =
            VideoSession::open(&path, stream.index, HwRequest::None).expect("decoder opens");
        loop {
            match decoder.pump().expect("the fixture decodes") {
                player_media::PumpResult::Frame(frame) => {
                    assert_eq!(frame.format(), AV_PIX_FMT_YUV420P);
                    return PresentedFrame {
                        layout: layout_of(frame.format()).expect("layout"),
                        color: color_of(frame.color_space(), frame.color_range(), frame.height()),
                        width: frame.width() as u32,
                        height: frame.height() as u32,
                        frame,
                        pts: 0.0,
                        id: 0,
                    };
                }
                player_media::PumpResult::NeedMore => {}
                player_media::PumpResult::Ended => panic!("the fixture decoded no frame"),
            }
        }
    }

    #[test]
    fn ten_bit_layouts_keep_chroma_dimensions() {
        for (format, layout, chroma) in [
            (AV_PIX_FMT_YUV420P10LE, PixelLayout::Yuv420p10, (3, 2)),
            (AV_PIX_FMT_YUV422P10LE, PixelLayout::Yuv422p10, (3, 3)),
            (AV_PIX_FMT_YUV444P10LE, PixelLayout::Yuv444p10, (5, 3)),
        ] {
            assert_eq!(layout_of(format).unwrap(), layout);
            assert_eq!(plane_dimensions(layout, 5, 3), vec![(5, 3), chroma, chroma]);
        }
    }

    #[test]
    fn yuv420p_planes_carry_the_expected_dimensions_and_rows() {
        let presented = first_frame();
        let planes = presented.planes().expect("planes derive");
        assert_eq!((presented.width, presented.height), (320, 240));
        assert_eq!(presented.layout, PixelLayout::Yuv420);
        let dimensions: Vec<_> = planes
            .iter()
            .map(|plane| (plane.width, plane.height))
            .collect();
        assert_eq!(dimensions, vec![(320, 240), (160, 120), (160, 120)]);
        for plane in &planes {
            assert!(
                plane.stride >= plane.width as usize,
                "a stride holds its row: {} < {}",
                plane.stride,
                plane.width
            );
            assert_eq!(
                plane.data.len(),
                plane.stride * plane.height as usize,
                "a plane covers its stride times its height"
            );
        }
        assert!(planes[0].data.iter().any(|byte| *byte != 0), "luma has picture");
    }
}
