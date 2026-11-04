use crate::osd::{composite, composite_straight, AssOverlay, OsdState};
use player_ass::RenderedImage;
use player_engine::{video::PresentedFrame, EngineStatus, SchedulerEngine, TrackKind};
use std::time::{Duration, Instant};

pub const DEFAULT_OSD_DURATION: Duration = Duration::from_millis(1000);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayImage {
    pub rgba: Vec<u8>,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

pub struct EngineBackend {
    engine: SchedulerEngine,
    sub_visible: bool,
    message: Option<OsdMessage>,
    osd: Option<OsdState>,
}

struct OsdMessage {
    text: String,
    until: Instant,
}

impl EngineBackend {
    pub fn open(engine: SchedulerEngine) -> Self {
        EngineBackend {
            engine,
            sub_visible: true,
            message: None,
            osd: None,
        }
    }

    pub fn status(&self) -> EngineStatus {
        self.engine.snapshot()
    }

    pub fn set_paused(&self, paused: bool) {
        self.engine.set_paused(paused);
    }

    pub fn seek_to(&self, seconds: f64) {
        self.engine.seek(seconds);
    }

    pub fn set_speed(&self, speed: f64) {
        self.engine.set_speed(speed);
    }

    pub fn set_volume_percent(&self, volume: f64) {
        self.engine.set_volume(volume as f32 / 100.0);
    }

    pub fn set_muted(&self, muted: bool) {
        self.engine.set_muted(muted);
    }

    pub fn set_sub_visible(&mut self, visible: bool) {
        self.sub_visible = visible;
        self.engine.set_sub_visible(visible);
    }

    pub fn sub_visible(&self) -> bool {
        self.sub_visible
    }

    pub fn add_sub(&self, path: &str) -> Result<(), String> {
        self.engine.add_sub(path)
    }

    pub fn select_sub_position(&self, position: usize) {
        self.engine.select_sub_position(position);
    }

    pub fn deselect_sub(&self) {
        self.engine.select_sub(None);
    }

    pub fn select_first_sub(&self) {
        let first = self
            .engine
            .snapshot()
            .tracks
            .iter()
            .filter(|track| track.kind == TrackKind::Subtitle)
            .find_map(|track| track.sub_index);
        self.engine.select_sub(first);
    }

    pub fn select_sub_id(&self, id: i64) -> Result<(), String> {
        let tracks = self.engine.snapshot().tracks;
        let track = tracks
            .iter()
            .find(|track| track.kind == TrackKind::Subtitle && track.id as i64 == id)
            .ok_or_else(|| format!("no subtitle track {id}"))?;
        let index = track
            .sub_index
            .ok_or_else(|| format!("subtitle track {id} uses an unsupported codec"))?;
        self.engine.select_sub(Some(index));
        Ok(())
    }

    pub fn select_audio_id(&self, id: i64) -> Result<(), String> {
        let tracks = self.engine.snapshot().tracks;
        let track = tracks
            .iter()
            .find(|track| track.kind == TrackKind::Audio && track.id as i64 == id)
            .ok_or_else(|| format!("no audio track {id}"))?;
        self.engine.select_audio(track.src_id.saturating_sub(1));
        Ok(())
    }

    pub fn show_text(&mut self, text: &str, duration: Duration) {
        self.message = Some(OsdMessage {
            text: text.to_owned(),
            until: Instant::now() + duration,
        });
    }

    fn visible_message(&mut self, now: Instant) -> Option<&str> {
        if self.message.as_ref().is_some_and(|message| now >= message.until) {
            self.message = None;
        }
        self.message.as_ref().map(|message| message.text.as_str())
    }

    pub fn has_message(&mut self) -> bool {
        self.visible_message(Instant::now()).is_some()
    }

    pub fn subtitles_selected(&self) -> bool {
        subtitles_selected(&self.engine.snapshot())
    }

    fn render_images(
        &mut self,
        width: u32,
        height: u32,
        osc: Option<&AssOverlay>,
    ) -> Result<Vec<RenderedImage>, String> {
        let status = self.engine.snapshot();
        let mut images = Vec::new();
        if subtitles_selected(&status) {
            let now_ms = (status.position * 1000.0).round() as i64;
            images.extend(self.engine.render_subtitles(
                width as i32,
                height as i32,
                1.0,
                now_ms,
            )?);
        }
        let message = self.visible_message(Instant::now()).map(str::to_owned);
        if osc.is_none() && message.is_none() {
            return Ok(images);
        }
        if self.osd.is_none() {
            self.osd = Some(OsdState::new()?);
        }
        let osd = self.osd.as_mut().expect("overlay renderer starts");
        images.extend(osd.render(osc, message.as_deref(), width as i64, height as i64)?);
        Ok(images)
    }

    pub fn compose(
        &mut self,
        frame: &mut [u8],
        width: u32,
        height: u32,
        osc: Option<&AssOverlay>,
    ) -> Result<(), String> {
        let images = self.render_images(width, height, osc)?;
        composite(frame, width, height, &images)
    }

    pub fn render_overlay(
        &mut self,
        width: u32,
        height: u32,
        osc: Option<&AssOverlay>,
    ) -> Result<Option<OverlayImage>, String> {
        let mut images = self.render_images(width, height, osc)?;
        let Some((x, y, crop_width, crop_height)) = inked_bounds(&images, width, height) else {
            return Ok(None);
        };
        for image in &mut images {
            image.dst_x -= x as i32;
            image.dst_y -= y as i32;
        }
        let mut rgba = vec![0u8; crop_width as usize * crop_height as usize * 4];
        composite_straight(&mut rgba, crop_width, crop_height, &images)?;
        Ok(Some(OverlayImage {
            rgba,
            x,
            y,
            width: crop_width,
            height: crop_height,
        }))
    }

    pub fn latest_frame(&self) -> Option<PresentedFrame> {
        self.engine.drain_frames().pop()
    }
}

fn inked_bounds(images: &[RenderedImage], width: u32, height: u32) -> Option<(u32, u32, u32, u32)> {
    let mut bounds: Option<(i32, i32, i32, i32)> = None;
    for image in images.iter().filter(|image| image.width > 0 && image.height > 0) {
        let x0 = image.dst_x.max(0);
        let y0 = image.dst_y.max(0);
        let x1 = (image.dst_x + image.width as i32).min(width as i32);
        let y1 = (image.dst_y + image.height as i32).min(height as i32);
        if x0 >= x1 || y0 >= y1 {
            continue;
        }
        bounds = Some(match bounds {
            None => (x0, y0, x1, y1),
            Some((a, b, c, d)) => (a.min(x0), b.min(y0), c.max(x1), d.max(y1)),
        });
    }
    bounds.map(|(x0, y0, x1, y1)| (x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32))
}

fn subtitles_selected(status: &EngineStatus) -> bool {
    status
        .tracks
        .iter()
        .any(|track| track.kind == TrackKind::Subtitle && track.selected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use player_audio::sink::SinkKind;

    fn fixture(name: &str) -> String {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("workspace root")
            .join(".work/fixtures")
            .join(name)
            .to_string_lossy()
            .into_owned()
    }

    fn backend() -> EngineBackend {
        let engine =
            SchedulerEngine::load(&fixture("audio-wav.wav"), SinkKind::Null).expect("engine loads");
        EngineBackend::open(engine)
    }

    fn video_backend() -> EngineBackend {
        let engine =
            SchedulerEngine::load(&fixture("h264.mp4"), SinkKind::Null).expect("engine loads");
        EngineBackend::open(engine)
    }

    fn poll_while(
        backend: &mut EngineBackend,
        done: impl Fn(&EngineStatus) -> bool,
    ) -> EngineStatus {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            backend.latest_frame();
            let status = backend.status();
            if done(&status) {
                return status;
            }
            assert!(Instant::now() < deadline, "engine never reached the state: {status:?}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn black_frame() -> Vec<u8> {
        let mut frame = vec![0u8; 320 * 240 * 4];
        for value in frame.iter_mut().skip(3).step_by(4) {
            *value = 255;
        }
        frame
    }

    fn inked(frame: &[u8]) -> usize {
        frame
            .chunks_exact(4)
            .filter(|pixel| pixel[..3].iter().any(|value| *value > 0))
            .count()
    }

    #[test]
    fn typed_controls_drive_playback_state() {
        let mut backend = backend();
        backend.set_paused(true);
        poll_while(&mut backend, |status| status.paused);
        backend.seek_to(1.0);
        poll_while(&mut backend, |status| (status.position - 1.0).abs() < 0.05);
        backend.set_paused(false);
        poll_while(&mut backend, |status| !status.paused);
        backend.set_volume_percent(110.0);
        poll_while(&mut backend, |status| {
            (status.volume as f64 * 100.0 - 110.0).abs() < 0.5
        });
        backend.set_muted(true);
        poll_while(&mut backend, |status| status.muted);
        assert!(backend.select_audio_id(1).is_ok());
        assert!(backend.select_audio_id(2).is_err());
        assert!(backend.select_sub_id(1).is_err());
    }

    #[test]
    fn subtitle_selection_by_id_deselect_and_first() {
        let mut backend = video_backend();
        poll_while(&mut backend, |status| status.tracks.len() >= 2);
        backend.add_sub(&fixture("external.srt")).expect("subtitle adds");
        poll_while(&mut backend, |status| status.tracks.len() >= 3);
        assert!(backend.subtitles_selected());
        backend.deselect_sub();
        poll_while(&mut backend, |status| {
            status
                .tracks
                .iter()
                .all(|track| track.kind != TrackKind::Subtitle || !track.selected)
        });
        backend.select_first_sub();
        poll_while(&mut backend, |status| {
            status
                .tracks
                .iter()
                .any(|track| track.kind == TrackKind::Subtitle && track.selected)
        });
        backend.deselect_sub();
        backend.select_sub_id(1).expect("subtitle 1 selects");
        poll_while(&mut backend, |status| {
            status
                .tracks
                .iter()
                .any(|track| track.kind == TrackKind::Subtitle && track.selected)
        });
        assert!(backend.select_sub_id(2).is_err());
        backend.set_sub_visible(false);
        assert!(!backend.sub_visible());
        assert!(backend.add_sub("no-such-sub.srt").is_err());
    }

    #[test]
    fn osc_overlay_composites_into_a_frame() {
        let mut backend = video_backend();
        let osc = AssOverlay {
            events: "{\\an7\\pos(40,30)\\fs40\\bord0\\shad0}Hi".to_owned(),
            res_x: 320,
            res_y: 240,
        };
        let mut frame = black_frame();
        backend
            .compose(&mut frame, 320, 240, Some(&osc))
            .expect("overlay composes");
        assert!(inked(&frame) > 0, "overlay drew no ink");
        assert_eq!(&frame[..3], &[0, 0, 0], "corner stays untouched");
        assert_eq!(backend.render_overlay(320, 240, None), Ok(None));
        let cropped = backend
            .render_overlay(320, 240, Some(&osc))
            .expect("overlay renders")
            .expect("overlay has ink");
        assert!(cropped.x >= 40 && cropped.y >= 30, "crop starts at the text: {cropped:?}");
        assert!(cropped.width < 120 && cropped.height < 80, "crop hugs the text");
        assert_eq!(cropped.rgba.len(), cropped.width as usize * cropped.height as usize * 4);
        assert!(cropped.rgba.chunks_exact(4).any(|pixel| pixel[3] > 0));
    }

    #[test]
    fn osd_message_expires() {
        let mut backend = backend();
        backend.show_text("fixture", DEFAULT_OSD_DURATION);
        assert!(backend.has_message());
        let later = Instant::now() + DEFAULT_OSD_DURATION + Duration::from_millis(1);
        assert!(backend.visible_message(later).is_none());
        assert!(!backend.has_message());
        let mut frame = black_frame();
        backend.show_text("fixture", Duration::from_secs(5));
        backend.compose(&mut frame, 320, 240, None).expect("message composes");
        assert!(inked(&frame) > 0, "message drew no ink");
    }

    #[test]
    #[cfg_attr(
        target_os = "linux",
        ignore = "static-pie test binaries cannot call the container Vulkan loader; run with RUSTFLAGS=\"-C target-feature=-crt-static\""
    )]
    fn external_subtitle_composites_into_a_frame() {
        let mut backend = video_backend();
        poll_while(&mut backend, |status| status.tracks.len() >= 2);
        backend.add_sub(&fixture("external.srt")).expect("subtitle adds");
        poll_while(&mut backend, |status| {
            status
                .tracks
                .iter()
                .any(|track| track.kind == TrackKind::Subtitle && track.selected)
        });
        backend.set_paused(true);
        backend.seek_to(0.6);
        poll_while(&mut backend, |status| (status.position - 0.6).abs() < 0.05);
        let mut frame = black_frame();
        backend
            .compose(&mut frame, 320, 240, None)
            .expect("subtitles compose");
        let inked = inked(&frame);
        assert!(inked > 100, "subtitle drew {inked} pixels");
    }
}
