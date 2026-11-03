use std::collections::HashMap;

use player_ass::{Library, RenderedImage, Renderer, Track};

use crate::engine::OsdOverlay;

const OSD_SYMBOLS_NAME: &str = "mpv-osd-symbols";
const OSD_SYMBOLS_FONT: &[u8] = include_bytes!("../../../sub/osd_font.otf");
const FONT_PLAYRES_X: f64 = 384.0;
const FONT_PLAYRES_Y: f64 = 288.0;
const PLAYRES_BASE: f64 = 720.0;
const WORD_JOINER: char = '\u{2060}';

fn ass_color(red: u8, green: u8, blue: u8, alpha: u8) -> String {
    format!("&H{:02X}{:02X}{:02X}{:02X}", 255 - alpha, blue, green, red)
}

fn alignment(align_x: i64, align_y: i64) -> i64 {
    let horizontal = align_x + 2;
    let vertical = match align_y {
        1 => 0,
        0 => 3,
        _ => 6,
    };
    vertical + horizontal
}

fn style_line(
    name: &str,
    font_size: f64,
    outline: f64,
    margin_x: f64,
    margin_y: f64,
    align_x: i64,
    align_y: i64,
) -> String {
    format!(
        "Style: {name},sans-serif,{font_size:.3},{ink},{ink},{outline_color},{back},0,0,0,0,100,100,0,0,1,{outline:.3},0,{align},{margin_x:.3},{margin_x:.3},{margin_y:.3},-1",
        ink = ass_color(255, 255, 255, 255),
        outline_color = ass_color(0, 0, 0, 255),
        back = ass_color(0, 0, 0, 175),
        align = alignment(align_x, align_y),
    )
}

fn styles(scale: f64) -> String {
    format!(
        "{}\n{}",
        style_line("OSD", 30.0 * scale, 1.65 * scale, 16.0 * scale, 16.0 * scale, -1, -1),
        style_line(
            "Default",
            38.0 * scale,
            1.65 * scale,
            19.0 * scale,
            34.0 * scale,
            0,
            1
        ),
    )
}

fn script_info(playres_x: i64, playres_y: i64, styles: &str) -> String {
    format!(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: {playres_x}\nPlayResY: {playres_y}\nTimer: 100.0000\nWrapStyle: 1\nScaledBorderAndShadow: yes\nKerning: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n{styles}\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n"
    )
}

fn frame_aspect(width: i64, height: i64, par: f64) -> f64 {
    let mut aspect = if height > 0 {
        width as f64 / height as f64
    } else {
        0.0
    };
    if par > 0.0 {
        aspect /= par;
    }
    aspect
}

fn overlay_playres(res_x: i64, res_y: i64, aspect: f64) -> (i64, i64) {
    let playres_y = if res_y > 0 { res_y } else { FONT_PLAYRES_Y as i64 };
    let playres_x = if res_x > 0 {
        res_x
    } else if aspect > 0.0 {
        (playres_y as f64 * aspect) as i64
    } else {
        FONT_PLAYRES_X as i64
    };
    (playres_x, playres_y)
}

fn overlay_content(data: &str, res_x: i64, res_y: i64, width: i64, height: i64, par: f64) -> String {
    let (playres_x, playres_y) = overlay_playres(res_x, res_y, frame_aspect(width, height, par));
    let scale = playres_y as f64 / PLAYRES_BASE;
    let mut content = script_info(playres_x, playres_y, &styles(scale));
    for line in data.split('\n') {
        if line.is_empty() {
            continue;
        }
        content.push_str("Dialogue: 0,0:00:00.00,0:00:00.10,OSD,,0,0,0,,");
        content.push_str(line);
        content.push('\n');
    }
    content
}

fn escape_osd_text(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    let mut line_start = true;
    for character in text.chars() {
        match character {
            '\n' => {
                escaped.push_str("\\N");
                line_start = true;
            }
            '{' => {
                escaped.push_str("\\{");
                line_start = false;
            }
            '\\' => {
                escaped.push('\\');
                escaped.push(WORD_JOINER);
                line_start = false;
            }
            ' ' if line_start => {
                escaped.push_str("\\h");
                line_start = false;
            }
            other => {
                escaped.push(other);
                line_start = false;
            }
        }
    }
    escaped
}

fn message_content(text: &str, width: i64, height: i64, par: f64) -> (String, (i64, i64)) {
    let aspect = frame_aspect(width, height, par);
    let playres_y = FONT_PLAYRES_Y as i64;
    let playres_x = if aspect > 0.0 {
        (playres_y as f64 * aspect) as i64
    } else {
        FONT_PLAYRES_X as i64
    };
    let mut content = script_info(
        playres_x,
        playres_y,
        &styles(playres_y as f64 / PLAYRES_BASE),
    );
    let escaped = escape_osd_text(text);
    for line in escaped.split('\n') {
        if line.is_empty() {
            continue;
        }
        content.push_str("Dialogue: 0,0:00:00.00,0:00:00.10,OSD,,0,0,0,,");
        content.push_str(line);
        content.push('\n');
    }
    (content, (playres_x, playres_y))
}

