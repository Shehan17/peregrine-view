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
use winit::window::{CursorIcon, Fullscreen, Window, WindowId};

use crate::AppEvent;
use crate::icon::IconImages;
use crate::loader::{self, Document, Source};
use crate::platform::Editor;
use crate::platform::trash::{self, TrashError};
use crate::render::{self, Backdrop, Frame, Job, Renderer};
use crate::text::TextRenderer;
use crate::settings::{self, Monitor, Settings, WindowPlace};
use crate::ui::{self, Background, Banner, Button, Canvas, EditScene, Hit, Info, MenuRow, MenuScene, Scene, Theme, Ui};
use crate::watch::{self, FileWatcher};
use crate::view::{self, Limits, View};

const APP_NAME: &str = "PeregrineView";
/// The window's inner size the first time, in logical pixels.
const DEFAULT_SIZE: (u32, u32) = (1024, 768);
/// The confirm button of the question before Delete.
const DELETE_LABEL: &str = "Delete";
/// Space around the picture in fit mode, in logical pixels.
const FIT_MARGIN: f64 = 16.0;
/// Editors often write a file in several steps; wait this long after the
/// last change before reloading.
const RELOAD_DELAY: Duration = Duration::from_millis(200);
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
    /// True when the load with `load_id` reloads the open file, which keeps the view.
    load_is_reload: bool,
    /// True when the load with `load_id` replaces a file that was just
    /// deleted. If it fails, the deleted picture leaves the screen.
    load_after_delete: bool,
    /// After a reload, frames of the old version can still be shown as a
    /// preview until the new one is drawn.
    reload_of: Option<u64>,
    /// When to reload the open file after it changed on disk.
    reload_at: Option<Instant>,
    watcher: Option<FileWatcher>,
    watched: Option<PathBuf>,
    /// Where Next and Previous count from: the last file opened or tried.
    nav_path: Option<PathBuf>,
    /// True while the loader is picking the next or previous file.
    nav_resolving: bool,
    /// Next (+1) and Previous (-1) presses made while picking, done after it.
    nav_queued: i32,
    exit_requested: bool,
    /// The app icon, kept until the window exists if it is ready first.
    icons: Option<IconImages>,
    text: Option<TextRenderer>,
    ui: Ui,
    banner: Option<Banner>,
    theme: Theme,
    background: Background,
    canvas: Canvas,
    /// The toolbar or empty-screen button under the cursor.
    hovered: Option<Button>,
    /// The button the left mouse button went down on, until it comes up.
    pressed: Option<Button>,
    /// True while a file is dragged over the window.
    file_hovering: bool,
    cursor_icon: CursorIcon,
    /// True from opening a file until its load finishes.
    loading: bool,
    dialog_open: bool,
    /// The app the "Open in" button opens files in: the last one picked from its menu.
    editor: Option<Editor>,
    /// The apps that can open files with the extension `.0` (".svg"), once listed.
    editors: Option<(String, Vec<Editor>)>,
    /// True while the app menu under the "Open in" button is open.
    menu_open: bool,
    /// The menu row under the cursor, or picked with the arrow keys.
    menu_highlight: Option<usize>,
    /// The thread that lists apps and opens files in them, started when first needed.
    #[cfg(windows)]
    shell: Option<crate::platform::windows::Shell>,
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
    /// Where the window is, or goes back to from maximized or full screen.
    /// Saved when the app closes, so the window opens there next time.
    place: Option<WindowPlace>,
    /// True when the window moved or changed size since `place` was noted.
    place_changed: bool,
}

impl App {
    pub fn new(
        started: Instant,
        renderer: Renderer,
        proxy: EventLoopProxy<AppEvent>,
        path: Option<PathBuf>,
    ) -> Self {
        let settings = settings::load();
        let mut app = Self {
            started,
            gfx: None,
            renderer,
            proxy,
            load_id: 0,
            load_is_reload: false,
            load_after_delete: false,
            reload_of: None,
            reload_at: None,
            watcher: None,
            watched: None,
            nav_path: None,
            nav_resolving: false,
            nav_queued: 0,
            exit_requested: false,
            icons: None,
            text: None,
            ui: Ui::default(),
            banner: None,
            theme: Theme::Light,
            background: settings.background,
            canvas: settings.canvas,
            hovered: None,
            pressed: None,
            file_hovering: false,
            cursor_icon: CursorIcon::Default,
            loading: false,
            dialog_open: false,
            editor: settings.editor.clone(),
            editors: None,
            menu_open: false,
            menu_highlight: None,
            #[cfg(windows)]
            shell: None,
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
            place: settings.window,
            place_changed: false,
        };
        if let Some(path) = path {
            app.open(Source::File(path));
        }
        app
    }

    fn open(&mut self, source: Source) {
        self.start_load(source, false);
    }

