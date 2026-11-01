//! The render worker thread, plus the cheap preview blit used while the view moves.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use resvg::tiny_skia::{Color, Pixmap, Transform};
use resvg::usvg;
use winit::event_loop::EventLoopProxy;

use crate::AppEvent;
use crate::view::View;

/// A request to draw `tree` with `view` into a `width` × `height` window.
pub struct Job {
    pub generation: u64,
    pub doc: u64,
    pub tree: Arc<usvg::Tree>,
    /// Maps tree coordinates to picture coordinates.
    pub base: Transform,
    pub view: View,
    pub width: u32,
    pub height: u32,
    /// Opaque background as 0x00RRGGBB.
    pub background: u32,
}

/// A finished render: window pixels as 0x00RRGGBB, for the view it was drawn at.
pub struct Frame {
    pub generation: u64,
    pub doc: u64,
    pub view: View,
    pub width: u32,
    pub height: u32,
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
    let [_, r, g, b] = job.background.to_be_bytes();
    pixmap.fill(Color::from_rgba8(r, g, b, 255));
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
        pixels,
        took: start.elapsed(),
    })
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
    fn short_buffers_fill_background() {
        let mut dst = vec![0; 8];
        blit_preview(&[1, 2], (4, 2), view(1.0, 0.0, 0.0), &mut dst, (4, 2), view(1.0, 0.0, 0.0), BG);
        assert_eq!(dst, vec![BG; 8]);
    }
}