struct OverlayTrack {
    content: String,
    playres: (i64, i64),
    track: Track,
}

pub struct OsdState {
    library: Library,
    renderer: Renderer,
    overlays: HashMap<i64, OverlayTrack>,
    message: Option<(String, (i64, i64), Track)>,
}

impl OsdState {
    pub fn new() -> Result<Self, String> {
        let library = Library::new()?;
        library.add_font(OSD_SYMBOLS_NAME, OSD_SYMBOLS_FONT)?;
        let renderer = Renderer::new(&library)?;
        Ok(OsdState {
            library,
            renderer,
            overlays: HashMap::new(),
            message: None,
        })
    }

    pub fn render(
        &mut self,
        overlays: &[&OsdOverlay],
        message: Option<&str>,
        width: i64,
        height: i64,
        par: f64,
    ) -> Result<Vec<RenderedImage>, String> {
        let mut images = Vec::new();
        let mut order: Vec<usize> = (0..overlays.len()).collect();
        order.sort_by_key(|&index| (overlays[index].z, overlays[index].id));
        let live: Vec<i64> = order.iter().map(|&index| overlays[index].id).collect();
        self.overlays.retain(|id, _| live.contains(id));

        if let Some(text) = message {
            let (content, playres) = message_content(text, width, height, par);
            let stale = match &self.message {
                Some((key, _, _)) => key != &content,
                None => true,
            };
            if stale {
                let mut track = Track::new(&self.library)?;
                track.process_data(&content)?;
                if self
                    .message
                    .as_ref()
                    .is_some_and(|(_, stored, _)| *stored != playres)
                {
                    self.renderer.clear_cache();
                }
                self.message = Some((content, playres, track));
            }
            let (_, _, track) = self.message.as_mut().expect("message track is stored");
            images.extend(self.renderer.render(
                track,
                width as i32,
                height as i32,
                None,
                par,
                0,
            )?);
        } else {
            self.message = None;
        }

        for index in order {
            let overlay = overlays[index];
            if overlay.hidden || overlay.format != "ass-events" || overlay.data.is_empty() {
                continue;
            }
            let content = overlay_content(
                &overlay.data,
                overlay.res_x,
                overlay.res_y,
                width,
                height,
                par,
            );
            let playres = overlay_playres(
                overlay.res_x,
                overlay.res_y,
                frame_aspect(width, height, par),
            );
            let stale = match self.overlays.get(&overlay.id) {
                Some(stored) => stored.content != content,
                None => true,
            };
            if stale {
                let mut track = Track::new(&self.library)?;
                track.process_data(&content)?;
                if self
                    .overlays
                    .get(&overlay.id)
                    .is_some_and(|stored| stored.playres != playres)
                {
                    self.renderer.clear_cache();
                }
                self.overlays.insert(
                    overlay.id,
                    OverlayTrack {
                        content,
                        playres,
                        track,
                    },
                );
            }
            let stored = self
                .overlays
                .get_mut(&overlay.id)
                .expect("stored overlay renders");
            images.extend(self.renderer.render(
                &mut stored.track,
                width as i32,
                height as i32,
                None,
                par,
                0,
            )?);
        }
        Ok(images)
    }
}

