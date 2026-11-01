//! UI drawn on top of the picture. For now: the error banner.

use std::time::{Duration, Instant};

use resvg::tiny_skia::{FillRule, Paint, Path, PathBuilder, Pixmap, Transform};

use crate::text::TextRenderer;

/// How long an error message stays on screen.
pub const BANNER_TIME: Duration = Duration::from_secs(6);
/// Space between the banner and the top of the window, in logical pixels.
const BANNER_TOP: f32 = 12.0;
const BANNER_SIDE_MIN: f32 = 12.0;
const BANNER_FONT_SIZE: f32 = 14.0;
const BANNER_PAD_X: f32 = 16.0;
const BANNER_PAD_Y: f32 = 9.0;
const BANNER_RADIUS: f32 = 8.0;
const BANNER_FILL: [u8; 4] = [0xB3, 0x26, 0x1E, 0xF2];
const BANNER_TEXT: [u8; 3] = [0xFF, 0xFF, 0xFF];

/// A short error message at the top of the window.
pub struct Banner {
    pub message: String,
    pub until: Instant,
    /// The drawn banner, and the scale and width limit it was drawn for.
    image: Option<(f32, u32, Pixmap)>,
}

impl Banner {
    pub fn new(message: String) -> Banner {
        Banner { message, until: Instant::now() + BANNER_TIME, image: None }
    }

    /// Draws the banner centered at the top of `dst`.
    pub fn draw(&mut self, text: &TextRenderer, dst: &mut [u32], size: (u32, u32), scale: f32) {
        let max_width = (size.0 as f32 - 2.0 * BANNER_SIDE_MIN * scale).max(1.0) as u32;
        let cached = matches!(&self.image, Some((s, w, _)) if *s == scale && *w == max_width);
        if !cached {
            self.image = banner_image(text, &self.message, scale, max_width as f32)
                .map(|image| (scale, max_width, image));
        }
        let Some((_, _, image)) = &self.image else { return };
        let x = (size.0 as i64 - i64::from(image.width())) / 2;
        let y = (BANNER_TOP * scale) as i64;
        composite(dst, size, image, x, y);
    }
}

fn banner_image(text: &TextRenderer, message: &str, scale: f32, max_width: f32) -> Option<Pixmap> {
    let font_size = BANNER_FONT_SIZE * scale;
    let (pad_x, pad_y) = (BANNER_PAD_X * scale, BANNER_PAD_Y * scale);
    let (ascent, line_height) = text.line_metrics(font_size);
    let width = (text.measure(message, font_size) + 2.0 * pad_x).min(max_width).ceil();
    let height = (line_height + 2.0 * pad_y).ceil();
    let mut pixmap = Pixmap::new(width as u32, height as u32)?;

    let mut paint = Paint::default();
    let [r, g, b, a] = BANNER_FILL;
    paint.set_color_rgba8(r, g, b, a);
    paint.anti_alias = true;
    let shape = rounded_rect(width, height, BANNER_RADIUS * scale)?;
    pixmap.fill_path(&shape, &paint, FillRule::Winding, Transform::identity(), None);

    text.draw(&mut pixmap, message, font_size, pad_x, pad_y + ascent, BANNER_TEXT);
    Some(pixmap)
}

fn rounded_rect(width: f32, height: f32, radius: f32) -> Option<Path> {
    let r = radius.min(width / 2.0).min(height / 2.0);
    // Control point distance for a quarter circle drawn with one cubic curve.
    let k = r * 0.552_284_8;
    let mut pb = PathBuilder::new();
    pb.move_to(r, 0.0);
    pb.line_to(width - r, 0.0);
    pb.cubic_to(width - r + k, 0.0, width, r - k, width, r);
    pb.line_to(width, height - r);
    pb.cubic_to(width, height - r + k, width - r + k, height, width - r, height);
    pb.line_to(r, height);
    pb.cubic_to(r - k, height, 0.0, height - r + k, 0.0, height - r);
    pb.line_to(0.0, r);
    pb.cubic_to(0.0, r - k, r - k, 0.0, r, 0.0);
    pb.close();
    pb.finish()
}

/// Draws premultiplied `src` over `dst` (0x00RRGGBB) with its top left at (x, y).
pub fn composite(dst: &mut [u32], dst_size: (u32, u32), src: &Pixmap, x: i64, y: i64) {
    let (dw, dh) = (i64::from(dst_size.0), i64::from(dst_size.1));
    let (sw, sh) = (i64::from(src.width()), i64::from(src.height()));
    if dst.len() < (dw * dh) as usize {
        return;
    }
    let data = src.data();
    for sy in 0.max(-y)..sh.min(dh - y) {
        for sx in 0.max(-x)..sw.min(dw - x) {
            let i = ((sy * sw + sx) * 4) as usize;
            let alpha = u32::from(data[i + 3]);
            if alpha == 0 {
                continue;
            }
            let d = &mut dst[((sy + y) * dw + sx + x) as usize];
            let blend = |s: u8, shift: u32| -> u32 {
                let under = (*d >> shift) & 0xFF;
                (u32::from(s) + under * (255 - alpha) / 255).min(255) << shift
            };
            *d = blend(data[i], 16) | blend(data[i + 1], 8) | blend(data[i + 2], 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use resvg::tiny_skia::Color;

    #[test]
    fn composite_blends_and_clips() {
        let mut src = Pixmap::new(2, 1).unwrap();
        src.fill(Color::from_rgba8(255, 0, 0, 255));
        let mut dst = vec![0x0000FF; 4];
        composite(&mut dst, (2, 2), &src, 1, 1);
        assert_eq!(dst, vec![0x0000FF, 0x0000FF, 0x0000FF, 0xFF0000]);

        let mut half = Pixmap::new(1, 1).unwrap();
        half.fill(Color::from_rgba8(255, 255, 255, 128));
        let mut dst = vec![0x000000];
        composite(&mut dst, (1, 1), &half, 0, 0);
        assert_eq!(dst[0] >> 16, 128);

        // Fully outside: nothing changes.
        let mut dst = vec![0x123456; 4];
        composite(&mut dst, (2, 2), &src, -5, 0);
        assert_eq!(dst, vec![0x123456; 4]);
    }

    #[test]
    fn banner_fits_the_window() {
        let text = TextRenderer::new().unwrap();
        let image = banner_image(&text, "This file isn't an SVG.", 1.0, 1000.0).unwrap();
        assert!(image.width() > 100 && image.width() < 400);
        let narrow = banner_image(&text, "This file isn't an SVG.", 1.0, 80.0).unwrap();
        assert_eq!(narrow.width(), 80);
    }
}
