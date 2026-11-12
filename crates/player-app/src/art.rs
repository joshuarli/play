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

pub(crate) fn gui_cover(path: &str) -> Result<Option<player_engine::video::PresentedFrame>, String> {
    let media = MediaSession::open(path).map_err(|error| error.to_string())?;
    let Some(attachment) = media.attachments().map_err(|error| error.to_string())?.into_iter().next() else {
        return Ok(None);
    };
    let mut decoder = VideoSession::open_shared(&media, attachment.stream_index, HwRequest::None)?;
    if !decoder.send(&attachment.data, Some(0.0))? {
        return Err("cover decoder did not accept its image".to_owned());
    }
    let frame = decoder.receive()?.ok_or("cover decoder returned no image")?;
    player_engine::video::PresentedFrame::from_frame(frame, 0.0, 0).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gui_cover_keeps_image_dimensions_and_owned_planes() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.work/fixtures");
        for name in ["audio-cover.flac", "audio-cover-jpeg.mp3"] {
            let frame = gui_cover(root.join(name).to_str().unwrap()).unwrap().unwrap();
            assert!(frame.width > 0 && frame.height > 0);
            assert!(frame.planes().unwrap().iter().all(|plane| !plane.data.is_empty()));
        }
        assert!(gui_cover(root.join("audio-flac.flac").to_str().unwrap()).unwrap().is_none());
    }
}
