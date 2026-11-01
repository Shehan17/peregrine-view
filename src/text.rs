//! UI text, drawn with one small embedded font.

use ab_glyph::{Font, FontRef, GlyphId, PxScale, ScaleFont, point};
use resvg::tiny_skia::Pixmap;

/// Noto Sans Regular, cut down to Latin and common punctuation (SIL OFL 1.1).
static FONT: &[u8] = include_bytes!("../assets/fonts/NotoSans-Regular-UI.ttf");

pub struct TextRenderer {
    font: FontRef<'static>,
}

impl TextRenderer {
    pub fn new() -> Option<TextRenderer> {
        match FontRef::try_from_slice(FONT) {
            Ok(font) => Some(TextRenderer { font }),
            Err(e) => {
                eprintln!("could not read the UI font: {e}");
                None
            }
        }
    }

    /// The width of `text` at `size` pixels.
    pub fn measure(&self, text: &str, size: f32) -> f32 {
        self.layout(text, size, |_, _| {})
    }

    /// The distance from the top of a line to its baseline, and the line height.
    pub fn line_metrics(&self, size: f32) -> (f32, f32) {
        let font = self.font.as_scaled(PxScale::from(size));
        (font.ascent(), font.ascent() - font.descent())
    }

    /// Draws `text` into `pixmap` with its baseline at `y`, starting at `x`.
    pub fn draw(&self, pixmap: &mut Pixmap, text: &str, size: f32, x: f32, y: f32, color: [u8; 3]) {
        let (width, height) = (pixmap.width() as i64, pixmap.height() as i64);
        let data = pixmap.data_mut();
        let _ = self.layout(text, size, |id, caret| {
            let glyph = id.with_scale_and_position(PxScale::from(size), point(x + caret, y));
            let Some(outlined) = self.font.outline_glyph(glyph) else { return };
            let bounds = outlined.px_bounds();
            outlined.draw(|gx, gy, coverage| {
                let px = bounds.min.x as i64 + i64::from(gx);
                let py = bounds.min.y as i64 + i64::from(gy);
                if px < 0 || py < 0 || px >= width || py >= height {
                    return;
                }
                let i = ((py * width + px) * 4) as usize;
                let alpha = coverage.clamp(0.0, 1.0);
                // Blend in premultiplied RGBA.
                for (channel, &c) in data[i..i + 3].iter_mut().zip(&color) {
                    *channel = (f32::from(c) * alpha + f32::from(*channel) * (1.0 - alpha)) as u8;
                }
                data[i + 3] = (255.0 * alpha + f32::from(data[i + 3]) * (1.0 - alpha)) as u8;
            });
        });
    }

    /// Calls `place` with each glyph and its pen position, and returns the
    /// full width.
    fn layout(&self, text: &str, size: f32, mut place: impl FnMut(GlyphId, f32)) -> f32 {
        let font = self.font.as_scaled(PxScale::from(size));
        let mut caret = 0.0;
        let mut previous: Option<GlyphId> = None;
        for c in text.chars() {
            let id = font.glyph_id(c);
            if let Some(previous) = previous {
                caret += font.kern(previous, id);
            }
            place(id, caret);
            caret += font.h_advance(id);
            previous = Some(id);
        }
        caret
    }
}
