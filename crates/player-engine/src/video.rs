use crate::sched::{DecodedFrame, Packet, PresentOutcome, VideoDecoder, VideoOutput};
use player_media::video::{AV_PIX_FMT_YUVJ420P, AV_PIX_FMT_YUVJ422P, AV_PIX_FMT_YUVJ444P};
use player_media::{
    HwRequest, MediaSession, VideoFrame, VideoSession, AVCOL_RANGE_JPEG,
    AVCOL_SPC_BT2020_NCL, AVCOL_SPC_BT470BG, AVCOL_SPC_BT709, AVCOL_SPC_SMPTE170M,
    AVCOL_SPC_UNSPECIFIED, AV_PIX_FMT_NV12, AV_PIX_FMT_NV21, AV_PIX_FMT_RGB24, AV_PIX_FMT_RGBA,
    AV_PIX_FMT_YUV420P, AV_PIX_FMT_YUV422P, AV_PIX_FMT_YUV444P,
    AV_PIX_FMT_YUV420P10LE, AV_PIX_FMT_YUV422P10LE, AV_PIX_FMT_YUV444P10LE, AV_PIX_FMT_P010LE,
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
    #[cfg(target_os = "macos")]
    surface: Option<player_output::NativeSurface>,
    pub pts: f64,
    pub id: u64,
    pub layout: PixelLayout,
    pub color: ColorSpec,
    pub width: u32,
    pub height: u32,
    pub geometry: player_output::VideoGeometry,
}

