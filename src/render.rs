//! The render worker thread, plus the cheap preview blit used while the view moves.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg;
use winit::event_loop::EventLoopProxy;

use crate::AppEvent;
use crate::view::View;

/// What fills the area behind the picture. Colors are 0x00RRGGBB.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fill {
    Solid(u32),
    /// Squares of `cell` pixels, lined up with the picture's top left corner.
    Checker { light: u32, dark: u32, cell: u32 },
}

/// The window color around the picture, and the fill behind it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Backdrop {
    pub canvas: u32,
    pub picture: Fill,
}

/// A request to draw `tree` with `view` into a `width` × `height` window.
pub struct Job {
    pub generation: u64,
    pub doc: u64,
    pub tree: Arc<usvg::Tree>,
    /// Maps tree coordinates to picture coordinates.
    pub base: Transform,
    /// The picture's size in CSS pixels, so the backdrop knows where it is.
    pub picture_size: (f32, f32),
    pub view: View,
    pub width: u32,
    pub height: u32,
    pub backdrop: Backdrop,
}

/// A finished render: window pixels as 0x00RRGGBB, for the view it was drawn at.
pub struct Frame {
    pub generation: u64,
    pub doc: u64,
    pub view: View,
    pub width: u32,
    pub height: u32,
    pub backdrop: Backdrop,
    pub pixels: Vec<u32>,
    pub took: Duration,
}

pub struct Renderer {
    jobs: Sender<Job>,
}

