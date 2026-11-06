use player_media::{HwRequest, MediaSession, VideoSession};
use player_tui::{COVER_SIZE, Cover};

pub fn cover_art(path: &str) -> Option<Cover> {
    let session = MediaSession::open(path).ok()?;
    let attachment = session.attachments().ok()?.into_iter().next()?;
    let mut decoder = VideoSession::open(path, attachment.stream_index, HwRequest::None).ok()?;
    decoder.send(&attachment.data, Some(0.0)).ok()?;
    let frame = decoder.receive().ok()??;
    let pixels = frame.to_rgb24(COVER_SIZE as i32, COVER_SIZE as i32).ok()?;
    Cover::new(pixels)
}
