//! Zoom and pan math. Pure code, no window or rendering dependencies.

use resvg::tiny_skia::Transform;

/// Maps SVG user units to physical window pixels: `screen = svg * scale + (tx, ty)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub scale: f32,
    pub tx: f32,
    pub ty: f32,
}

impl Default for View {
    fn default() -> Self {
        Self { scale: 1.0, tx: 0.0, ty: 0.0 }
    }
}

impl View {
    pub fn transform(&self) -> Transform {
        Transform::from_row(self.scale, 0.0, 0.0, self.scale, self.tx, self.ty)
    }
}

/// Fits `content` inside `viewport` with `margin` on every side, centered.
/// Small content is scaled up. Falls back to no margin when the viewport is
/// too small for it, and to the identity view when the content has no size.
pub fn fit(content: (f32, f32), viewport: (f32, f32), margin: f32) -> View {
    let (cw, ch) = content;
    let (vw, vh) = viewport;
    if !(cw.is_finite() && ch.is_finite() && cw > 0.0 && ch > 0.0) {
        return View::default();
    }
    if !(vw > 0.0 && vh > 0.0) {
        return View::default();
    }

    let margin = if vw > 2.0 * margin && vh > 2.0 * margin { margin.max(0.0) } else { 0.0 };
    let scale = ((vw - 2.0 * margin) / cw).min((vh - 2.0 * margin) / ch);
    View {
        scale,
        tx: (vw - cw * scale) / 2.0,
        ty: (vh - ch * scale) / 2.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn wide_content_centers_vertically() {
        let v = fit((200.0, 100.0), (420.0, 420.0), 10.0);
        assert!(approx(v.scale, 2.0));
        assert!(approx(v.tx, 10.0));
        assert!(approx(v.ty, 110.0));
    }

    #[test]
    fn tall_content_centers_horizontally() {
        let v = fit((100.0, 200.0), (420.0, 420.0), 10.0);
        assert!(approx(v.scale, 2.0));
        assert!(approx(v.tx, 110.0));
        assert!(approx(v.ty, 10.0));
    }

    #[test]
    fn exact_fit() {
        let v = fit((100.0, 50.0), (120.0, 70.0), 10.0);
        assert!(approx(v.scale, 1.0));
        assert!(approx(v.tx, 10.0));
        assert!(approx(v.ty, 10.0));
    }

    #[test]
    fn tiny_icon_scales_up() {
        let v = fit((24.0, 24.0), (1024.0, 768.0), 16.0);
        assert!(approx(v.scale, (768.0 - 32.0) / 24.0));
        assert!(approx(v.ty, 16.0));
        assert!(approx(v.tx, (1024.0 - 24.0 * v.scale) / 2.0));
    }

    #[test]
    fn viewport_smaller_than_margin_drops_margin() {
        let v = fit((10.0, 10.0), (20.0, 20.0), 16.0);
        assert!(approx(v.scale, 2.0));
        assert!(approx(v.tx, 0.0));
        assert!(approx(v.ty, 0.0));
    }

    #[test]
    fn zero_or_invalid_content_is_identity() {
        assert_eq!(fit((0.0, 10.0), (100.0, 100.0), 0.0), View::default());
        assert_eq!(fit((f32::NAN, 10.0), (100.0, 100.0), 0.0), View::default());
        assert_eq!(fit((10.0, 10.0), (0.0, 100.0), 0.0), View::default());
    }
}