    fn start_load(&mut self, source: Source, reload: bool) {
        // Absolute paths keep the folder for Next, Previous and the watcher.
        let source = match source {
            Source::File(path) => Source::File(std::path::absolute(&path).unwrap_or(path)),
            source => source,
        };
        self.load_id += 1;
        self.load_is_reload = reload;
        self.load_after_delete = false;
        self.nav_resolving = false;
        self.nav_queued = 0;
        self.loading = true;
        loader::spawn_load(self.load_id, source, self.proxy.clone());
    }

    /// Loads the open file again, keeping the zoom and position.
    fn reload(&mut self) {
        self.reload_at = None;
        if let Some(path) = self.doc.as_ref().and_then(|doc| doc.path.clone()) {
            self.start_load(Source::File(path), true);
        }
    }

    /// Opens the next or previous SVG in the folder.
    fn go(&mut self, forward: bool) {
        // Until the loader has picked a file, count presses instead of
        // starting again from the same place.
        if self.nav_resolving {
            self.nav_queued += if forward { 1 } else { -1 };
            return;
        }
        if let Some(from) = self.nav_path.clone() {
            self.open(Source::Neighbor { from, forward });
            self.nav_resolving = true;
        }
    }

    fn on_resolved(&mut self, path: PathBuf) {
        self.nav_path = Some(path);
        self.nav_resolving = false;
        if self.nav_queued != 0 {
            let forward = self.nav_queued > 0;
            let queued = self.nav_queued - self.nav_queued.signum();
            self.go(forward);
            self.nav_queued = queued;
        }
    }

    /// Puts the app icon on the title bar and the taskbar button.
    fn apply_icons(&mut self) {
        let (Some(gfx), Some(icons)) = (&self.gfx, &self.icons) else { return };
        gfx.window.set_window_icon(icons.small.to_icon());
        #[cfg(windows)]
        {
            use winit::platform::windows::WindowExtWindows;
            gfx.window.set_taskbar_icon(icons.large.to_icon());
        }
        self.icons = None;
    }

    fn toggle_fullscreen(&mut self) {
        if let Some(gfx) = &self.gfx {
            let fullscreen = gfx.window.fullscreen().is_none().then_some(Fullscreen::Borderless(None));
            gfx.window.set_fullscreen(fullscreen);
        }
    }

    fn leave_fullscreen(&mut self) {
        if let Some(gfx) = &self.gfx
            && gfx.window.fullscreen().is_some()
        {
            gfx.window.set_fullscreen(None);
        }
    }

    fn on_loaded(&mut self, path: Option<PathBuf>, result: Result<Document, loader::LoadError>) {
        let after_delete = std::mem::take(&mut self.load_after_delete);
        self.loading = false;
        self.nav_resolving = false;
        self.nav_queued = 0;
        if path.is_some() {
            self.nav_path = path;
        }
        let doc = match result {
            Ok(doc) => doc,
            // The previous picture stays on screen, unless it was deleted.
            Err(e) => {
                if after_delete {
                    self.close_doc();
                }
                // The folder being empty now is no surprise after a delete.
                if !(after_delete && matches!(e, loader::LoadError::NoOtherFiles)) {
                    self.show_error(e.to_string());
                }
                return;
            }
        };
        if let Some(gfx) = &self.gfx {
            gfx.window.set_title(&format!("{} – {APP_NAME}", doc.name));
        }
        // A good file clears any error about the one before.
        self.banner = None;

        let old_id = self.doc_id;
        self.doc = Some(doc);
        self.doc_id += 1;
        self.watch_doc();
        if self.editable_path().is_none() {
            self.close_menu();
        } else if self.current_editors().is_none() {
            // List the apps now, so the menu opens with them.
            self.list_editors();
        }
        if self.load_is_reload {
            // Keep the zoom and position, and show the old picture until the new one is drawn.
            self.reload_of = Some(old_id);
            if self.fit_mode {
                self.refit();
            } else {
                self.settle_at = Some(Instant::now());
                self.request_redraw();
            }
        } else {
            self.reload_of = None;
            self.frame = None;
            self.refit();
        }
    }

    /// Watches the open file for changes, unless it is watched already.
    fn watch_doc(&mut self) {
        let path = self.doc.as_ref().and_then(|doc| doc.path.clone());
        if path != self.watched {
            self.watched = path;
            self.watcher = None;
            self.watcher = self.watched.as_ref().and_then(|path| watch::watch(path, self.proxy.clone()));
        }
    }

    /// Takes the picture off the screen and shows the empty screen.
    fn close_doc(&mut self) {
        self.doc = None;
        // Renders of the closed picture still on the way are dropped.
        self.doc_id += 1;
        self.frame = None;
        self.reload_of = None;
        self.reload_at = None;
        self.watch_doc();
        self.close_menu();
        if let Some(gfx) = &self.gfx {
            gfx.window.set_title(APP_NAME);
        }
        self.request_redraw();
    }

