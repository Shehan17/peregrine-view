//! App state and window event handling.

use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::{Duration, Instant};

use resvg::tiny_skia::{Color, Pixmap};
use softbuffer::{Context, Surface};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

use crate::AppEvent;
use crate::loader::Document;
use crate::view::{self, Limits, View};

const APP_NAME: &str = "PeregrineView";
/// Space around the picture in fit mode, in logical pixels.
const FIT_MARGIN: f64 = 16.0;
const BACKGROUND: (u8, u8, u8) = (0xF4, 0xF4, 0xF4);
/// Zoom per mouse wheel notch.
const WHEEL_ZOOM: f32 = 1.2;
/// Zoom per `+` or `-` key press.
const KEY_ZOOM: f32 = 1.25;
/// How far the arrow keys move the view, in logical pixels.
const ARROW_STEP: f64 = 60.0;
/// Two clicks closer than this in time and distance make a double-click.
const DOUBLE_CLICK_TIME: Duration = Duration::from_millis(500);
const DOUBLE_CLICK_DISTANCE: f32 = 4.0;

struct Gfx {
    window: Rc<Window>,
    surface: Surface<Rc<Window>, Rc<Window>>,
}

pub struct App {
    started: Instant,
    gfx: Option<Gfx>,
    doc: Option<Document>,
    view: View,
    /// While true, the picture refits whenever the window changes size.
    fit_mode: bool,
    first_frame_shown: bool,
    /// Last known inner size, in physical pixels.
    viewport: PhysicalSize<u32>,
    scale_factor: f64,
    cursor: (f32, f32),
    modifiers: ModifiersState,
    /// Cursor position at the last drag step, while a drag is in progress.
    drag_from: Option<(f32, f32)>,
    last_click: Option<(Instant, (f32, f32))>,
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
            viewport: PhysicalSize::new(0, 0),
            scale_factor: 1.0,
            cursor: (0.0, 0.0),
            modifiers: ModifiersState::empty(),
            drag_from: None,
            last_click: None,
        }
    }

    fn request_redraw(&self) {
        if let Some(gfx) = &self.gfx {
            gfx.window.request_redraw();
        }
    }

    fn content_size(&self) -> Option<(f32, f32)> {
        let size = self.doc.as_ref()?.tree.size();
        Some((size.width(), size.height()))
    }

    fn center(&self) -> (f32, f32) {
        (self.viewport.width as f32 / 2.0, self.viewport.height as f32 / 2.0)
    }

    fn fit_view(&self) -> Option<View> {
        let margin = (FIT_MARGIN * self.scale_factor) as f32;
        let viewport = (self.viewport.width as f32, self.viewport.height as f32);
        Some(view::fit(self.content_size()?, viewport, margin))
    }

    fn limits(&self) -> Option<Limits> {
        let fit = self.fit_view()?;
        Some(view::limits(self.content_size()?, fit.scale, self.scale_factor as f32))
    }

    fn refit(&mut self) {
        if let Some(fit) = self.fit_view() {
            self.view = fit;
            self.fit_mode = true;
            self.request_redraw();
        }
    }

    /// Applies a zoom or move chosen by the user, which leaves fit mode.
    fn change_view(&mut self, change: impl FnOnce(View, Limits) -> View) {
        let Some(limits) = self.limits() else { return };
        let view = change(self.view, limits);
        if view != self.view {
            self.view = view;
            self.fit_mode = false;
            self.request_redraw();
        }
    }

    fn zoom_at(&mut self, factor: f32, anchor: (f32, f32)) {
        self.change_view(|v, l| v.zoom_at(factor, anchor, l));
    }

    fn pan(&mut self, dx: f32, dy: f32) {
        self.change_view(|v, _| v.pan(dx, dy));
    }

    /// 100% means one SVG unit per logical pixel.
    fn actual_size_scale(&self) -> f32 {
        self.scale_factor as f32
    }

    /// Switches between 100% and fit.
    fn toggle_actual_size(&mut self) {
        let actual = self.actual_size_scale();
        if !self.fit_mode && (self.view.scale - actual).abs() <= actual * 1e-3 {
            self.refit();
        } else {
            let center = self.center();
            self.change_view(|v, l| v.zoom_to(actual, center, l));
        }
    }

    fn on_resized(&mut self, size: PhysicalSize<u32>) {
        let old = self.viewport;
        self.viewport = size;
        if self.fit_mode {
            self.refit();
        } else {
            // Keep the same part of the picture in the middle of the window.
            let dx = (size.width as f32 - old.width as f32) / 2.0;
            let dy = (size.height as f32 - old.height as f32) / 2.0;
            self.view = self.view.pan(dx, dy);
            self.request_redraw();
        }
    }

    fn on_scale_factor_changed(&mut self, scale_factor: f64) {
        let ratio = (scale_factor / self.scale_factor) as f32;
        self.scale_factor = scale_factor;
        if !self.fit_mode {
            // Keep the same zoom percentage. Bypass the limits, which depend
            // on the new window size that arrives with the next Resized.
            let center = self.center();
            let wide = Limits { min: 0.0, max: f32::INFINITY };
            self.view = self.view.zoom_at(ratio, center, wide);
        }
    }

    fn on_wheel(&mut self, delta: MouseScrollDelta) {
        let zoom_modifier = self.modifiers.control_key() || self.modifiers.super_key();
        match delta {
            // A mouse wheel. On Windows, trackpads also arrive as LineDelta.
            MouseScrollDelta::LineDelta(_, y) => {
                self.zoom_at(WHEEL_ZOOM.powf(y), self.cursor);
            }
            // A trackpad: scroll moves the picture, Ctrl/Cmd + scroll zooms.
            MouseScrollDelta::PixelDelta(PhysicalPosition { x, y }) => {
                if zoom_modifier {
                    self.zoom_at((y as f32 * 0.005).exp(), self.cursor);
                } else {
                    self.pan(x as f32, y as f32);
                }
            }
        }
    }

    fn on_mouse_button(&mut self, state: ElementState, button: MouseButton) {
        match (button, state) {
            (MouseButton::Left, ElementState::Pressed) => {
                let now = Instant::now();
                let is_double = self.last_click.is_some_and(|(time, pos)| {
                    now.duration_since(time) <= DOUBLE_CLICK_TIME
                        && (pos.0 - self.cursor.0).hypot(pos.1 - self.cursor.1)
                            <= DOUBLE_CLICK_DISTANCE
                });
                if is_double {
                    self.last_click = None;
                    let factor = if self.modifiers.shift_key() { 0.5 } else { 2.0 };
                    self.zoom_at(factor, self.cursor);
                } else {
                    self.last_click = Some((now, self.cursor));
                }
                self.drag_from = Some(self.cursor);
            }
            (MouseButton::Middle, ElementState::Pressed) => self.drag_from = Some(self.cursor),
            (MouseButton::Left | MouseButton::Middle, ElementState::Released) => {
                self.drag_from = None;
            }
            _ => {}
        }
    }

    fn on_cursor_moved(&mut self, position: PhysicalPosition<f64>) {
        self.cursor = (position.x as f32, position.y as f32);
        if let Some(from) = self.drag_from {
            self.drag_from = Some(self.cursor);
            self.pan(self.cursor.0 - from.0, self.cursor.1 - from.1);
        }
    }

    fn on_key(&mut self, event: &KeyEvent) {
        if event.state != ElementState::Pressed {
            return;
        }
        let m = self.modifiers;
        if m.control_key() || m.alt_key() || m.super_key() {
            return;
        }
        let step = (ARROW_STEP * self.scale_factor) as f32;
        let center = self.center();
        match &event.logical_key {
            Key::Character(c) => match c.as_str() {
                "+" | "=" => self.zoom_at(KEY_ZOOM, center),
                "-" => self.zoom_at(1.0 / KEY_ZOOM, center),
                "0" | "f" | "F" => self.refit(),
                "1" => self.toggle_actual_size(),
                _ => {}
            },
            // The arrows move the view, so the picture moves the other way.
            Key::Named(NamedKey::ArrowLeft) => self.pan(step, 0.0),
            Key::Named(NamedKey::ArrowRight) => self.pan(-step, 0.0),
            Key::Named(NamedKey::ArrowUp) => self.pan(0.0, step),
            Key::Named(NamedKey::ArrowDown) => self.pan(0.0, -step),
            _ => {}
        }
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

        let zoom = self.view.scale / self.actual_size_scale() * 100.0;
        eprintln!("frame {w}x{h} at {zoom:.0}%: render {rendered:?}, total {:?}", start.elapsed());
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
        self.viewport = window.inner_size();
        self.scale_factor = window.scale_factor();
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
                self.refit();
            }
            // The previous picture stays on screen. The error banner comes in milestone 4.
            AppEvent::Loaded(Err(e)) => eprintln!("error: {e}"),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => self.on_resized(size),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.on_scale_factor_changed(scale_factor);
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::CursorMoved { position, .. } => self.on_cursor_moved(position),
            // A release can be missed while another window has focus.
            WindowEvent::Focused(false) => self.drag_from = None,
            WindowEvent::MouseInput { state, button, .. } => self.on_mouse_button(state, button),
            WindowEvent::MouseWheel { delta, .. } => self.on_wheel(delta),
            WindowEvent::PinchGesture { delta, .. } if delta.is_finite() => {
                self.zoom_at((1.0 + delta as f32).max(0.01), self.cursor);
            }
            WindowEvent::KeyboardInput { event, .. } => self.on_key(&event),
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }
}
