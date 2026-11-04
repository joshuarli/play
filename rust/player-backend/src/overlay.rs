use player_overlay::{Align, Anchor, Color, DisplayList, Outline, OverlayImage, Point, TextBlock, TextEngine, TextStyle, Weight};

const BASE_HEIGHT: f64 = 720.0;
const SUB_SIZE: f64 = 32.0;
const SUB_OUTLINE: f64 = 1.65;
const SUB_MARGIN_X: f64 = 19.0;
const SUB_MARGIN_Y: f64 = 34.0;
const OSD_SIZE: f64 = 25.0;
const OSD_OUTLINE: f64 = 1.65;
const OSD_MARGIN: f64 = 16.0;

/// What the player draws over video this frame, bottom to top.
pub struct Scene<'a> {
    pub subtitle: Option<&'a str>,
    pub message: Option<&'a str>,
    pub osc: Option<&'a DisplayList>,
}

/// Lays out subtitles and OSD messages and rasterizes them with the OSC.
pub struct OverlayRenderer {
    text: TextEngine,
}

fn outlined(text: &str, size: f64, outline: f64, max_width: f64, align: Align) -> TextBlock {
    TextBlock {
        text: text.to_owned(),
        style: TextStyle {
            size,
            weight: Weight::Regular,
            color: Color::WHITE,
            outline: Some(Outline { width: outline, color: Color::BLACK }),
        },
        max_width: Some(max_width.max(1.0)),
        align,
    }
}

/// Subtitles sit bottom-centre and wrap inside the side margins, in units of
/// a 720-line window.
pub fn subtitle_layer(text: &str, width: u32, height: u32) -> DisplayList {
    let scale = height as f64 / BASE_HEIGHT;
    let mut list = DisplayList::new(width, height, scale);
    let logical_width = width as f64 / scale;
    list.text(
        Point { x: logical_width / 2.0, y: BASE_HEIGHT - SUB_MARGIN_Y },
        Anchor::BOTTOM_CENTER,
        outlined(text, SUB_SIZE, SUB_OUTLINE, logical_width - 2.0 * SUB_MARGIN_X, Align::Center),
    );
    list
}

/// OSD messages sit top-left.
pub fn message_layer(text: &str, width: u32, height: u32) -> DisplayList {
    let scale = height as f64 / BASE_HEIGHT;
    let mut list = DisplayList::new(width, height, scale);
    let logical_width = width as f64 / scale;
    list.text(
        Point { x: OSD_MARGIN, y: OSD_MARGIN },
        Anchor::TOP_LEFT,
        outlined(text, OSD_SIZE, OSD_OUTLINE, logical_width - 2.0 * OSD_MARGIN, Align::Start),
    );
    list
}

impl OverlayRenderer {
    pub fn new() -> Result<Self, String> {
        Ok(OverlayRenderer { text: TextEngine::new()? })
    }

    pub fn render(&mut self, scene: &Scene, width: u32, height: u32) -> Result<Option<OverlayImage>, String> {
        let subtitle = scene.subtitle.filter(|text| !text.trim().is_empty()).map(|text| subtitle_layer(text, width, height));
        let message = scene.message.map(|text| message_layer(text, width, height));
        let mut layers: Vec<&DisplayList> = Vec::with_capacity(3);
        layers.extend(subtitle.as_ref());
        layers.extend(message.as_ref());
        if let Some(osc) = scene.osc {
            if (osc.width, osc.height) != (width, height) {
                return Err(format!(
                    "OSC was laid out for {}x{} but the overlay is {width}x{height}",
                    osc.width, osc.height
                ));
            }
            layers.push(osc);
        }
        player_overlay::render(&layers, &mut self.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered(scene: Scene) -> Option<OverlayImage> {
        OverlayRenderer::new().unwrap().render(&scene, 1280, 720).unwrap()
    }

    #[test]
    fn subtitles_sit_bottom_centre_above_the_margin() {
        let image = rendered(Scene { subtitle: Some("Hello there"), message: None, osc: None }).unwrap();
        let centre = image.x as f64 + image.width as f64 / 2.0;
        assert!((centre - 640.0).abs() < 4.0, "{image:?}");
        let bottom = image.y + image.height;
        assert!(bottom > 720 - 34 && bottom < 720 - 34 + 20, "bottom {bottom}");
    }

    #[test]
    fn long_subtitles_wrap_inside_the_margins() {
        let line = "a fairly long subtitle line that keeps going ".repeat(4);
        let image = rendered(Scene { subtitle: Some(&line), message: None, osc: None }).unwrap();
        assert!(image.x >= 19 - 12 && image.x + image.width <= 1280 - 19 + 12, "{image:?}");
        assert!(image.height > 80, "wrapped onto several lines");
    }

    #[test]
    fn messages_sit_top_left_and_blank_subtitles_draw_nothing() {
        let image = rendered(Scene { subtitle: Some("  "), message: Some("Volume: 50%"), osc: None }).unwrap();
        assert!(image.x < 30 && image.y < 30, "{image:?}");
        assert!(image.y + image.height < 80);
        assert_eq!(rendered(Scene { subtitle: Some(""), message: None, osc: None }), None);
    }

    #[test]
    fn an_osc_laid_out_for_another_size_is_rejected() {
        let osc = DisplayList::new(640, 360, 0.5);
        let scene = Scene { subtitle: None, message: None, osc: Some(&osc) };
        assert!(OverlayRenderer::new().unwrap().render(&scene, 1280, 720).is_err());
    }
}