    fn show_error(&mut self, message: String) {
        eprintln!("error: {message}");
        self.banner = Some(Banner::new(message));
        self.request_redraw();
    }

    fn backdrop(&self) -> Backdrop {
        // The empty screen keeps the theme's color, so its text stays readable.
        let canvas = if self.doc.is_some() { self.canvas } else { Canvas::Theme };
        ui::backdrop(self.theme, self.background, canvas, self.scale_factor as f32)
    }

    /// The colors changed: redraw the picture right away.
    fn style_changed(&mut self) {
        self.settle_at = Some(Instant::now());
        self.request_redraw();
    }

    fn cycle_background(&mut self) {
        self.background = self.background.next();
        self.save_settings();
        self.style_changed();
    }

    fn cycle_canvas(&mut self) {
        self.canvas = self.canvas.next();
        self.save_settings();
        self.style_changed();
    }

    fn settings(&self) -> Settings {
        Settings { background: self.background, canvas: self.canvas, editor: self.editor.clone(), window: self.place }
    }

    fn save_settings(&self) {
        settings::save(self.settings());
    }

    /// Where to open the window: where it was last time, if that is still on a screen.
    fn start_place(&self, event_loop: &ActiveEventLoop) -> Option<WindowPlace> {
        let mut place = self.place?;
        let monitors: Vec<Monitor> = event_loop
            .available_monitors()
            .map(|m| Monitor {
                position: (m.position().x, m.position().y),
                size: (m.size().width, m.size().height),
                scale: m.scale_factor(),
            })
            .collect();
        if !place.is_on_screen(&monitors) {
            eprintln!("the window's last place is off screen, so the system places it");
            place.position = None;
            if let Some(primary) = event_loop.primary_monitor() {
                let screen = primary.size().to_logical::<u32>(primary.scale_factor());
                place.size = (place.size.0.min(screen.width), place.size.1.min(screen.height));
            }
        }
        Some(place)
    }

    /// Notes where the window is, to open there next time. A maximized
    /// window keeps the place it goes back to; a minimized or full screen
    /// one changes nothing.
    fn remember_place(&mut self) {
        let Some(gfx) = &self.gfx else { return };
        let window = &gfx.window;
        if window.fullscreen().is_some() || window.is_minimized() == Some(true) {
            return;
        }
        if window.is_maximized() {
            let normal = WindowPlace { position: None, size: DEFAULT_SIZE, maximized: false };
            self.place = Some(WindowPlace { maximized: true, ..self.place.unwrap_or(normal) });
            return;
        }
        let size = window.inner_size().to_logical::<u32>(window.scale_factor());
        if size.width == 0 || size.height == 0 {
            return;
        }
        let position = window.outer_position().ok().map(|p| (p.x, p.y));
        self.place = Some(WindowPlace { position, size: (size.width, size.height), maximized: false });
    }

    fn set_theme(&mut self, theme: winit::window::Theme) {
        let theme = match theme {
            winit::window::Theme::Light => Theme::Light,
            winit::window::Theme::Dark => Theme::Dark,
        };
        if theme != self.theme {
            self.theme = theme;
            self.style_changed();
        }
    }

    /// What the UI shows right now.
    fn scene(&self) -> Scene {
        Scene {
            window: (self.viewport.width, self.viewport.height),
            scale: self.scale_factor as f32,
            theme: self.theme,
            info: self.doc.as_ref().map(|doc| Info {
                name: doc.name.clone(),
                size: doc.size,
                file_size: doc.file_size,
            }),
            show_empty: !self.loading,
            zoom_percent: self.view.scale / self.actual_size_scale() * 100.0,
            background: self.background,
            canvas: self.canvas,
            nav_enabled: self.nav_path.is_some(),
            delete_enabled: self.deletable_path().is_some(),
            edit: self.edit_scene(),
            menu: self.menu_scene(),
            hovered: self.hovered,
            pressed: self.pressed,
            file_hovering: self.file_hovering,
        }
    }

    fn activate(&mut self, button: Button) {
        let center = self.center();
        match button {
            Button::Open => self.open_dialog(),
            Button::Edit => self.edit(),
            Button::EditMenu => self.toggle_menu(),
            Button::Previous | Button::SidePrevious => self.go(false),
            Button::Next | Button::SideNext => self.go(true),
            Button::ZoomOut => self.zoom_at(1.0 / KEY_ZOOM, center),
            Button::Zoom => self.toggle_actual_size(),
            Button::ZoomIn => self.zoom_at(KEY_ZOOM, center),
            Button::Fit => self.refit(),
            Button::Background => self.cycle_background(),
            Button::Canvas => self.cycle_canvas(),
            Button::Delete => self.ask_delete(),
        }
    }

    fn set_cursor_icon(&mut self, icon: CursorIcon) {
        if icon != self.cursor_icon {
            self.cursor_icon = icon;
            if let Some(gfx) = &self.gfx {
                gfx.window.set_cursor(icon);
            }
        }
    }

