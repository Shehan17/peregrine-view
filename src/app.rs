//! App state and window event handling.

use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::Instant;

use resvg::tiny_skia::{Color, Pixmap};
use softbuffer::{Context, Surface};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

use crate::AppEvent;
use crate::loader::Document;
use crate::view::{self, View};

const APP_NAME: &str = "PeregrineView";
/// Space around the picture in fit mode, in logical pixels.
const FIT_MARGIN: f64 = 16.0;
const BACKGROUND: (u8, u8, u8) = (0xF4, 0xF4, 0xF4);

struct Gfx {
    window: Rc<Window>,
    surface: Surface<Rc<Window>, Rc<Window>>,
}

pub struct App {
    started: Instant,
    gfx: Option<Gfx>,
    doc: Option<Document>,
    view: View,
    fit_mode: bool,
    first_frame_shown: bool,
}

impl App {
    pub fn new(started: Instant) -> Self {
        Self {
            started,
            gfx: None,
            doc: None,
            view: View::default(),
            fit_mode: true,
            first_frame_shown: false,
        }
    }

    fn refit(&mut self) {
        let (Some(gfx), Some(doc)) = (&self.gfx, &self.doc) else { return };
        let size = gfx.window.inner_size();
        let margin = (FIT_MARGIN * gfx.window.scale_factor()) as f32;
        let content = doc.tree.size();
        self.view = view::fit(
            (content.width(), content.height()),
            (size.width as f32, size.height as f32),
            margin,
        );
    }

    fn redraw(&mut self) {
        let Some(gfx) = &mut self.gfx else { return };
        let size = gfx.window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return; // Minimized.
        };

        let start = Instant::now();
        let Some(mut pixmap) = Pixmap::new(w.get(), h.get()) else {
            eprintln!("could not allocate a {w}x{h} pixmap");
            return;
        };
        let (r, g, b) = BACKGROUND;
        pixmap.fill(Color::from_rgba8(r, g, b, 255));
        if let Some(doc) = &self.doc {
            resvg::render(&doc.tree, self.view.transform(), &mut pixmap.as_mut());
        }
        let rendered = start.elapsed();

        if let Err(e) = gfx.surface.resize(w, h) {
            eprintln!("surface resize failed: {e}");
            return;
        }
        let mut buffer = match gfx.surface.buffer_mut() {
            Ok(buffer) => buffer,
            Err(e) => {
                eprintln!("could not get window buffer: {e}");
                return;
            }
        };
        // The background is opaque, so premultiplied RGBA is already plain RGB.
        let (pixels, _) = pixmap.data().as_chunks::<4>();
        for (dst, &[r, g, b, _]) in buffer.iter_mut().zip(pixels) {
            *dst = (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
        }
        gfx.window.pre_present_notify();
        if let Err(e) = buffer.present() {
            eprintln!("present failed: {e}");
            return;
        }

        eprintln!("frame {w}x{h}: render {rendered:?}, total {:?}", start.elapsed());
        if !self.first_frame_shown && self.doc.is_some() {
            self.first_frame_shown = true;
            eprintln!("first picture {:?} after start", self.started.elapsed());
        }
    }
}

impl ApplicationHandler<AppEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title(APP_NAME)
            .with_inner_size(LogicalSize::new(1024.0, 768.0));
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Rc::new(window),
            Err(e) => {
                eprintln!("could not create window: {e}");
                event_loop.exit();
                return;
            }
        };
        let surface = Context::new(window.clone())
            .and_then(|context| Surface::new(&context, window.clone()));
        let surface = match surface {
            Ok(surface) => surface,
            Err(e) => {
                eprintln!("could not create drawing surface: {e}");
                event_loop.exit();
                return;
            }
        };
        if let Some(doc) = &self.doc {
            window.set_title(&format!("{} – {APP_NAME}", doc.file_name));
        }
        self.gfx = Some(Gfx { window, surface });
        eprintln!("window created {:?} after start", self.started.elapsed());
        self.refit();
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::Loaded(Ok(doc)) => {
                eprintln!("showing {}", doc.path.display());
                if let Some(gfx) = &self.gfx {
                    gfx.window.set_title(&format!("{} – {APP_NAME}", doc.file_name));
                }
                self.doc = Some(doc);
                self.fit_mode = true;
                self.refit();
                if let Some(gfx) = &self.gfx {
                    gfx.window.request_redraw();
                }
            }
            // The previous picture stays on screen. The error banner comes in milestone 4.
            AppEvent::Loaded(Err(e)) => eprintln!("error: {e}"),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                if self.fit_mode {
                    self.refit();
                }
                if let Some(gfx) = &self.gfx {
                    gfx.window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }
}
