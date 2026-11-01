//! App state and window event handling.

use std::num::NonZeroU32;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use softbuffer::{Context, Surface};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::{Window, WindowId};

use crate::AppEvent;
use crate::loader::{self, Document, Source};
use crate::render::{self, Frame, Job, Renderer};
use crate::text::TextRenderer;
use crate::ui::Banner;
use crate::view::{self, Limits, View};

const APP_NAME: &str = "PeregrineView";
/// Space around the picture in fit mode, in logical pixels.
const FIT_MARGIN: f64 = 16.0;
const BACKGROUND: u32 = 0xF4F4F4;
/// How long input must stop before the sharp render starts.
const SETTLE_DELAY: Duration = Duration::from_millis(100);
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
    renderer: Renderer,
    proxy: EventLoopProxy<AppEvent>,
    /// Counts started loads, so a slower, older load can't replace a newer one.
    load_id: u64,
    text: Option<TextRenderer>,
    banner: Option<Banner>,
    dialog_open: bool,
    doc: Option<Document>,
    /// Counts opened documents, so renders of an older one can be dropped.
    doc_id: u64,
    /// The last sharp render. While the view moves it is shown scaled and shifted.
    frame: Option<Frame>,
    /// Counts submitted render jobs.
    generation: u64,
    /// The newest job sent to the render thread, until its result arrives.
    in_flight: Option<(u64, Target)>,
    /// When to ask for a sharp render, once input has stopped.
    settle_at: Option<Instant>,
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
    pub fn new(
        started: Instant,
        renderer: Renderer,
        proxy: EventLoopProxy<AppEvent>,
        path: Option<PathBuf>,
    ) -> Self {
        let mut app = Self {
            started,
            gfx: None,
            renderer,
            proxy,
            load_id: 0,
            text: None,
            banner: None,
            dialog_open: false,
            doc: None,
            doc_id: 0,
            frame: None,
            generation: 0,
            in_flight: None,
            settle_at: None,
            view: View::default(),
            fit_mode: true,
            first_frame_shown: false,
            viewport: PhysicalSize::new(0, 0),
            scale_factor: 1.0,
            cursor: (0.0, 0.0),
            modifiers: ModifiersState::empty(),
            drag_from: None,
            last_click: None,
        };
        if let Some(path) = path {
            app.open(Source::File(path));
        }
        app
    }

    fn open(&mut self, source: Source) {
        self.load_id += 1;
        loader::spawn_load(self.load_id, source, self.proxy.clone());
    }

    fn show_error(&mut self, message: String) {
        eprintln!("error: {message}");
        self.banner = Some(Banner::new(message));
        self.request_redraw();
    }

    fn open_dialog(&mut self) {
        let Some(gfx) = &self.gfx else { return };
        if self.dialog_open {
            return;
        }
        let mut dialog = rfd::FileDialog::new()
            .set_title("Open SVG")
            .add_filter("SVG images", &["svg", "svgz"])
            .add_filter("All files", &["*"])
            .set_parent(&*gfx.window);
        if let Some(folder) = self.doc.as_ref().and_then(|d| d.path.as_ref()?.parent()) {
            dialog = dialog.set_directory(folder);
        }
        // The dialog blocks its thread until it closes, so it gets its own.
        let proxy = self.proxy.clone();
        let spawned = std::thread::Builder::new()
            .name("open-dialog".into())
            .spawn(move || {
                let _ = proxy.send_event(AppEvent::DialogClosed(dialog.pick_file()));
            });
        match spawned {
            Ok(_) => self.dialog_open = true,
            Err(e) => eprintln!("could not start dialog thread: {e}"),
        }
    }

    /// Opens a copied file, a copied path, or copied SVG code.
    fn paste(&mut self) {
        let mut clipboard = match arboard::Clipboard::new() {
            Ok(clipboard) => clipboard,
            Err(e) => {
                eprintln!("clipboard: {e}");
                self.show_error("The clipboard can't be read.".into());
                return;
            }
        };
        if let Some(path) = clipboard.get().file_list().ok().and_then(|files| files.into_iter().next()) {
            self.open(Source::File(path));
            return;
        }
        if let Ok(text) = clipboard.get_text() {
            if text.contains("<svg") {
                self.open(Source::Text(text));
                return;
            }
            let path = PathBuf::from(text.trim().trim_matches('"'));
            if path.is_file() {
                self.open(Source::File(path));
                return;
            }
        }
        self.show_error("The clipboard has no SVG.".into());
    }

    fn request_redraw(&self) {
        if let Some(gfx) = &self.gfx {
            gfx.window.request_redraw();
        }
    }

    /// Shows a preview now and schedules a sharp render for when input stops.
    fn view_changed(&mut self) {
        // With no picture of this document yet there is nothing to preview,
        // so render right away.
        let has_frame = self.frame.as_ref().is_some_and(|f| f.doc == self.doc_id);
        let delay = if has_frame { SETTLE_DELAY } else { Duration::ZERO };
        self.settle_at = Some(Instant::now() + delay);
        self.request_redraw();
    }

    fn content_size(&self) -> Option<(f32, f32)> {
        self.doc.as_ref().map(|doc| doc.size)
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
            self.view_changed();
        }
    }

    /// Applies a zoom or move chosen by the user, which leaves fit mode.
    fn change_view(&mut self, change: impl FnOnce(View, Limits) -> View) {
        let Some(limits) = self.limits() else { return };
        let view = change(self.view, limits);
        if view != self.view {
            self.view = view;
            self.fit_mode = false;
            self.view_changed();
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
            self.view_changed();
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
            self.view_changed();
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
        // Cmd on macOS, Ctrl elsewhere.
        let primary = if cfg!(target_os = "macos") { m.super_key() } else { m.control_key() };
        if primary && !event.repeat {
            if let Key::Character(c) = event.key_without_modifiers() {
                match c.to_lowercase().as_str() {
                    "o" => self.open_dialog(),
                    "v" => self.paste(),
                    _ => {}
                }
            }
            return;
        }
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

    /// What a sharp render of the current state would be drawn for.
    fn target(&self) -> Target {
        Target {
            doc: self.doc_id,
            view: self.view,
            size: (self.viewport.width, self.viewport.height),
        }
    }

    /// Starts a sharp render of the visible area, unless one is already
    /// on screen or on the way.
    fn request_sharp(&mut self) {
        let Some(doc) = &self.doc else { return };
        let target = self.target();
        if target.size.0 == 0 || target.size.1 == 0 {
            return;
        }
        let shown = self.frame.as_ref().is_some_and(|f| Target::of(f) == target);
        let coming = self.in_flight.is_some_and(|(_, t)| t == target);
        if shown || coming {
            return;
        }
        self.generation += 1;
        self.in_flight = Some((self.generation, target));
        self.renderer.submit(Job {
            generation: self.generation,
            doc: self.doc_id,
            tree: doc.tree.clone(),
            base: doc.base,
            view: self.view,
            width: target.size.0,
            height: target.size.1,
            background: BACKGROUND,
        });
    }

    fn on_rendered(&mut self, frame: Frame) {
        let latest = self.in_flight.is_some_and(|(generation, _)| generation == frame.generation);
        if !latest {
            eprintln!("render dropped: a newer job replaced it");
            return;
        }
        self.in_flight = None;
        eprintln!("render {}x{}: {:?}", frame.width, frame.height, frame.took);

        let current = Target::of(&frame) == self.target();
        // A render for an old view is dropped, unless there is nothing better
        // to preview from, as when the user moves before the first picture.
        let nothing_better = self.frame.as_ref().is_none_or(|f| f.doc != self.doc_id);
        if frame.doc == self.doc_id && (current || nothing_better) {
            self.frame = Some(frame);
            self.request_redraw();
        } else {
            eprintln!("render dropped: the view changed while it was drawn");
        }
        if !current && self.settle_at.is_none() {
            self.settle_at = Some(Instant::now());
        }
    }

    fn redraw(&mut self) {
        let Some(gfx) = &mut self.gfx else { return };
        let size = (self.viewport.width, self.viewport.height);
        let (Some(w), Some(h)) = (NonZeroU32::new(size.0), NonZeroU32::new(size.1)) else {
            return; // Minimized.
        };

        let start = Instant::now();
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
        let target = Target { doc: self.doc_id, view: self.view, size };
        let sharp = match &self.frame {
            Some(f) if Target::of(f) == target && f.pixels.len() == buffer.len() => {
                buffer.copy_from_slice(&f.pixels);
                true
            }
            Some(f) if f.doc == self.doc_id => {
                let from = (f.width, f.height);
                render::blit_preview(&f.pixels, from, f.view, &mut buffer, size, self.view, BACKGROUND);
                false
            }
            _ => {
                buffer.fill(BACKGROUND);
                false
            }
        };
        if let (Some(banner), Some(text)) = (&mut self.banner, &self.text) {
            banner.draw(text, &mut buffer, size, self.scale_factor as f32);
        }
        gfx.window.pre_present_notify();
        if let Err(e) = buffer.present() {
            eprintln!("present failed: {e}");
            return;
        }

        let zoom = self.view.scale / self.actual_size_scale() * 100.0;
        let kind = if sharp { "sharp" } else { "preview" };
        eprintln!("present {kind} {w}x{h} at {zoom:.0}%: {:?}", start.elapsed());
        if sharp && !self.first_frame_shown {
            self.first_frame_shown = true;
            eprintln!("first picture {:?} after start", self.started.elapsed());
        }
    }
}

/// The document, view and window size a frame is drawn for.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Target {
    doc: u64,
    view: View,
    size: (u32, u32),
}