pub fn composite(
    frame: &mut [u8],
    width: u32,
    height: u32,
    images: &[RenderedImage],
) -> Result<(), String> {
    let expected = width as usize * height as usize * 4;
    if frame.len() < expected {
        return Err(format!(
            "overlay target holds {} bytes, needs {expected}",
            frame.len()
        ));
    }
    for image in images {
        let source_red = ((image.color >> 24) & 0xff) as u32;
        let source_green = ((image.color >> 16) & 0xff) as u32;
        let source_blue = ((image.color >> 8) & 0xff) as u32;
        let source_alpha = (0xff - (image.color & 0xff)) as u32;
        for row in 0..image.height {
            let target_y = image.dst_y + row as i32;
            if target_y < 0 || target_y >= height as i32 {
                continue;
            }
            for column in 0..image.width {
                let target_x = image.dst_x + column as i32;
                if target_x < 0 || target_x >= width as i32 {
                    continue;
                }
                let coverage =
                    image.bitmap[row as usize * image.stride + column as usize] as u32;
                let coverage_alpha = source_alpha * coverage;
                if coverage_alpha == 0 {
                    continue;
                }
                let inverse = 255 * 255 - coverage_alpha;
                let offset = (target_y as usize * width as usize + target_x as usize) * 4;
                for (channel, source) in
                    [source_red, source_green, source_blue].into_iter().enumerate()
                {
                    let target = frame[offset + channel] as u32;
                    frame[offset + channel] =
                        ((source * coverage_alpha + target * inverse) / (255 * 255)) as u8;
                }
                let target_alpha = frame[offset + 3] as u32;
                frame[offset + 3] =
                    ((coverage_alpha * 255 + target_alpha * inverse) / (255 * 255)) as u8;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{composite, OsdState};
    use crate::engine::OsdOverlay;
    use player_ass::RenderedImage;

    fn overlay(data: &str) -> OsdOverlay {
        OsdOverlay {
            id: 1,
            format: "ass-events".to_owned(),
            data: data.to_owned(),
            res_x: 320,
            res_y: 240,
            z: 0,
            hidden: false,
        }
    }

    fn ink(frame: &[u8], width: u32) -> Vec<(u32, u32)> {
        let mut points = Vec::new();
        for offset in (0..frame.len()).step_by(4) {
            if frame[offset..offset + 3].iter().any(|value| *value > 0) {
                let pixel = offset / 4;
                points.push((pixel as u32 % width, pixel as u32 / width));
            }
        }
        points
    }

    #[test]
    fn overlay_renders_inside_its_position() {
        let mut state = OsdState::new().expect("osd starts");
        let overlay = overlay("{\\an7\\pos(40,30)\\fs40\\bord0\\shad0}Hi");
        let images = state
            .render(&[&overlay], None, 320, 240, 1.0)
            .expect("overlay renders");
        assert!(!images.is_empty(), "overlay produced no images");
        let mut frame = vec![0u8; 320 * 240 * 4];
        for value in frame.iter_mut().skip(3).step_by(4) {
            *value = 255;
        }
        composite(&mut frame, 320, 240, &images).expect("overlay composites");
        let points = ink(&frame, 320);
        assert!(!points.is_empty(), "overlay produced no ink");
        let (min_x, max_x) = points.iter().fold((u32::MAX, 0), |(low, high), (x, _)| {
            (low.min(*x), high.max(*x))
        });
        let (min_y, max_y) = points.iter().fold((u32::MAX, 0), |(low, high), (_, y)| {
            (low.min(*y), high.max(*y))
        });
        assert!(min_x >= 40 && max_x < 120, "x range {min_x}..{max_x}");
        assert!(min_y >= 30 && max_y < 110, "y range {min_y}..{max_y}");
        assert_eq!(frame[0], 0, "top-left corner stays untouched");
    }

    #[test]
    fn hidden_overlay_and_empty_message_render_nothing() {
        let mut state = OsdState::new().expect("osd starts");
        let mut hidden = overlay("{\\an7}hidden");
        hidden.hidden = true;
        let images = state
            .render(&[&hidden], None, 320, 240, 1.0)
            .expect("hidden overlay renders");
        assert!(images.is_empty());
    }

    #[test]
    fn osd_message_renders_at_the_top_left() {
        let mut state = OsdState::new().expect("osd starts");
        let images = state
            .render(&[], Some("probe text"), 320, 240, 1.0)
            .expect("message renders");
        assert!(!images.is_empty(), "message produced no images");
        let mut frame = vec![0u8; 320 * 240 * 4];
        for value in frame.iter_mut().skip(3).step_by(4) {
            *value = 255;
        }
        composite(&mut frame, 320, 240, &images).expect("message composites");
        let points = ink(&frame, 320);
        assert!(!points.is_empty(), "message produced no ink");
        let min_y = points.iter().map(|(_, y)| *y).min().unwrap_or(u32::MAX);
        let max_y = points.iter().map(|(_, y)| *y).max().unwrap_or(0);
        assert!(min_y < 12, "message starts too low: {min_y}");
        assert!(max_y < 30, "message reaches too low: {max_y}");
        let min_x = points.iter().map(|(x, _)| *x).min().unwrap_or(u32::MAX);
        assert!(min_x < 12, "message starts too far right: {min_x}");
    }

    #[test]
    fn composite_matches_mpv_blending() {
        let image = RenderedImage {
            width: 1,
            height: 1,
            stride: 1,
            bitmap: vec![128],
            color: 0x40_20_10_20,
            dst_x: 0,
            dst_y: 0,
        };
        let mut frame = vec![100, 50, 20, 255];
        composite(&mut frame, 1, 1, &[image]).expect("composite runs");
        let alpha = (255 - 0x20) * 128;
        let inverse = 255 * 255 - alpha;
        let expected = |source: u32, target: u32| {
            ((source * alpha + target * inverse) / (255 * 255)) as u8
        };
        assert_eq!(frame[0], expected(0x40, 100));
        assert_eq!(frame[1], expected(0x20, 50));
        assert_eq!(frame[2], expected(0x10, 20));
        assert_eq!(frame[3], 255);
    }
}
