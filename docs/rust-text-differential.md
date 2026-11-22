# Rust text stack differential

Evidence for LAP-51 Track D: how far harfrust shaping and skrifa/zeno glyph masks are from the HarfBuzz and FreeType path Play ships on Linux. Nothing in the production path changes; the harness is a separate, unpublished crate that is not a workspace member.

## Run it

```
cd crates/player-overlay/text-differential
RUSTFLAGS="-C target-feature=-crt-static" cargo \
  --config 'source.crates-io.replace-with="mirror"' \
  --config 'source.mirror.registry="sparse+https://index.crates.io./"' \
  test -- --ignored --nocapture
```

Requirements:

- `scripts/build-rust-fontconfig-linux.sh` has populated `.work/deps/rust-fontconfig`, as for any `player-overlay` build.
- The two `--config` flags are needed while the `stuff` crates mirror named in `.cargo/config.toml` has no `harfrust`, `skrifa`, `zeno`, or `read-fonts`. They point Cargo at crates.io for this one invocation; the trailing dot in the host name only gives the source a distinct identity. Drop them once the mirror carries the crates.
- `-crt-static` off, as in `scripts/build.sh`: a static musl test binary cannot load the HarfBuzz, FreeType, and rust-fontconfig libraries.

The harness reads font directories from `$FONTCONFIG_FILE` (default `/etc/fonts/fonts.conf`), which is also what Play's rust-fontconfig reads, so fonts added there reach both sides. `PLAY_TEXT_DIFF_FONTS` (colon-separated directories) adds fonts the harness may use without changing what Play can see; the variable-weight entry takes its font only from the indexed set and is skipped, and said so, when none has a `wght` axis covering the text.

## What it compares

Corpus: Latin with combining marks, Arabic, Hebrew in mixed bidi, Devanagari, Thai, CJK (Han, Hiragana, Hangul), ZWJ, flag, keycap, and skin-tone emoji, and variable-weight Latin. Text is split into runs by bidi level and resolved script, as Play does. Size is 32 px.

1. Font choice. Each run is laid out by Play's own `TextEngine`; the font of its first glyph run is the font both sides use. A run with `missing_glyphs > 0` is counted as uncovered. Its shaping is still compared (under the `uncovered.` prefix) but it is never rasterized.
2. Shaping. Direct HarfBuzz calls with Play's parameters (scale 64 units per pixel, direction from the level, script when not neutral, `guess_segment_properties`, no features) against harfrust at the same scale. Compared per glyph: glyph id, cluster, x and y advance, x and y offset. Counted as glyph-count, glyph-id, cluster, advance, and offset differences.
3. Play engine cross-check. For a single-line, single-glyph-run layout, `TextEngine`'s glyph ids against the direct HarfBuzz ids, which confirms the harness reproduces what Play does.
4. Masks. For each distinct glyph: FreeType with `FT_LOAD_NO_HINTING | FT_LOAD_COLOR` and normal rendering, as in Play's `draw`, against skrifa unhinted outlines rasterized by zeno at the origin. Compared: bounding box edges (identical, within 1 px, more), per-pixel coverage on the union box (max difference buckets of 255, summed absolute difference, summed coverage), plus glyphs one side cannot produce (color bitmaps, no outline, empty on one side only).
5. Variable weight. The default instance, which is what Play loads, and `wght=700` on both sides (`hb_font_set_variations` and `FT_Set_Var_Design_Coordinates` against `Font::instance_builder`). The run asserts that `wght=700` adds ink on both FreeType and skrifa, so a silently ignored variation fails the test.

## Result on this host

Host: x86-64 musl Linux, system HarfBuzz 14.2.1 and FreeType, DejaVu, Noto Sans and Serif, Noto Sans Symbols, JetBrains Mono. A run with `NotoColorEmoji` (CBDT) added to the fontconfig directories and the variable `JetBrainsMono[wght].ttf` added through `PLAY_TEXT_DIFF_FONTS` indexed 65 faces and compared 16 runs.

Matched:

- Shaping: 10 covered runs (98 glyphs at the default instance plus 27 at `wght=700`) were identical in glyph id, cluster, advance, and offset. Zero differences in every category, including Arabic joining and the lam-alef ligature, Latin stacked combining marks, and Hebrew with embedded Latin and digits.
- Play engine ids matched direct HarfBuzz in 13 of 13 comparable runs; one isolated run (` !` inside an RTL context) is not comparable because the harness splits it from its bidi context.
- Masks: 66 default-instance glyphs and 20 `wght=700` glyphs had identical bounding boxes. Coverage never differed by more than 4 of 255, and total coverage was about 0.14 percent lower in skrifa/zeno than in FreeType. No difference in the 9 to 255 range.

Differed or not comparable:

- The 6 runs for Devanagari, Thai, Han, Hiragana, and Hangul have no covering font on this host (`corpus.runs_with_no_covering_host_font = 6`). Their shaping through `.notdef` is identical, which says nothing about conjunct formation, Thai marks, or CJK features. These scripts remain unmeasured until a host with the fonts runs the harness.
- Color emoji: FreeType returns BGRA strikes for 5 glyphs and skrifa has neither an outline nor COLR paint for them, so no mask is comparable (`freetype_color_bitmap.skrifa_has_no_outline = 5`). Replacing FreeType for CBDT or sbix fonts needs bitmap decoding that these crates do not do in this harness.
- Version skew: the system HarfBuzz is 14.2.1 and this harfrust matches HarfBuzz 15.2.0, so a difference on a newer shaping rule would show up as a version difference rather than a port bug.

## What the harness cannot see

- Hinting. Play uses no hinting, so the comparison is unhinted; skrifa's hinted paths are not exercised.
- Color glyph composition: COLR v0 and v1 painting, CBDT, and sbix bitmaps, and how Play's `Bitmap::blit` scales a strike.
- Subpixel placement. FreeType here renders at the integer origin; Play moves glyphs by a fractional `FT_Set_Transform` delta, which is not compared.
- Fallback and line breaking. Font choice, fallback splitting, and wrapping stay with rust-fontconfig and Play; only the first font of each run is compared.
- Anything past a single glyph: pen placement across runs, outlines (stroking), canvas compositing, performance, and memory.
- Fonts the host lacks, as above.

## Dependencies

All are dev-only to the harness crate, pure Rust, and need no native library. The production dependency graph is unchanged: `cargo tree -e normal -p player-overlay` lists the same three crates before and after.

| Crate | Version | License | Maintenance |
|---|---|---|---|
| harfrust | 0.16.1 | MIT | harfbuzz organization; releases 2026-10-22, 11-12, 11-19 |
| skrifa | 0.50.0 | MIT OR Apache-2.0 | googlefonts/fontations; releases 2026-10-03, 10-21, 11-10 |
| read-fonts, font-types | 0.47.0, 0.12.7 | MIT OR Apache-2.0 | same repository, pulled in by skrifa and harfrust |
| zeno | 0.3.3 | Apache-2.0 OR MIT | dfrg; last release 2025-05-08, low churn, small and stable |

swash 0.2.10 (2026-07-17) was not needed: zeno rasterizes skrifa's outlines directly.