    /// Updates the hovered button and the cursor shape for the cursor position.
    fn update_hover(&mut self) {
        let hit = if self.drag_from.is_some() { None } else { ui::hit(&self.scene(), self.cursor) };
        let hovered = match hit {
            Some(Hit::Button(button)) => Some(button),
            _ => None,
        };
        if hovered != self.hovered {
            self.hovered = hovered;
            self.request_redraw();
        }
        if let Some(Hit::MenuItem(row)) = hit
            && self.menu_highlight != Some(row)
        {
            self.menu_highlight = Some(row);
            self.request_redraw();
        }
        let icon = if self.drag_from.is_some() {
            CursorIcon::Grabbing
        } else if hovered.is_some() || matches!(hit, Some(Hit::MenuItem(_))) {
            CursorIcon::Pointer
        } else {
            CursorIcon::Default
        };
        self.set_cursor_icon(icon);
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

    /// The open file, when the "Open in" button can open it in another app.
    /// Pasted SVG code has no file, and so far only Windows lists the apps.
    fn editable_path(&self) -> Option<&PathBuf> {
        if !cfg!(windows) {
            return None;
        }
        self.doc.as_ref()?.path.as_ref()
    }

    /// The extension the app list is for, like ".svg".
    fn edit_ext(&self) -> Option<String> {
        let ext = self.editable_path()?.extension();
        Some(ext.map_or(".svg".into(), |ext| format!(".{}", ext.to_string_lossy().to_lowercase())))
    }

    /// The apps that can open the open file, once they are listed.
    fn current_editors(&self) -> Option<&[Editor]> {
        let ext = self.edit_ext()?;
        self.editors.as_ref().filter(|(listed, _)| *listed == ext).map(|(_, list)| list.as_slice())
    }

    /// The app menu's rows, and what each one does.
    fn menu_rows(&self) -> Vec<(MenuRow, MenuAction)> {
        let row = |label: &str, enabled| MenuRow { label: label.to_owned(), enabled };
        let mut rows: Vec<(MenuRow, MenuAction)> = match self.current_editors() {
            Some(list) => list.iter().enumerate().map(|(i, e)| (row(&e.name, true), MenuAction::Editor(i))).collect(),
            None => vec![(row("Looking for apps…", false), MenuAction::None)],
        };
        rows.push((row("Other app…", true), MenuAction::OtherApp));
        rows
    }

    fn edit_scene(&self) -> Option<EditScene> {
        let text = self.text.as_ref()?;
        self.editable_path()?;
        let label = match &self.editor {
            Some(editor) => format!("Open in {}", editor.name),
            None => "Open with…".to_owned(),
        };
        Some(EditScene { label_size: ui::measure(text, &label), label })
    }

    fn menu_scene(&self) -> Option<MenuScene> {
        if !self.menu_open {
            return None;
        }
        let text = self.text.as_ref()?;
        self.editable_path()?;
        let rows = self.menu_rows();
        let editors = self.current_editors().unwrap_or_default();
        let checked = rows.iter().position(|(_, action)| match action {
            MenuAction::Editor(i) => self.editor.as_ref().is_some_and(|e| e.id == editors[*i].id),
            _ => false,
        });
        let text_width = rows.iter().map(|(row, _)| ui::measure(text, &row.label).0).fold(0.0, f32::max);
        let rows = rows.into_iter().map(|(row, _)| row).collect();
        Some(MenuScene { rows, checked, highlighted: self.menu_highlight, text_width })
    }

    /// The menu row to start on: the default app, or else the first one.
    fn first_highlight(&self) -> Option<usize> {
        let menu = self.menu_scene()?;
        menu.checked.or_else(|| menu.rows.iter().position(|row| row.enabled))
    }

    #[cfg(windows)]
    fn shell(&mut self) -> Option<&crate::platform::windows::Shell> {
        if self.shell.is_none() {
            match crate::platform::windows::Shell::spawn(self.proxy.clone()) {
                Ok(shell) => self.shell = Some(shell),
                Err(e) => eprintln!("could not start shell thread: {e}"),
            }
        }
        self.shell.as_ref()
    }

    /// Asks for the apps that can open the open file.
    fn list_editors(&mut self) {
        #[cfg(windows)]
        if let Some(ext) = self.edit_ext()
            && let Some(shell) = self.shell()
        {
            shell.list(ext);
        }
    }

    fn open_in(&mut self, editor: Editor) {
        #[cfg(windows)]
        if let Some(path) = self.editable_path().cloned()
            && let Some(shell) = self.shell()
        {
            shell.open(editor, path);
        }
        #[cfg(not(windows))]
        let _ = editor;
    }

    /// The main part of the "Open in" button: opens the file in the default
    /// app, or shows the menu to pick one.
    fn edit(&mut self) {
        if self.editable_path().is_none() {
            return;
        }
        match self.editor.clone() {
            Some(editor) => self.open_in(editor),
            None => self.open_menu(),
        }
    }

    fn open_menu(&mut self) {
        if self.menu_open || self.editable_path().is_none() {
            return;
        }
        self.menu_open = true;
        self.menu_highlight = self.first_highlight();
        // List again each time, so newly installed apps show up.
        self.list_editors();
        self.update_hover();
        self.request_redraw();
    }

    fn close_menu(&mut self) {
        if self.menu_open {
            self.menu_open = false;
            self.menu_highlight = None;
            self.update_hover();
            self.request_redraw();
        }
    }

    fn toggle_menu(&mut self) {
        if self.menu_open { self.close_menu() } else { self.open_menu() }
    }

    /// Does what app menu row `index` says. A picked app becomes the default.
    fn pick(&mut self, index: usize) {
        let action = self.menu_rows().get(index).map(|&(_, action)| action);
        self.close_menu();
        match action {
            Some(MenuAction::Editor(i)) => {
                if let Some(editor) = self.current_editors().and_then(|list| list.get(i)).cloned() {
                    self.editor = Some(editor.clone());
                    self.save_settings();
                    self.open_in(editor);
                }
            }
            Some(MenuAction::OtherApp) => self.open_with(),
            _ => {}
        }
    }

    /// Moves the menu highlight to the next row down or up that can be picked.
    fn move_highlight(&mut self, down: bool) {
        let rows = self.menu_rows();
        let enabled: Vec<usize> = (0..rows.len()).filter(|&i| rows[i].0.enabled).collect();
        let count = enabled.len();
        if count == 0 {
            return;
        }
        let at = self.menu_highlight.and_then(|h| enabled.iter().position(|&i| i == h));
        let next = match (at, down) {
            (Some(p), true) => enabled[(p + 1) % count],
            (Some(p), false) => enabled[(p + count - 1) % count],
            (None, true) => enabled[0],
            (None, false) => enabled[count - 1],
        };
        self.menu_highlight = Some(next);
        self.request_redraw();
    }

    /// Shows Windows' own chooser, to open the file once in any other app.
    fn open_with(&mut self) {
        #[cfg(windows)]
        {
            let Some(path) = self.editable_path().cloned() else { return };
            if self.gfx.is_none() || self.dialog_open {
                return;
            }
            match crate::platform::windows::spawn_open_with(self.hwnd(), path, self.proxy.clone()) {
                Ok(()) => self.dialog_open = true,
                Err(e) => eprintln!("could not start open-with thread: {e}"),
            }
        }
    }

    /// The window's handle on Windows, for system dialogs to show over. 0 elsewhere.
    fn hwnd(&self) -> isize {
        #[cfg(windows)]
        {
            use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

            if let Some(gfx) = &self.gfx
                && let Ok(RawWindowHandle::Win32(handle)) = gfx.window.window_handle().map(|h| h.as_raw())
            {
                return handle.hwnd.get();
            }
        }
        0
    }

    /// The open file, when Delete can move it to the trash. Pasted SVG code
    /// has no file, and a file just deleted stays on screen until the next one loads.
    fn deletable_path(&self) -> Option<&PathBuf> {
        if self.load_after_delete {
            return None;
        }
        self.doc.as_ref()?.path.as_ref()
    }

    /// Asks whether to move the open file to the trash. The answer comes
    /// back as `AppEvent::DeleteAnswered`.
    fn ask_delete(&mut self) {
        let Some(gfx) = &self.gfx else { return };
        let Some(path) = self.deletable_path().cloned() else { return };
        if self.dialog_open {
            return;
        }
        let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        let dialog = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Warning)
            .set_title("Delete file")
            .set_description(format!("Move “{name}” to the {}?", trash::TRASH_NAME))
            // Windows shows OK and Cancel instead.
            .set_buttons(rfd::MessageButtons::OkCancelCustom(DELETE_LABEL.into(), "Cancel".into()))
            .set_parent(&*gfx.window);
        // The dialog blocks its thread until it closes, so it gets its own.
        let proxy = self.proxy.clone();
        let spawned = std::thread::Builder::new().name("delete-dialog".into()).spawn(move || {
            let confirmed = match dialog.show() {
                rfd::MessageDialogResult::Ok => true,
                rfd::MessageDialogResult::Custom(label) => label == DELETE_LABEL,
                _ => false,
            };
            let _ = proxy.send_event(AppEvent::DeleteAnswered { path, confirmed });
        });
        match spawned {
            Ok(_) => self.dialog_open = true,
            Err(e) => eprintln!("could not start delete dialog thread: {e}"),
        }
    }