impl PresentedFrame {
    pub fn from_frame(frame: VideoFrame, pts: f64, id: u64) -> Result<Self, String> {
        let metadata = frame.geometry();
        let geometry = player_output::VideoGeometry { pixel_aspect: metadata.pixel_aspect, rotation: metadata.rotation };
        #[cfg(target_os = "macos")]
        let native = frame.iosurface();
        #[cfg(target_os = "macos")]
        let surface = native.map(|(raw, _, dimensions)| {
            // SAFETY: the live frame owns the pixel buffer and its IOSurface;
            // dimensions were read from that same buffer.
            unsafe { player_output::NativeSurface::retain(raw, dimensions) }
        });
        #[cfg(target_os = "macos")]
        let frame = if frame.is_hardware() && surface.is_none() { frame.transfer_to_software()? } else { frame };
        #[cfg(not(target_os = "macos"))]
        let frame = if frame.is_hardware() { frame.transfer_to_software()? } else { frame };
        #[cfg(target_os = "macos")]
        let format = native.map(|(_, format, _)| format).unwrap_or_else(|| frame.format());
        #[cfg(not(target_os = "macos"))]
        let format = frame.format();
        let layout = layout_of(format)?;
        let width = frame.width().max(0) as u32;
        let height = frame.height().max(0) as u32;
        let color = color_of(frame.color_space(), frame.color_range(), height as i32);
        Ok(Self { frame, #[cfg(target_os = "macos")] surface, pts, id, layout, color, width, height, geometry })
    }

    #[cfg(target_os = "macos")]
    pub fn surface(&self) -> Option<&player_output::NativeSurface> { self.surface.as_ref() }

    pub fn into_software(self) -> Result<Self, String> {
        if self.frame.is_hardware() {
            Self::from_frame(self.frame.transfer_to_software()?, self.pts, self.id)
        } else {
            Ok(self)
        }
    }

    pub fn with_software_planes<T>(&self, render: impl FnOnce(PixelLayout, &[Plane<'_>]) -> Result<T, String>) -> Result<T, String> {
        if self.frame.is_hardware() {
            let software = self.frame.transfer_to_software()?;
            let layout = layout_of(software.format())?;
            let dimensions = plane_dimensions(layout, software.width(), software.height());
            let mut planes = Vec::with_capacity(dimensions.len());
            for (index, (width, height)) in dimensions.into_iter().enumerate() {
                let (data, stride) = software.plane(index).ok_or_else(|| format!("video plane {index} missing"))?;
                planes.push(Plane { data, stride, width, height });
            }
            render(layout, &planes)
        } else {
            render(self.layout, &self.planes()?)
        }
    }

    pub fn planes(&self) -> Result<Vec<Plane<'_>>, String> {
        if self.frame.is_hardware() { return Err("hardware frame needs software transfer".into()); }
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
    pub fn open(session: &MediaSession, stream: usize, hw: HwRequest) -> Result<(Self, Option<String>), String> {
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
        AV_PIX_FMT_YUV420P | AV_PIX_FMT_YUVJ420P => Ok(PixelLayout::Yuv420),
        AV_PIX_FMT_YUV422P | AV_PIX_FMT_YUVJ422P => Ok(PixelLayout::Yuv422),
        AV_PIX_FMT_YUV444P | AV_PIX_FMT_YUVJ444P => Ok(PixelLayout::Yuv444),
        AV_PIX_FMT_NV12 => Ok(PixelLayout::Nv12),
        AV_PIX_FMT_P010LE => Ok(PixelLayout::P010),
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
        PixelLayout::Nv12 | PixelLayout::Nv21 | PixelLayout::P010 => vec![(width, height), (half_width, half_height)],
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
                        geometry: player_output::VideoGeometry::default(),
                        layout: layout_of(frame.format()).expect("layout"),
                        color: color_of(frame.color_space(), frame.color_range(), frame.height()),
                        width: frame.width() as u32,
                        height: frame.height() as u32,
                        frame,
                        #[cfg(target_os = "macos")]
                        surface: None,
                        pts: 0.0,
                        id: 0,
                    };
                }
                player_media::PumpResult::NeedMore => {}
                player_media::PumpResult::Ended => panic!("the fixture decoded no frame"),
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn native_surface_matches_transfer(path: &str, expected_layout: PixelLayout) {
        use player_media::PumpResult;
        use player_output::{Gpu, render_frame_with_geometry, render_surface_to_target};

        fn next(decoder: &mut VideoSession, id: u64) -> PresentedFrame {
            loop {
                match decoder.pump().expect("decode") {
                    PumpResult::Frame(frame) => return PresentedFrame::from_frame(frame, 0.0, id).unwrap(),
                    PumpResult::NeedMore => (),
                    PumpResult::Ended => panic!("fixture has too few frames"),
                }
            }
        }
        let media = MediaSession::open(path).expect("fixture opens");
        let stream = media.streams().into_iter().find(|s| s.kind == StreamKind::Video).unwrap();
        drop(media);
        let gpu = Gpu::metal().expect("Metal device");
        let renderer = gpu.create_renderer().unwrap();
        let mut decoder = VideoSession::open(path, stream.index, HwRequest::VideoToolbox).expect("VideoToolbox decoder");
        let mut expected = Vec::new();
        let mut retained_frame = None;
        for id in 0..12 {
            let frame = next(&mut decoder, id);
            assert_eq!(frame.layout, expected_layout);
            assert!(frame.surface().is_some(), "hardware IOSurface required");
            let size = (frame.width as i32, frame.height as i32);
            let image = frame.with_software_planes(|layout, planes| {
                render_frame_with_geometry(&gpu, &renderer, layout, frame.color, planes,
                    size.0, size.1, frame.geometry, None)
            }).expect("software reference");
            expected.push(image);
            if id == 0 { retained_frame = Some(frame); }
        }
        drop(decoder);
        let frame = retained_frame.take().unwrap();
        let size = (frame.width as i32, frame.height as i32);
        let image = player_output::render_surface_frame(&gpu, &renderer, frame.surface().unwrap(),
            frame.layout, frame.color, size, size, frame.geometry).unwrap().expect("direct import");
        let worst = image.iter().zip(&expected[0]).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
        assert!(worst <= 2, "frame after decoder shutdown differs by {worst}");
        if expected_layout == PixelLayout::Nv12 {
            let vulkan = Gpu::vulkan().expect("Vulkan device");
            let vk_renderer = vulkan.create_renderer().unwrap();
            assert!(player_output::render_surface_frame(&vulkan, &vk_renderer, frame.surface().unwrap(),
                frame.layout, frame.color, size, size, frame.geometry).unwrap().is_none());
            let fallback = frame.with_software_planes(|layout, planes| {
                render_frame_with_geometry(&vulkan, &vk_renderer, layout, frame.color, planes,
                    size.0, size.1, frame.geometry, None)
            }).expect("Vulkan software fallback");
            assert_eq!(fallback.len(), expected[0].len());
            let worst = fallback.iter().zip(&expected[0]).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
            assert!(worst <= 4, "Vulkan fallback differs by {worst}");
        }
        let fallback_format = gpu.find_rgba8(1 << 3).unwrap();
        let fallback_target = gpu.create_render_target(size.0, size.1, fallback_format).unwrap();
        assert!(!render_surface_to_target(&gpu, &renderer, frame.surface().unwrap(),
            PixelLayout::Rgba8, frame.color, size, fallback_target, size, false,
            frame.geometry, None).unwrap(), "unsupported native layout requests fallback");
        frame.with_software_planes(|layout, planes| {
            player_output::render_to_target_with_geometry(&gpu, &renderer, layout,
                frame.color, planes, fallback_target, size.0, size.1, false,
                frame.geometry, None)
        }).expect("fallback renders the same target");
        let fallback_pixels = gpu.download(fallback_target, size.0, size.1).unwrap();
        let worst = fallback_pixels.iter().zip(&expected[0]).map(|(a,b)| a.abs_diff(*b)).max().unwrap();
        assert!(worst <= 2, "same-target fallback differs by {worst}");
        gpu.destroy_tex(fallback_target);
        let copied = frame.into_software().expect("explicit copy mode");
        assert!(copied.surface().is_none());
        assert!(!copied.planes().unwrap().is_empty());
        drop(copied);
        let mut decoder = VideoSession::open(path, stream.index, HwRequest::VideoToolbox).unwrap();
        let format = gpu.find_rgba8(1 << 3).unwrap();
        let mut targets = Vec::new();
        for index in 0..24 {
            if index == 12 { decoder.seek_seconds(0.0).expect("seek"); }
            let frame = next(&mut decoder, index);
            assert_eq!(frame.layout, expected_layout);
            let size = (frame.width as i32, frame.height as i32);
            let target = gpu.create_render_target(size.0, size.1, format).unwrap();
            assert!(render_surface_to_target(&gpu, &renderer, frame.surface().expect("native surface"),
                frame.layout, frame.color, size, target, size, false, frame.geometry, None).unwrap(),
                "direct import required");
            targets.push((target, size, index as usize % 12));
            drop(frame);
        }
        drop(decoder);
        for (target, (width, height), index) in targets {
            let actual = gpu.download(target, width, height).expect("native readback");
            let reference = &expected[index];
            assert_eq!(actual.len(), reference.len());
            let worst = actual.iter().zip(reference).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
            assert!(worst <= 2, "frame {index} worst channel difference {worst}");
            gpu.destroy_tex(target);
        }
        let mut decoder = VideoSession::open(path, stream.index, HwRequest::VideoToolbox).unwrap();
        let frame = next(&mut decoder, 0);
        let size = (frame.width as i32, frame.height as i32);
        let target = gpu.create_render_target(size.0, size.1, format).unwrap();
        assert!(render_surface_to_target(&gpu, &renderer, frame.surface().unwrap(), frame.layout,
            frame.color, size, target, size, false, frame.geometry, None).unwrap());
        drop(frame);
        drop(decoder);
        gpu.destroy_tex(target);
        drop(renderer);
        drop(gpu);
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "requires real VideoToolbox and Metal"]
    fn videotoolbox_nv12_import_matches_transfer_through_seek_and_shutdown() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.work/fixtures/h264.mp4");
        native_surface_matches_transfer(path.to_str().unwrap(), PixelLayout::Nv12);
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "requires real VideoToolbox, Metal and a HEVC Main10 fixture"]
    fn videotoolbox_p010_import_matches_transfer_through_seek_and_shutdown() {
        let path = std::env::var("PLAYER_TEST_MAIN10").expect("PLAYER_TEST_MAIN10 must name a HEVC Main10 fixture with at least 12 frames");
        native_surface_matches_transfer(&path, PixelLayout::P010);
    }

    #[test]
    fn presentation_keeps_coded_planes_and_display_geometry() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../player-media/tests/fixtures/geometry-rotated.mp4");
        let path = path.to_str().unwrap();
        let session = MediaSession::open(path).unwrap();
        let stream = session.streams().into_iter().find(|s| s.kind == StreamKind::Video).unwrap();
        let mut decoder = VideoSession::open(path, stream.index, HwRequest::None).unwrap();
        loop {
            match decoder.pump().unwrap() {
                player_media::PumpResult::Frame(frame) => {
                    let presented = PresentedFrame::from_frame(frame, 0.0, 0).unwrap();
                    assert_eq!((presented.width, presented.height), (64, 32));
                    assert_eq!(presented.geometry.display_size(presented.width, presented.height), (32, 128));
                    assert_eq!(presented.geometry.rotation, 270);
                    assert_eq!((presented.planes().unwrap()[0].width, presented.planes().unwrap()[0].height), (64, 32));
                    break;
                }
                player_media::PumpResult::NeedMore => (),
                player_media::PumpResult::Ended => panic!("no decoded frame"),
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
