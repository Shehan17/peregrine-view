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

/// Largest zoom, relative to 100% (one SVG unit per logical pixel).
const MAX_ZOOM: f32 = 512.0;
/// Smallest zoom, relative to the fit scale.
const MIN_ZOOM_OF_FIT: f32 = 1.0 / 16.0;
/// The picture is never drawn larger than this on its longest side, in physical pixels.
const MAX_SIDE_PX: f32 = 64_000.0;

/// The allowed range for `View::scale`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    pub min: f32,
    pub max: f32,
}

/// Zoom limits for `content` (SVG units) given the current fit scale and the
/// window scale factor. The fit scale itself is always allowed.
pub fn limits(content: (f32, f32), fit_scale: f32, scale_factor: f32) -> Limits {
    let longest = content.0.max(content.1);
    let mut max = MAX_ZOOM * scale_factor;
    if longest.is_finite() && longest > 0.0 {
        max = max.min(MAX_SIDE_PX / longest);
    }
    let max = max.max(fit_scale);
    let min = (fit_scale * MIN_ZOOM_OF_FIT).min(max);
    Limits { min, max }
}

impl View {
    pub fn transform(&self) -> Transform {
        Transform::from_row(self.scale, 0.0, 0.0, self.scale, self.tx, self.ty)
    }

    /// Multiplies the scale by `factor`, clamped to `limits`, keeping the
    /// screen point `anchor` over the same spot of the picture.
    pub fn zoom_at(self, factor: f32, anchor: (f32, f32), limits: Limits) -> View {
        self.zoom_to(self.scale * factor, anchor, limits)
    }

    /// Sets the scale to `scale`, clamped to `limits`, keeping the screen
    /// point `anchor` over the same spot of the picture.
    pub fn zoom_to(self, scale: f32, anchor: (f32, f32), limits: Limits) -> View {
        let scale = scale.clamp(limits.min, limits.max);
        if !scale.is_finite() || scale <= 0.0 || self.scale <= 0.0 {
            return self;
        }
        let k = scale / self.scale;
        View {
            scale,
            tx: anchor.0 - (anchor.0 - self.tx) * k,
            ty: anchor.1 - (anchor.1 - self.ty) * k,
        }
    }

    /// Moves the picture by `(dx, dy)` screen pixels.
    pub fn pan(self, dx: f32, dy: f32) -> View {
        if !(dx.is_finite() && dy.is_finite()) {
            return self;
        }
        View { tx: self.tx + dx, ty: self.ty + dy, ..self }
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

    const WIDE: Limits = Limits { min: 0.001, max: 1000.0 };

    /// The screen position of SVG point `p` under view `v`.
    fn to_screen(v: View, p: (f32, f32)) -> (f32, f32) {
        (p.0 * v.scale + v.tx, p.1 * v.scale + v.ty)
    }

    /// The SVG point under screen point `s` in view `v`.
    fn to_svg(v: View, s: (f32, f32)) -> (f32, f32) {
        ((s.0 - v.tx) / v.scale, (s.1 - v.ty) / v.scale)
    }

    #[test]
    fn zoom_keeps_anchor_still() {
        let v = View { scale: 1.5, tx: 30.0, ty: -20.0 };
        let anchor = (123.0, 456.0);
        let under = to_svg(v, anchor);
        for factor in [2.0, 0.5, 1.2, 1.0 / 1.2] {
            let z = v.zoom_at(factor, anchor, WIDE);
            assert!(approx(z.scale, v.scale * factor));
            let s = to_screen(z, under);
            assert!(approx(s.0, anchor.0) && approx(s.1, anchor.1));
        }
    }

    #[test]
    fn zoom_in_then_out_returns_to_start() {
        let v = View { scale: 2.0, tx: 10.0, ty: 20.0 };
        let back = v.zoom_at(2.0, (300.0, 200.0), WIDE).zoom_at(0.5, (300.0, 200.0), WIDE);
        assert!(approx(back.scale, v.scale) && approx(back.tx, v.tx) && approx(back.ty, v.ty));
    }

    #[test]
    fn zoom_clamps_and_still_keeps_anchor_still() {
        let limits = Limits { min: 0.5, max: 4.0 };
        let v = View { scale: 2.0, tx: 0.0, ty: 0.0 };
        let anchor = (100.0, 50.0);
        let under = to_svg(v, anchor);

        let z = v.zoom_at(100.0, anchor, limits);
        assert!(approx(z.scale, 4.0));
        let s = to_screen(z, under);
        assert!(approx(s.0, anchor.0) && approx(s.1, anchor.1));

        assert!(approx(v.zoom_at(0.001, anchor, limits).scale, 0.5));
    }

    #[test]
    fn zoom_ignores_bad_factors() {
        let v = View { scale: 2.0, tx: 5.0, ty: 6.0 };
        assert_eq!(v.zoom_at(f32::NAN, (1.0, 1.0), WIDE), v);
        assert_eq!(v.zoom_at(f32::INFINITY, (1.0, 1.0), Limits { min: 0.1, max: f32::INFINITY }), v);
    }

    #[test]
    fn zoom_to_sets_exact_scale() {
        let v = View { scale: 3.7, tx: 1.0, ty: 2.0 };
        assert!(approx(v.zoom_to(1.25, (0.0, 0.0), WIDE).scale, 1.25));
    }

    #[test]
    fn pan_moves_by_delta() {
        let v = View { scale: 2.0, tx: 1.0, ty: 2.0 }.pan(10.0, -5.0);
        assert_eq!(v, View { scale: 2.0, tx: 11.0, ty: -3.0 });
        assert_eq!(v.pan(f32::NAN, 0.0), v);
    }

    #[test]
    fn limits_follow_the_spec() {
        // Small icon: 512x of 100% (at scale factor 2) is the cap.
        let l = limits((24.0, 24.0), 30.0, 2.0);
        assert!(approx(l.max, 1024.0));
        assert!(approx(l.min, 30.0 / 16.0));

        // Large drawing: the 64,000 px longest side is the cap.
        let l = limits((4000.0, 1000.0), 0.25, 1.0);
        assert!(approx(l.max, 16.0));
        assert!(approx(l.min, 0.25 / 16.0));
    }

    #[test]
    fn limits_always_allow_fit() {
        let l = limits((100_000.0, 10.0), 2.0, 1.0);
        assert!(l.max >= 2.0);
        assert!(l.min <= l.max);
    }

    #[test]
    fn zero_or_invalid_content_is_identity() {
        assert_eq!(fit((0.0, 10.0), (100.0, 100.0), 0.0), View::default());
        assert_eq!(fit((f32::NAN, 10.0), (100.0, 100.0), 0.0), View::default());
        assert_eq!(fit((10.0, 10.0), (0.0, 100.0), 0.0), View::default());
    }
}