    /// Moves `path` to the trash, after the user said yes.
    fn trash(&mut self, path: PathBuf) {
        // Stop watching first, or the deletion would look like a change to reload.
        if self.watched.as_ref() == Some(&path) {
            self.watcher = None;
            self.watched = None;
            self.reload_at = None;
        }
        // The dialog stays marked open until it is done, so Delete can't start twice.
        match trash::spawn(path, self.hwnd(), self.proxy.clone()) {
            Ok(()) => self.dialog_open = true,
            Err(e) => {
                eprintln!("could not start trash thread: {e}");
                self.dialog_open = false;
                self.watch_doc();
                self.show_error(format!("This file can't be moved to the {}.", trash::TRASH_NAME));
            }
        }
    }

    fn on_trashed(&mut self, path: PathBuf, result: Result<(), TrashError>) {
        self.dialog_open = false;
        match result {
            Ok(()) => {
                eprintln!("moved {} to the {}", path.display(), trash::TRASH_NAME);
                let is_open = self.doc.as_ref().is_some_and(|doc| doc.path.as_ref() == Some(&path));
                if is_open {
                    // Show the next SVG in the folder, as Next would.
                    self.nav_path = Some(path);
                    self.nav_resolving = false;
                    self.go(true);
                    self.load_after_delete = true;
                    self.request_redraw();
                }
            }
            Err(e) => {
                self.watch_doc();
                if e == TrashError::Failed {
                    self.show_error(format!("This file can't be moved to the {}.", trash::TRASH_NAME));
                }
            }
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
        // Presses on the UI never reach the picture.
        let pressed = state == ElementState::Pressed;
        if pressed && matches!(button, MouseButton::Left | MouseButton::Middle) {
            match ui::hit(&self.scene(), self.cursor) {
                Some(Hit::Button(hit)) if button == MouseButton::Left => {
                    self.pressed = Some(hit);
                    self.request_redraw();
                    return;
                }
                Some(Hit::MenuItem(row)) if button == MouseButton::Left => {
                    self.pick(row);
                    return;
                }
                // A click outside the open menu only closes it.
                Some(Hit::Dismiss) => {
                    self.close_menu();
                    return;
                }
                Some(_) => return,
                None => {}
            }
        }
        if !pressed && button == MouseButton::Left
            && let Some(button) = self.pressed.take()
        {
            self.request_redraw();
            // A click counts when the button is released over the same button.
            if ui::hit(&self.scene(), self.cursor) == Some(Hit::Button(button)) {
                self.activate(button);
            }
            return;
        }
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
        self.update_hover();
    }

    fn on_cursor_moved(&mut self, position: PhysicalPosition<f64>) {
        self.cursor = (position.x as f32, position.y as f32);
        if let Some(from) = self.drag_from {
            self.drag_from = Some(self.cursor);
            self.pan(self.cursor.0 - from.0, self.cursor.1 - from.1);
        }
        self.update_hover();
    }

    fn on_key(&mut self, event: &KeyEvent) {
        if event.state != ElementState::Pressed {
            return;
        }
        // The open menu takes the keys it uses first.
        if self.menu_open {
            match &event.logical_key {
                Key::Named(NamedKey::ArrowDown) => {
                    self.move_highlight(true);
                    return;
                }
                Key::Named(NamedKey::ArrowUp) => {
                    self.move_highlight(false);
                    return;
                }
                Key::Named(NamedKey::Enter) => {
                    if let Some(row) = self.menu_highlight {
                        self.pick(row);
                    }
                    return;
                }
                Key::Named(NamedKey::Escape) => {
                    self.close_menu();
                    return;
                }
                _ => {}
            }
        }
        let m = self.modifiers;
        // Cmd on macOS, Ctrl elsewhere.
        let primary = if cfg!(target_os = "macos") { m.super_key() } else { m.control_key() };
        let center = self.center();
        if primary {
            // Cmd+Delete moves files to the Trash in Finder too.
            if cfg!(target_os = "macos") && event.logical_key == Key::Named(NamedKey::Backspace) {
                if !event.repeat {
                    self.ask_delete();
                }
                return;
            }
            if let Key::Character(c) = event.key_without_modifiers() {
                let once = !event.repeat;
                match c.to_lowercase().as_str() {
                    "o" if once => self.open_dialog(),
                    "v" if once => self.paste(),
                    "w" | "q" if once => self.exit_requested = true,
                    // Browsers and most viewers zoom with Ctrl/Cmd and these keys.
                    "+" | "=" => self.zoom_at(KEY_ZOOM, center),
                    "-" => self.zoom_at(1.0 / KEY_ZOOM, center),
                    "0" if once => self.refit(),
                    // Ctrl+Cmd+F is the usual full-screen shortcut on macOS.
                    "f" if once && cfg!(target_os = "macos") && m.control_key() => {
                        self.toggle_fullscreen();
                    }
                    _ => {}
                }
            }
            return;
        }
        if m.control_key() || m.alt_key() || m.super_key() {
            return;
        }
        let step = (ARROW_STEP * self.scale_factor) as f32;
        match &event.logical_key {
            Key::Character(c) => match c.as_str() {
                "+" | "=" => self.zoom_at(KEY_ZOOM, center),
                "-" => self.zoom_at(1.0 / KEY_ZOOM, center),
                "0" | "f" | "F" => self.refit(),
                "1" => self.toggle_actual_size(),
                "b" | "B" if m.shift_key() => self.cycle_canvas(),
                "b" | "B" => self.cycle_background(),
                "r" | "R" => self.reload(),
                "e" | "E" if !event.repeat && m.shift_key() => self.toggle_menu(),
                "e" | "E" if !event.repeat => self.edit(),
                _ => {}
            },
            // The arrows move the view, so the picture moves the other way.
            Key::Named(NamedKey::ArrowLeft) => self.pan(step, 0.0),
            Key::Named(NamedKey::ArrowRight) => self.pan(-step, 0.0),
            Key::Named(NamedKey::ArrowUp) => self.pan(0.0, step),
            Key::Named(NamedKey::ArrowDown) => self.pan(0.0, -step),
            Key::Named(NamedKey::PageDown | NamedKey::Space) => self.go(true),
            Key::Named(NamedKey::PageUp | NamedKey::Backspace) => self.go(false),
            Key::Named(NamedKey::F11) => self.toggle_fullscreen(),
            Key::Named(NamedKey::Escape) => self.leave_fullscreen(),
            Key::Named(NamedKey::Delete) if !event.repeat => self.ask_delete(),
            _ => {}
        }
    }

    /// What a sharp render of the current state would be drawn for.
    fn target(&self) -> Target {
        Target {
            doc: self.doc_id,
            view: self.view,
            size: (self.viewport.width, self.viewport.height),
            backdrop: self.backdrop(),
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
            picture_size: doc.size,
            view: self.view,
            width: target.size.0,
            height: target.size.1,
            backdrop: target.backdrop,
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
        let target = self.target();
        let scene = self.scene();
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
        let canvas = target.backdrop.canvas;
        let sharp = match &self.frame {
            Some(f) if Target::of(f) == target && f.pixels.len() == buffer.len() => {
                buffer.copy_from_slice(&f.pixels);
                true
            }
            Some(f) if f.doc == self.doc_id || Some(f.doc) == self.reload_of => {
                let from = (f.width, f.height);
                render::blit_preview(&f.pixels, from, f.view, &mut buffer, size, self.view, canvas);
                false
            }
            _ => {
                buffer.fill(canvas);
                false
            }
        };
        if let Some(text) = &self.text {
            self.ui.draw(text, &scene, &mut buffer);
            if let Some(banner) = &mut self.banner {
                banner.draw(text, &mut buffer, size, scene.scale);
            }
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

/// What a row of the app menu does.
#[derive(Debug, Clone, Copy, PartialEq)]
enum MenuAction {
    /// Open the file in the listed app with this index.
    Editor(usize),
    /// Show Windows' own chooser.
    OtherApp,
    /// Nothing: a placeholder row.
    None,
}

/// The document, view, window size and colors a frame is drawn for.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Target {
    doc: u64,
    view: View,
    size: (u32, u32),
    backdrop: Backdrop,
}

impl Target {
    fn of(frame: &Frame) -> Target {
        Target {
            doc: frame.doc,
            view: frame.view,
            size: (frame.width, frame.height),
            backdrop: frame.backdrop,
        }
    }
}

impl ApplicationHandler<AppEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        let mut attributes = Window::default_attributes()
            .with_title(APP_NAME)
            .with_inner_size(LogicalSize::new(DEFAULT_SIZE.0, DEFAULT_SIZE.1));
        // Open where the window was when the app last closed.
        if let Some(place) = self.start_place(event_loop) {
            attributes = attributes
                .with_inner_size(LogicalSize::new(place.size.0, place.size.1))
                .with_maximized(place.maximized);
            if let Some((x, y)) = place.position {
                attributes = attributes.with_position(PhysicalPosition::new(x, y));
            }
        }
        // The Wayland app ID and X11 WM_CLASS. Desktops match it to
        // peregrineview.desktop for the taskbar icon and window grouping.
        #[cfg(target_os = "linux")]
        let attributes = {
            use winit::platform::wayland::WindowAttributesExtWayland;
            attributes.with_name("peregrineview", "peregrineview")
        };
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
        if let Some(theme) = window.theme() {
            self.set_theme(theme);
        }
        self.viewport = window.inner_size();
        self.scale_factor = window.scale_factor();
        self.gfx = Some(Gfx { window, surface });
        self.place_changed = true;
        self.apply_icons();
        eprintln!("window created {:?} after start", self.started.elapsed());
        self.refit();
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::Loaded { id, .. } if id != self.load_id => {
                eprintln!("load dropped: a newer one was started");
            }
            AppEvent::Loaded { path, result, .. } => self.on_loaded(path, result),
            AppEvent::Resolved { id, path } if id == self.load_id => self.on_resolved(path),
            AppEvent::Resolved { .. } => {}
            AppEvent::FileChanged(path) => {
                if self.doc.as_ref().is_some_and(|doc| doc.path.as_ref() == Some(&path)) {
                    self.reload_at = Some(Instant::now() + RELOAD_DELAY);
                }
            }
            AppEvent::DialogClosed(path) => {
                self.dialog_open = false;
                if let Some(path) = path {
                    self.open(Source::File(path));
                }
            }
            AppEvent::DeleteAnswered { path, confirmed } => {
                self.dialog_open = false;
                if confirmed {
                    self.trash(path);
                }
            }
            AppEvent::Trashed { path, result } => self.on_trashed(path, result),
            AppEvent::Rendered(frame) => self.on_rendered(frame),
            AppEvent::Icons(icons) => {
                self.icons = Some(icons);
                self.apply_icons();
            }
            #[cfg(target_os = "macos")]
            AppEvent::OpenFile(path) => self.open(Source::File(path)),
            #[cfg(target_os = "linux")]
            AppEvent::SystemTheme(theme) => self.set_theme(theme),
            #[cfg(windows)]
            AppEvent::OpenWithClosed(ok) => {
                self.dialog_open = false;
                if !ok {
                    self.show_error("This file can't be opened in another app.".into());
                }
            }
            #[cfg(windows)]
            AppEvent::Editors { ext, list } => {
                self.editors = Some((ext, list));
                if self.menu_open {
                    // The rows changed, so start again on the default app.
                    self.menu_highlight = self.first_highlight();
                    self.update_hover();
                    self.request_redraw();
                }
            }
            #[cfg(windows)]
            AppEvent::EditorOpened { editor, result } => {
                use crate::platform::windows::OpenError;
                match result {
                    Ok(()) => {}
                    Err(OpenError::NotInstalled) => {
                        self.show_error(format!("{} isn't installed any more. Pick another app.", editor.name));
                        self.open_menu();
                    }
                    Err(OpenError::Failed(_)) => {
                        self.show_error(format!("This file can't be opened in {}.", editor.name));
                    }
                }
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Noted only now: while a window maximizes, Windows reports the move
        // before winit knows it is maximized.
        if std::mem::take(&mut self.place_changed) {
            self.remember_place();
        }
        let now = Instant::now();
        if self.settle_at.is_some_and(|at| now >= at) {
            self.settle_at = None;
            self.request_sharp();
        }
        if self.banner.as_ref().is_some_and(|b| now >= b.until) {
            self.banner = None;
            self.request_redraw();
        }
        if self.reload_at.is_some_and(|at| now >= at) {
            eprintln!("file changed on disk, reloading");
            self.reload();
        }
        let next = [self.settle_at, self.banner.as_ref().map(|b| b.until), self.reload_at]
            .into_iter()
            .flatten()
            .min();
        event_loop.set_control_flow(match next {
            Some(at) => ControlFlow::WaitUntil(at),
            None => ControlFlow::Wait,
        });
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::DroppedFile(path) => {
                self.file_hovering = false;
                self.open(Source::File(path));
            }
            WindowEvent::HoveredFile(_) => {
                self.file_hovering = true;
                self.request_redraw();
            }
            WindowEvent::HoveredFileCancelled => {
                self.file_hovering = false;
                self.request_redraw();
            }
            WindowEvent::ThemeChanged(theme) => self.set_theme(theme),
            WindowEvent::CursorLeft { .. } => {
                self.hovered = None;
                self.request_redraw();
            }
            WindowEvent::Resized(size) => {
                self.on_resized(size);
                self.place_changed = true;
            }
            WindowEvent::Moved(_) => self.place_changed = true,
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.on_scale_factor_changed(scale_factor);
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::CursorMoved { position, .. } => self.on_cursor_moved(position),
            // A release can be missed while another window has focus.
            WindowEvent::Focused(false) => {
                self.drag_from = None;
                self.close_menu();
            }
            // Linux sends no ThemeChanged, so check again when the user comes back.
            #[cfg(target_os = "linux")]
            WindowEvent::Focused(true) => {
                crate::platform::linux::spawn_theme_query(self.proxy.clone());
            }
            WindowEvent::MouseInput { state, button, .. } => self.on_mouse_button(state, button),
            WindowEvent::MouseWheel { delta, .. } => self.on_wheel(delta),
            WindowEvent::PinchGesture { delta, .. } if delta.is_finite() => {
                self.zoom_at((1.0 + delta as f32).max(0.01), self.cursor);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                self.on_key(&event);
                if self.exit_requested {
                    event_loop.exit();
                }
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        // Saved now and not on a thread, which would stop when the app does.
        settings::save_now(&self.settings());
    }
}