impl Target {
    fn of(frame: &Frame) -> Target {
        Target { doc: frame.doc, view: frame.view, size: (frame.width, frame.height) }
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
            window.set_title(&format!("{} – {APP_NAME}", doc.name));
        }
        self.text = TextRenderer::new();
        self.viewport = window.inner_size();
        self.scale_factor = window.scale_factor();
        self.gfx = Some(Gfx { window, surface });
        eprintln!("window created {:?} after start", self.started.elapsed());
        self.refit();
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::Loaded { id, .. } if id != self.load_id => {
                eprintln!("load dropped: a newer one was started");
            }
            AppEvent::Loaded { result: Ok(doc), .. } => {
                if let Some(gfx) = &self.gfx {
                    gfx.window.set_title(&format!("{} – {APP_NAME}", doc.name));
                }
                // A good file clears any error about the one before.
                self.banner = None;
                self.doc = Some(doc);
                self.doc_id += 1;
                self.frame = None;
                self.refit();
            }
            // The previous picture stays on screen.
            AppEvent::Loaded { result: Err(e), .. } => self.show_error(e.to_string()),
            AppEvent::DialogClosed(path) => {
                self.dialog_open = false;
                if let Some(path) = path {
                    self.open(Source::File(path));
                }
            }
            AppEvent::Rendered(frame) => self.on_rendered(frame),
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if self.settle_at.is_some_and(|at| now >= at) {
            self.settle_at = None;
            self.request_sharp();
        }
        if self.banner.as_ref().is_some_and(|b| now >= b.until) {
            self.banner = None;
            self.request_redraw();
        }
        let next = [self.settle_at, self.banner.as_ref().map(|b| b.until)].into_iter().flatten().min();
        event_loop.set_control_flow(match next {
            Some(at) => ControlFlow::WaitUntil(at),
            None => ControlFlow::Wait,
        });
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::DroppedFile(path) => self.open(Source::File(path)),
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
