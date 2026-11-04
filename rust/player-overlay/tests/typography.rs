use player_overlay::{Align, Color, TextBlock, TextEngine, TextLayout, TextStyle, Weight};

fn layout(text: &str, max_width: Option<f64>) -> std::sync::Arc<TextLayout> {
    let mut engine = TextEngine::new().expect("text engine starts");
    let block = TextBlock {
        text: text.to_owned(),
        style: TextStyle { size: 32.0, weight: Weight::Regular, color: Color::WHITE, outline: None },
        max_width,
        align: Align::Center,
    };
    engine.layout(&block).expect("text lays out")
}

fn inked(layout: &TextLayout) -> usize {
    layout.canvas.rgba.chunks_exact(4).filter(|pixel| pixel[3] > 128).count()
}

fn assert_renders(text: &str) -> std::sync::Arc<TextLayout> {
    let layout = layout(text, None);
    assert_eq!(layout.missing_glyphs, 0, "{text}: {:?}", layout.lines);
    assert!(inked(&layout) > 50, "{text} paints glyphs");
    assert_eq!(layout.lines.len(), 1);
    assert_eq!(layout.lines[0].range, 0..text.len());
    layout
}

fn directions(layout: &TextLayout) -> Vec<bool> {
    layout.lines.iter().flat_map(|line| line.runs.iter().map(|run| run.right_to_left)).collect()
}

#[test]
fn scripts_shape_with_installed_fonts() {
    for text in [
        "The quick brown fox",
        "Привет, мир",
        "Γειά σου κόσμε",
        "你好，世界",
        "こんにちは世界",
        "안녕하세요",
        "สวัสดีชาวโลก",
        "नमस्ते दुनिया",
    ] {
        assert_renders(text);
    }
}

#[test]
fn arabic_and_hebrew_run_right_to_left() {
    for text in ["مرحبا بالعالم", "שלום עולם"] {
        let layout = assert_renders(text);
        assert!(directions(&layout).iter().all(|rtl| *rtl), "{text}");
    }
}

#[test]
fn arabic_letters_join_into_contextual_forms() {
    let joined = layout("ببب", None);
    let isolated = layout("ب ب ب", None);
    let glyphs = |layout: &TextLayout| -> Vec<u32> {
        layout.lines[0].runs.iter().flat_map(|run| run.glyphs.clone()).collect()
    };
    let joined = glyphs(&joined);
    let isolated = glyphs(&isolated);
    assert!(joined.iter().any(|glyph| !isolated.contains(glyph)), "joined {joined:?} isolated {isolated:?}");
}

#[test]
fn mixed_direction_text_keeps_both_runs() {
    let layout = assert_renders("Hello שלום world");
    let directions = directions(&layout);
    assert!(directions.contains(&true) && directions.contains(&false), "{directions:?}");
}

#[test]
fn devanagari_conjuncts_form_fewer_glyphs_than_characters() {
    let text = "क्षत्रिय";
    let layout = assert_renders(text);
    let glyphs: usize = layout.lines[0].runs.iter().map(|run| run.glyphs.len()).sum();
    assert!(glyphs < text.chars().count(), "{glyphs} glyphs");
}

#[test]
fn combining_marks_attach_without_widening() {
    let base = layout("e", None);
    let marked = layout("e\u{301}\u{323}", None);
    assert_eq!(marked.missing_glyphs, 0);
    assert!((marked.text_width - base.text_width).abs() < 1.0);
}

#[test]
fn emoji_render_in_color() {
    let layout = assert_renders("🎬🍿");
    let colored = layout
        .canvas
        .rgba
        .chunks_exact(4)
        .filter(|pixel| pixel[3] > 200 && (pixel[0] != pixel[1] || pixel[1] != pixel[2]))
        .count();
    assert!(colored > 50, "emoji keep their own colors");
}

#[test]
fn long_text_wraps_within_the_width() {
    let text = "The quick brown fox jumps over the lazy dog and keeps on running";
    let layout = layout(text, Some(300.0));
    assert!(layout.lines.len() >= 3, "{:?}", layout.lines);
    for line in &layout.lines {
        assert!(line.width <= 300.0, "{line:?}");
    }
    let rejoined: Vec<&str> = layout.lines.iter().map(|line| text[line.range.clone()].trim()).collect();
    assert_eq!(rejoined.join(" "), text);
}

#[test]
fn hard_breaks_start_new_lines() {
    let layout = layout("first\nsecond", None);
    assert_eq!(layout.lines.len(), 2);
    assert_eq!(layout.lines[1].range, 6..12);
    assert!(layout.height() >= 2.0 * layout.line_height);
}

#[test]
fn unassigned_code_points_count_as_missing() {
    let layout = layout("\u{10FFFD}", None);
    assert!(layout.missing_glyphs > 0);
}