impl Renderer {
    pub fn spawn(proxy: EventLoopProxy<AppEvent>) -> std::io::Result<Renderer> {
        let (jobs, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("render".into())
            .spawn(move || worker(rx, proxy))?;
        Ok(Renderer { jobs })
    }

    pub fn submit(&self, job: Job) {
        if self.jobs.send(job).is_err() {
            eprintln!("render thread has stopped");
        }
    }
}

fn worker(rx: Receiver<Job>, proxy: EventLoopProxy<AppEvent>) {
    while let Ok(mut job) = rx.recv() {
        // Only the newest job matters; older ones are already out of date.
        while let Ok(newer) = rx.try_recv() {
            job = newer;
        }
        let Some(frame) = render(&job) else {
            eprintln!("could not allocate a {}x{} render", job.width, job.height);
            continue;
        };
        if proxy.send_event(AppEvent::Rendered(frame)).is_err() {
            break; // The event loop has exited.
        }
    }
}

fn render(job: &Job) -> Option<Frame> {
    let start = Instant::now();
    let mut pixmap = Pixmap::new(job.width, job.height)?;
    let mut backdrop = vec![0; job.width as usize * job.height as usize];
    let (w, h) = job.picture_size;
    let picture = (job.view.tx, job.view.ty, w * job.view.scale, h * job.view.scale);
    paint_backdrop(&mut backdrop, (job.width, job.height), picture, job.backdrop);
    for (dst, src) in pixmap.data_mut().as_chunks_mut::<4>().0.iter_mut().zip(&backdrop) {
        let [_, r, g, b] = src.to_be_bytes();
        *dst = [r, g, b, 255];
    }
    let transform = job.view.transform().pre_concat(job.base);
    resvg::render(&job.tree, transform, &mut pixmap.as_mut());

    // The background is opaque, so premultiplied RGBA is already plain RGB.
    let (rgba, _) = pixmap.data().as_chunks::<4>();
    let pixels = rgba
        .iter()
        .map(|&[r, g, b, _]| (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b))
        .collect();
    Some(Frame {
        generation: job.generation,
        doc: job.doc,
        view: job.view,
        width: job.width,
        height: job.height,
        backdrop: job.backdrop,
        pixels,
        took: start.elapsed(),
    })
}

/// Fills `pixels` with the canvas color, and the `picture` rectangle
/// (x, y, width, height in window pixels) with the picture fill.
fn paint_backdrop(pixels: &mut [u32], size: (u32, u32), picture: (f32, f32, f32, f32), backdrop: Backdrop) {
    pixels.fill(backdrop.canvas);
    let (width, height) = (size.0 as usize, size.1 as usize);
    if width == 0 || pixels.len() < width * height {
        return;
    }
    let (x, y, w, h) = picture;
    if !(x.is_finite() && y.is_finite() && w.is_finite() && h.is_finite()) {
        return;
    }
    let clamp = |v: f32, max: usize| v.round().clamp(0.0, max as f32) as usize;
    let (x0, x1) = (clamp(x, width), clamp(x + w, width));
    let (y0, y1) = (clamp(y, height), clamp(y + h, height));
    let origin = (x.round() as i64, y.round() as i64);
    for (row_index, row) in pixels.chunks_exact_mut(width).enumerate().take(y1).skip(y0) {
        let row = &mut row[x0..x1];
        match backdrop.picture {
            Fill::Solid(color) => row.fill(color),
            Fill::Checker { light, dark, cell } => {
                let cell = i64::from(cell.max(1));
                let cy = (row_index as i64 - origin.1).div_euclid(cell);
                for (i, pixel) in row.iter_mut().enumerate() {
                    let cx = ((x0 + i) as i64 - origin.0).div_euclid(cell);
                    *pixel = if (cx + cy) % 2 == 0 { light } else { dark };
                }
            }
        }
    }
}

/// Draws `src` (rendered at view `from`) into `dst` as it would look at view
/// `to`, using nearest-neighbor sampling. Pixels that `src` doesn't cover get
/// `background`.
pub fn blit_preview(
    src: &[u32],
    src_size: (u32, u32),
    from: View,
    dst: &mut [u32],
    dst_size: (u32, u32),
    to: View,
    background: u32,
) {
    let (sw, sh) = (src_size.0 as usize, src_size.1 as usize);
    let (dw, dh) = (dst_size.0 as usize, dst_size.1 as usize);
    if src.len() < sw * sh || dst.len() < dw * dh || from.scale <= 0.0 {
        dst.fill(background);
        return;
    }

    // A point drawn at `p` in the source is at `p * k + offset` in the destination.
    let k = to.scale / from.scale;
    let offset = (to.tx - from.tx * k, to.ty - from.ty * k);
    let source_index = |d: usize, offset: f32, len: usize| -> Option<usize> {
        let s = ((d as f32 + 0.5 - offset) / k).floor();
        (s >= 0.0 && s < len as f32).then_some(s as usize)
    };

    let columns: Vec<Option<usize>> = (0..dw).map(|x| source_index(x, offset.0, sw)).collect();
    for (y, row) in dst.chunks_exact_mut(dw).take(dh).enumerate() {
        let Some(sy) = source_index(y, offset.1, sh) else {
            row.fill(background);
            continue;
        };
        let src_row = &src[sy * sw..(sy + 1) * sw];
        for (pixel, column) in row.iter_mut().zip(&columns) {
            *pixel = column.map_or(background, |sx| src_row[sx]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BG: u32 = 0xEEEEEE;

    /// A 4x2 source where each pixel holds its own index.
    fn source() -> Vec<u32> {
        (0..8).collect()
    }

    fn view(scale: f32, tx: f32, ty: f32) -> View {
        View { scale, tx, ty }
    }

    #[test]
    fn same_view_copies() {
        let mut dst = vec![0; 8];
        blit_preview(&source(), (4, 2), view(1.0, 0.0, 0.0), &mut dst, (4, 2), view(1.0, 0.0, 0.0), BG);
        assert_eq!(dst, source());
    }

    #[test]
    fn pan_shifts_and_fills_background() {
        let mut dst = vec![0; 8];
        let from = view(2.0, 10.0, 10.0);
        blit_preview(&source(), (4, 2), from, &mut dst, (4, 2), from.pan(1.0, 1.0), BG);
        assert_eq!(dst, vec![BG, BG, BG, BG, BG, 0, 1, 2]);
    }

    #[test]
    fn zoom_in_2x_repeats_pixels() {
        let mut dst = vec![0; 8];
        blit_preview(&source(), (4, 2), view(1.0, 0.0, 0.0), &mut dst, (4, 2), view(2.0, 0.0, 0.0), BG);
        assert_eq!(dst, vec![0, 0, 1, 1, 0, 0, 1, 1]);
    }

    #[test]
    fn zoom_out_2x_samples_and_fills_background() {
        let mut dst = vec![0; 8];
        blit_preview(&source(), (4, 2), view(1.0, 0.0, 0.0), &mut dst, (4, 2), view(0.5, 0.0, 0.0), BG);
        // Each destination pixel samples the center of a 2x2 source block.
        assert_eq!(dst, vec![5, 7, BG, BG, BG, BG, BG, BG]);
    }

    #[test]
    fn different_window_size() {
        let mut dst = vec![0; 6];
        blit_preview(&source(), (4, 2), view(1.0, 0.0, 0.0), &mut dst, (2, 3), view(1.0, 0.0, 0.0), BG);
        assert_eq!(dst, vec![0, 1, 4, 5, BG, BG]);
    }

    #[test]
    fn backdrop_fills_canvas_and_picture() {
        let backdrop = Backdrop { canvas: 1, picture: Fill::Solid(2) };
        let mut pixels = vec![0; 16];
        paint_backdrop(&mut pixels, (4, 4), (1.0, 1.0, 2.0, 2.0), backdrop);
        assert_eq!(pixels, vec![1, 1, 1, 1, 1, 2, 2, 1, 1, 2, 2, 1, 1, 1, 1, 1]);
    }

    #[test]
    fn checker_lines_up_with_the_picture_and_clips() {
        let backdrop = Backdrop { canvas: 0, picture: Fill::Checker { light: 1, dark: 2, cell: 1 } };
        let mut pixels = vec![9; 9];
        // The picture starts above and left of the window.
        paint_backdrop(&mut pixels, (3, 3), (-1.0, -1.0, 3.0, 3.0), backdrop);
        assert_eq!(pixels, vec![1, 2, 0, 2, 1, 0, 0, 0, 0]);
    }

    #[test]
    fn short_buffers_fill_background() {
        let mut dst = vec![0; 8];
        blit_preview(&[1, 2], (4, 2), view(1.0, 0.0, 0.0), &mut dst, (4, 2), view(1.0, 0.0, 0.0), BG);
        assert_eq!(dst, vec![BG; 8]);
    }
}
