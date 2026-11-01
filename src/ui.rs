//! UI drawn on top of the picture: toolbar, info bar, empty screen and the
//! error banner. Icons are vector shapes drawn in code.

use std::time::{Duration, Instant};

use resvg::tiny_skia::{
    FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Stroke, StrokeDash, Transform,
};

use crate::render::{Backdrop, Fill};
use crate::text::TextRenderer;

/// How long an error message stays on screen.
pub const BANNER_TIME: Duration = Duration::from_secs(6);

// Sizes below are in logical pixels.
const EDGE: f32 = 12.0;
const TOOLBAR_BOTTOM: f32 = 16.0;
const TOOLBAR_PAD: f32 = 6.0;
const BUTTON: f32 = 32.0;
const ZOOM_BUTTON: f32 = 60.0;
const BUTTON_GAP: f32 = 2.0;
const SEPARATOR: f32 = 9.0;
const PANEL_RADIUS: f32 = 10.0;
const BUTTON_RADIUS: f32 = 7.0;
const ICON: f32 = 20.0;
const UI_FONT_SIZE: f32 = 13.0;
const INFO_PAD_X: f32 = 12.0;
const INFO_PAD_Y: f32 = 7.0;
/// The ▾ part of the "Open in" button.
const EDIT_ARROW: f32 = 28.0;
/// The longest "Open in" label; longer ones are shortened with "…".
const EDIT_MAX_LABEL: f32 = 240.0;
/// Space kept between the info bar and the "Open in" button.
const EDIT_GAP: f32 = 8.0;
const MENU_GAP: f32 = 4.0;
const MENU_PAD: f32 = 5.0;
const MENU_ROW: f32 = 30.0;
const MENU_SEPARATOR: f32 = 9.0;
/// Where menu labels start, leaving room for the check mark.
const MENU_TEXT_X: f32 = 34.0;
const MENU_MIN_WIDTH: f32 = 180.0;
const CHECKER_CELL: f32 = 8.0;
const BANNER_FONT_SIZE: f32 = 14.0;
const BANNER_PAD_X: f32 = 16.0;
const BANNER_PAD_Y: f32 = 9.0;
const BANNER_RADIUS: f32 = 8.0;
const BANNER_FILL: [u8; 4] = [0xB3, 0x26, 0x1E, 0xF2];
const WHITE: [u8; 3] = [0xFF, 0xFF, 0xFF];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
}

/// What is drawn behind the picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Background {
    Checkerboard,
    White,
    Dark,
}

impl Background {
    pub fn next(self) -> Background {
        match self {
            Background::Checkerboard => Background::White,
            Background::White => Background::Dark,
            Background::Dark => Background::Checkerboard,
        }
    }
}

struct Palette {
    canvas: u32,
    checker: (u32, u32),
    panel: [u8; 4],
    border: [u8; 4],
    separator: [u8; 4],
    hover: [u8; 4],
    pressed: [u8; 4],
    icon: [u8; 3],
    text: [u8; 3],
    muted: [u8; 3],
    accent: [u8; 4],
    accent_hover: [u8; 4],
    drop_border: [u8; 4],
    drop_active: [u8; 4],
}

const LIGHT: Palette = Palette {
    canvas: 0xF3F3F3,
    checker: (0xFFFFFF, 0xE4E4E4),
    panel: [0xFF, 0xFF, 0xFF, 0xF0],
    border: [0x00, 0x00, 0x00, 0x1F],
    separator: [0x00, 0x00, 0x00, 0x1F],
    hover: [0x00, 0x00, 0x00, 0x14],
    pressed: [0x00, 0x00, 0x00, 0x26],
    icon: [0x2B, 0x2B, 0x2B],
    text: [0x1F, 0x1F, 0x1F],
    muted: [0x6B, 0x6B, 0x6B],
    accent: [0x25, 0x63, 0xEB, 0xFF],
    accent_hover: [0x1D, 0x4E, 0xD8, 0xFF],
    drop_border: [0x00, 0x00, 0x00, 0x40],
    drop_active: [0x25, 0x63, 0xEB, 0xFF],
};

const DARK: Palette = Palette {
    canvas: 0x1C1C1C,
    checker: (0x3A3A3A, 0x2C2C2C),
    panel: [0x2C, 0x2C, 0x2C, 0xF0],
    border: [0xFF, 0xFF, 0xFF, 0x1F],
    separator: [0xFF, 0xFF, 0xFF, 0x24],
    hover: [0xFF, 0xFF, 0xFF, 0x1A],
    pressed: [0xFF, 0xFF, 0xFF, 0x2E],
    icon: [0xE8, 0xE8, 0xE8],
    text: [0xF0, 0xF0, 0xF0],
    muted: [0xA8, 0xA8, 0xA8],
    accent: [0x25, 0x63, 0xEB, 0xFF],
    accent_hover: [0x3B, 0x76, 0xF0, 0xFF],
    drop_border: [0xFF, 0xFF, 0xFF, 0x47],
    drop_active: [0x60, 0x9B, 0xF5, 0xFF],
};

impl Theme {
    fn palette(self) -> &'static Palette {
        match self {
            Theme::Light => &LIGHT,
            Theme::Dark => &DARK,
        }
    }
}

/// The colors the render thread paints behind and around the picture.
pub fn backdrop(theme: Theme, background: Background, scale: f32) -> Backdrop {
    let palette = theme.palette();
    let picture = match background {
        Background::Checkerboard => Fill::Checker {
            light: palette.checker.0,
            dark: palette.checker.1,
            cell: (CHECKER_CELL * scale).round().max(1.0) as u32,
        },
        Background::White => Fill::Solid(0xFFFFFF),
        Background::Dark => Fill::Solid(0x2A2A2A),
    };
    Backdrop { canvas: palette.canvas, picture }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Open,
    /// The main part of the top right button: opens the file in the default app.
    Edit,
    /// The ▾ part of the top right button: shows the app menu.
    EditMenu,
    Previous,
    Next,
    ZoomOut,
    Zoom,
    ZoomIn,
    Fit,
    Background,
}

/// A rectangle in window pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    /// Scales a logical rectangle to window pixels, rounding each edge.
    fn scaled(x: f32, y: f32, w: f32, h: f32, scale: f32) -> Rect {
        let (x0, y0) = ((x * scale).round() as i32, (y * scale).round() as i32);
        let (x1, y1) = (((x + w) * scale).round() as i32, ((y + h) * scale).round() as i32);
        Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
    }

    pub fn contains(&self, (px, py): (f32, f32)) -> bool {
        px >= self.x as f32
            && py >= self.y as f32
            && px < (self.x + self.w) as f32
            && py < (self.y + self.h) as f32
    }

    fn offset(&self, dx: i32, dy: i32) -> Rect {
        Rect { x: self.x + dx, y: self.y + dy, ..*self }
    }
}

enum Item {
    Button(Button),
    Separator,
}

const TOOLBAR_ITEMS: &[Item] = &[
    Item::Button(Button::Open),
    Item::Separator,
    Item::Button(Button::Previous),
    Item::Button(Button::Next),
    Item::Separator,
    Item::Button(Button::ZoomOut),
    Item::Button(Button::Zoom),
    Item::Button(Button::ZoomIn),
    Item::Button(Button::Fit),
    Item::Separator,
    Item::Button(Button::Background),
];

#[derive(Debug, PartialEq)]
pub struct ToolbarLayout {
    pub panel: Rect,
    pub buttons: Vec<(Button, Rect)>,
    pub separators: Vec<Rect>,
}

/// The toolbar, centered at the bottom of a `window`-sized area.
pub fn toolbar_layout(window: (u32, u32), scale: f32) -> ToolbarLayout {
    let width_of = |item: &Item| match item {
        Item::Button(Button::Zoom) => ZOOM_BUTTON,
        Item::Button(_) => BUTTON,
        Item::Separator => SEPARATOR,
    };
    let inner: f32 = TOOLBAR_ITEMS.iter().map(width_of).sum::<f32>()
        + BUTTON_GAP * (TOOLBAR_ITEMS.len() - 1) as f32;
    let (panel_w, panel_h) = (inner + 2.0 * TOOLBAR_PAD, BUTTON + 2.0 * TOOLBAR_PAD);
    let (window_w, window_h) = (window.0 as f32 / scale, window.1 as f32 / scale);
    let panel_x = ((window_w - panel_w) / 2.0).round();
    let panel_y = window_h - TOOLBAR_BOTTOM - panel_h;

    let mut layout = ToolbarLayout {
        panel: Rect::scaled(panel_x, panel_y, panel_w, panel_h, scale),
        buttons: Vec::new(),
        separators: Vec::new(),
    };
    let mut x = panel_x + TOOLBAR_PAD;
    let y = panel_y + TOOLBAR_PAD;
    for item in TOOLBAR_ITEMS {
        let w = width_of(item);
        match item {
            Item::Button(button) => layout.buttons.push((*button, Rect::scaled(x, y, w, BUTTON, scale))),
            Item::Separator => layout.separators.push(Rect::scaled(x + w / 2.0, y + 6.0, 0.0, BUTTON - 12.0, scale)),
        }
        x += w + BUTTON_GAP;
    }
    layout
}

#[derive(Debug, PartialEq)]
pub struct EmptyLayout {
    pub area: Rect,
    pub button: Rect,
}

/// The drop area and its Open button, centered in the window.
pub fn empty_layout(window: (u32, u32), scale: f32) -> EmptyLayout {
    let (window_w, window_h) = (window.0 as f32 / scale, window.1 as f32 / scale);
    let w = 420.0_f32.min(window_w - 48.0).max(0.0);
    let h = 230.0_f32.min(window_h - 48.0).max(0.0);
    let (x, y) = (((window_w - w) / 2.0).round(), ((window_h - h) / 2.0).round());
    let (bw, bh) = (132.0, 36.0);
    EmptyLayout {
        area: Rect::scaled(x, y, w, h, scale),
        button: Rect::scaled(x + ((w - bw) / 2.0).round(), y + h - 28.0 - bh, bw, bh, scale),
    }
}

/// What the UI needs to know about the open file.
pub struct Info {
    pub name: String,
    pub size: (f32, f32),
    pub file_size: Option<u64>,
}

/// Everything the UI draws depends on, for one frame.
pub struct Scene {
    pub window: (u32, u32),
    pub scale: f32,
    pub theme: Theme,
    /// The open file. Without one, the empty screen shows instead.
    pub info: Option<Info>,
    /// False while the first file is still loading, so the empty screen
    /// doesn't flash at startup.
    pub show_empty: bool,
    pub zoom_percent: f32,
    pub background: Background,
    pub nav_enabled: bool,
    /// The "Open in" button, when the open picture is a file another app can open.
    pub edit: Option<EditScene>,
    /// The app menu, while it is open. Needs `edit`.
    pub menu: Option<MenuScene>,
    pub hovered: Option<Button>,
    pub pressed: Option<Button>,
    pub file_hovering: bool,
}

/// The "Open in <app>" button in the top right corner.
pub struct EditScene {
    pub label: String,
    /// The label's width and line height in logical pixels, from `measure`.
    pub label_size: (f32, f32),
}

/// One line in the app menu.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuRow {
    pub label: String,
    pub enabled: bool,
}

/// The app menu under the "Open in" button.
pub struct MenuScene {
    /// The apps, then "Other app…", which comes after a separator.
    pub rows: Vec<MenuRow>,
    /// The row with a check mark: the default app.
    pub checked: Option<usize>,
    /// The row under the cursor, or picked with the arrow keys.
    pub highlighted: Option<usize>,
    /// The widest label in logical pixels, from `measure`.
    pub text_width: f32,
}

/// The width and line height of `s` in the UI font, in logical pixels.
pub fn measure(text: &TextRenderer, s: &str) -> (f32, f32) {
    (text.measure(s, UI_FONT_SIZE), text.line_metrics(UI_FONT_SIZE).1)
}

/// Where the "Open in" button goes, in logical pixels: (x, y, main part
/// width, height). It is as tall as the info bar.
fn edit_box(window_w: f32, edit: &EditScene) -> (f32, f32, f32, f32) {
    let (label_w, line_h) = edit.label_size;
    let main_w = (label_w.min(EDIT_MAX_LABEL) + 2.0 * INFO_PAD_X).ceil();
    let x = (window_w - EDGE - main_w - EDIT_ARROW).round();
    (x, EDGE, main_w, line_h + 2.0 * INFO_PAD_Y)
}

#[derive(Debug, PartialEq)]
pub struct EditLayout {
    pub panel: Rect,
    pub main: Rect,
    pub arrow: Rect,
}

/// The "Open in" button, in the top right corner of a `window`-sized area.
pub fn edit_layout(window: (u32, u32), scale: f32, edit: &EditScene) -> EditLayout {
    let (x, y, main_w, h) = edit_box(window.0 as f32 / scale, edit);
    EditLayout {
        panel: Rect::scaled(x, y, main_w + EDIT_ARROW, h, scale),
        main: Rect::scaled(x, y, main_w, h, scale),
        arrow: Rect::scaled(x + main_w, y, EDIT_ARROW, h, scale),
    }
}

#[derive(Debug, PartialEq)]
pub struct MenuLayout {
    pub panel: Rect,
    pub rows: Vec<Rect>,
    /// The line before the last row, when there is more than one row.
    pub separator: Option<Rect>,
}

/// The app menu, right-aligned under the "Open in" button.
pub fn menu_layout(window: (u32, u32), scale: f32, edit: &EditScene, menu: &MenuScene) -> MenuLayout {
    let window_w = window.0 as f32 / scale;
    let (edit_x, edit_y, main_w, edit_h) = edit_box(window_w, edit);
    let right = edit_x + main_w + EDIT_ARROW;
    // Room for the widest label: its start, the label, and the padding after
    // it, inside the row, inside the panel's padding.
    let width = (MENU_TEXT_X + menu.text_width + INFO_PAD_X + MENU_PAD + 1.0)
        .max(MENU_MIN_WIDTH)
        .max(main_w + EDIT_ARROW)
        .min(window_w - 2.0 * EDGE)
        .max(1.0)
        .ceil();
    let (x, top) = ((right - width).round(), edit_y + edit_h + MENU_GAP);

    let count = menu.rows.len();
    let has_separator = count > 1;
    let mut layout = MenuLayout { panel: Rect::default(), rows: Vec::with_capacity(count), separator: None };
    let mut y = top + MENU_PAD;
    for i in 0..count {
        if has_separator && i == count - 1 {
            layout.separator = Some(Rect::scaled(x + MENU_PAD, y + MENU_SEPARATOR / 2.0, width - 2.0 * MENU_PAD, 0.0, scale));
            y += MENU_SEPARATOR;
        }
        layout.rows.push(Rect::scaled(x + MENU_PAD, y, width - 2.0 * MENU_PAD, MENU_ROW, scale));
        y += MENU_ROW;
    }
    layout.panel = Rect::scaled(x, top, width, y + MENU_PAD - top, scale);
    layout
}

/// How wide the info bar may be, in window pixels, so it stays clear of the
/// "Open in" button.
fn info_max_width(scene: &Scene, scale: f32) -> i32 {
    let mut max_width = scene.window.0 as f32 - 2.0 * EDGE * scale;
    if let Some(edit) = &scene.edit {
        max_width -= edit_layout(scene.window, scale, edit).panel.w as f32 + EDIT_GAP * scale;
    }
    max_width.max(40.0 * scale) as i32
}

/// What is under the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Button(Button),
    /// A row of the app menu.
    MenuItem(usize),
    /// UI that takes the click but does nothing: a panel or a disabled button.
    Inert,
    /// Anywhere outside the open app menu. A click there only closes it.
    Dismiss,
}

pub fn hit(scene: &Scene, point: (f32, f32)) -> Option<Hit> {
    let scale = scale_or_one(scene.scale);
    if let Some(edit) = &scene.edit {
        let layout = edit_layout(scene.window, scale, edit);
        if let Some(menu) = &scene.menu {
            if layout.arrow.contains(point) {
                return Some(Hit::Button(Button::EditMenu));
            }
            let menu_layout = menu_layout(scene.window, scale, edit, menu);
            if let Some(i) = menu_layout.rows.iter().position(|row| row.contains(point)) {
                return Some(if menu.rows[i].enabled { Hit::MenuItem(i) } else { Hit::Inert });
            }
            return Some(if menu_layout.panel.contains(point) { Hit::Inert } else { Hit::Dismiss });
        }
        if layout.main.contains(point) {
            return Some(Hit::Button(Button::Edit));
        }
        if layout.arrow.contains(point) {
            return Some(Hit::Button(Button::EditMenu));
        }
        if layout.panel.contains(point) {
            return Some(Hit::Inert);
        }
    }
    if scene.info.is_some() {
        let layout = toolbar_layout(scene.window, scale);
        if !layout.panel.contains(point) {
            return None;
        }
        let button = layout.buttons.iter().find(|(_, rect)| rect.contains(point));
        Some(match button {
            Some(&(button, _)) if is_enabled(scene, button) => Hit::Button(button),
            _ => Hit::Inert,
        })
    } else if scene.show_empty {
        let layout = empty_layout(scene.window, scale);
        layout.button.contains(point).then_some(Hit::Button(Button::Open))
    } else {
        None
    }
}

fn is_enabled(scene: &Scene, button: Button) -> bool {
    match button {
        Button::Previous | Button::Next => scene.nav_enabled,
        _ => true,
    }
}

fn scale_or_one(scale: f32) -> f32 {
    if scale.is_finite() && scale > 0.0 { scale } else { 1.0 }
}

pub fn format_zoom(percent: f32) -> String {
    if percent >= 9.95 { format!("{percent:.0}%") } else { format!("{percent:.1}%") }
}

pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if b < KB {
        format!("{bytes} bytes")
    } else if b < KB * KB {
        format!("{:.1} KB", b / KB)
    } else if b < KB * KB * KB {
        format!("{:.1} MB", b / (KB * KB))
    } else {
        format!("{:.1} GB", b / (KB * KB * KB))
    }
}

fn info_label(info: &Info) -> (String, String) {
    let mut details = format!("{:.0} × {:.0}", info.size.0, info.size.1);
    if let Some(bytes) = info.file_size {
        details.push_str("  ·  ");
        details.push_str(&format_bytes(bytes));
    }
    (info.name.clone(), details)
}

/// A drawn piece of UI, kept until what it shows changes.
struct Cached<K> {
    key: Option<K>,
    pixmap: Option<Pixmap>,
}

impl<K> Default for Cached<K> {
    fn default() -> Self {
        Cached { key: None, pixmap: None }
    }
}

impl<K: PartialEq> Cached<K> {
    fn get(&mut self, key: K, draw: impl FnOnce() -> Option<Pixmap>) -> Option<&Pixmap> {
        if self.key.as_ref() != Some(&key) {
            self.pixmap = draw();
            self.key = Some(key);
        }
        self.pixmap.as_ref()
    }
}

#[derive(PartialEq)]
struct ToolbarKey {
    scale: f32,
    theme: Theme,
    zoom: String,
    background: Background,
    nav_enabled: bool,
    hovered: Option<Button>,
    pressed: Option<Button>,
}

#[derive(PartialEq)]
struct EditKey {
    scale: f32,
    theme: Theme,
    label: String,
    size: (i32, i32, i32),
    hovered: Option<Button>,
    pressed: Option<Button>,
    menu_open: bool,
}

#[derive(PartialEq)]
struct MenuKey {
    scale: f32,
    theme: Theme,
    rows: Vec<MenuRow>,
    checked: Option<usize>,
    highlighted: Option<usize>,
    size: (i32, i32),
}

#[derive(PartialEq)]
struct InfoKey {
    scale: f32,
    theme: Theme,
    max_width: i32,
    label: (String, String),
}

#[derive(PartialEq)]
struct EmptyKey {
    scale: f32,
    theme: Theme,
    layout: (i32, i32, i32, i32),
    hovered: bool,
    pressed: bool,
    file_hovering: bool,
}

/// Draws the UI and keeps each drawn piece until it changes.
#[derive(Default)]
pub struct Ui {
    toolbar: Cached<ToolbarKey>,
    info: Cached<InfoKey>,
    edit: Cached<EditKey>,
    menu: Cached<MenuKey>,
    empty: Cached<EmptyKey>,
}

impl Ui {
    pub fn draw(&mut self, text: &TextRenderer, scene: &Scene, dst: &mut [u32]) {
        let scale = scale_or_one(scene.scale);
        let palette = scene.theme.palette();
        if let Some(info) = &scene.info {
            let label = info_label(info);
            let max_width = info_max_width(scene, scale);
            let key = InfoKey { scale, theme: scene.theme, max_width, label };
            let pixmap = self.info.get(key, || draw_info(text, palette, &info_label(info), max_width, scale));
            if let Some(pixmap) = pixmap {
                let edge = (EDGE * scale).round() as i64;
                composite(dst, scene.window, pixmap, edge, edge);
            }

            let layout = toolbar_layout(scene.window, scale);
            let key = ToolbarKey {
                scale,
                theme: scene.theme,
                zoom: format_zoom(scene.zoom_percent),
                background: scene.background,
                nav_enabled: scene.nav_enabled,
                hovered: scene.hovered,
                pressed: scene.pressed,
            };
            let pixmap = self.toolbar.get(key, || draw_toolbar(text, palette, scene, &layout, scale));
            if let Some(pixmap) = pixmap {
                composite(dst, scene.window, pixmap, layout.panel.x.into(), layout.panel.y.into());
            }

            if let Some(edit) = &scene.edit {
                let layout = edit_layout(scene.window, scale, edit);
                let own = |button: Option<Button>| button.filter(|b| matches!(b, Button::Edit | Button::EditMenu));
                let key = EditKey {
                    scale,
                    theme: scene.theme,
                    label: edit.label.clone(),
                    size: (layout.panel.w, layout.panel.h, layout.main.w),
                    hovered: own(scene.hovered),
                    pressed: own(scene.pressed),
                    menu_open: scene.menu.is_some(),
                };
                let pixmap = self.edit.get(key, || draw_edit(text, palette, scene, edit, &layout, scale));
                if let Some(pixmap) = pixmap {
                    composite(dst, scene.window, pixmap, layout.panel.x.into(), layout.panel.y.into());
                }
                // The menu goes on top of everything else.
                if let Some(menu) = &scene.menu {
                    let layout = menu_layout(scene.window, scale, edit, menu);
                    let key = MenuKey {
                        scale,
                        theme: scene.theme,
                        rows: menu.rows.clone(),
                        checked: menu.checked,
                        highlighted: menu.highlighted,
                        size: (layout.panel.w, layout.panel.h),
                    };
                    let pixmap = self.menu.get(key, || draw_menu(text, palette, menu, &layout, scale));
                    if let Some(pixmap) = pixmap {
                        composite(dst, scene.window, pixmap, layout.panel.x.into(), layout.panel.y.into());
                    }
                }
            }
        } else if scene.show_empty {
            let layout = empty_layout(scene.window, scale);
            let area = layout.area;
            let hovered = scene.hovered == Some(Button::Open);
            let key = EmptyKey {
                scale,
                theme: scene.theme,
                layout: (area.w, area.h, layout.button.x - area.x, layout.button.y - area.y),
                hovered,
                pressed: hovered && scene.pressed == Some(Button::Open),
                file_hovering: scene.file_hovering,
            };
            let pixmap = self.empty.get(key, || draw_empty(text, palette, scene, &layout, scale));
            if let Some(pixmap) = pixmap {
                composite(dst, scene.window, pixmap, area.x.into(), area.y.into());
            }
        }
    }
}

fn paint(rgba: [u8; 4]) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]);
    paint.anti_alias = true;
    paint
}

fn opaque(rgb: [u8; 3]) -> [u8; 4] {
    [rgb[0], rgb[1], rgb[2], 0xFF]
}

fn fill_rounded(pixmap: &mut Pixmap, rect: Rect, radius: f32, color: [u8; 4]) {
    if let Some(path) = rounded_rect(rect.x as f32, rect.y as f32, rect.w as f32, rect.h as f32, radius) {
        pixmap.fill_path(&path, &paint(color), FillRule::Winding, Transform::identity(), None);
    }
}

/// A panel: a rounded fill with a hairline border just inside it.
fn draw_panel(pixmap: &mut Pixmap, palette: &Palette, scale: f32) {
    let (w, h) = (pixmap.width() as f32, pixmap.height() as f32);
    let radius = PANEL_RADIUS * scale;
    if let Some(path) = rounded_rect(0.0, 0.0, w, h, radius) {
        pixmap.fill_path(&path, &paint(palette.panel), FillRule::Winding, Transform::identity(), None);
    }
    if let Some(path) = rounded_rect(0.5, 0.5, w - 1.0, h - 1.0, radius - 0.5) {
        let stroke = Stroke { width: 1.0, ..Stroke::default() };
        pixmap.stroke_path(&path, &paint(palette.border), &stroke, Transform::identity(), None);
    }
}

fn draw_toolbar(
    text: &TextRenderer,
    palette: &Palette,
    scene: &Scene,
    layout: &ToolbarLayout,
    scale: f32,
) -> Option<Pixmap> {
    let panel = layout.panel;
    let mut pixmap = Pixmap::new(panel.w.max(1) as u32, panel.h.max(1) as u32)?;
    draw_panel(&mut pixmap, palette, scale);

    for separator in &layout.separators {
        let r = separator.offset(-panel.x, -panel.y);
        let line = Rect { x: r.x, y: r.y, w: scale.round().max(1.0) as i32, h: r.h };
        fill_rounded(&mut pixmap, line, 0.0, palette.separator);
    }

    for &(button, rect) in &layout.buttons {
        let rect = rect.offset(-panel.x, -panel.y);
        let enabled = is_enabled(scene, button);
        if enabled && scene.hovered == Some(button) {
            let fill = if scene.pressed == Some(button) { palette.pressed } else { palette.hover };
            fill_rounded(&mut pixmap, rect, BUTTON_RADIUS * scale, fill);
        }
        if button == Button::Zoom {
            let label = format_zoom(scene.zoom_percent);
            let size = UI_FONT_SIZE * scale;
            let (ascent, line_height) = text.line_metrics(size);
            let x = rect.x as f32 + (rect.w as f32 - text.measure(&label, size)) / 2.0;
            let y = rect.y as f32 + (rect.h as f32 - line_height) / 2.0 + ascent;
            text.draw(&mut pixmap, &label, size, x.round(), y.round(), palette.text);
            continue;
        }
        let mut color = opaque(palette.icon);
        if !enabled {
            color[3] = 0x59;
        }
        draw_icon(&mut pixmap, button, scene.background, rect, color, scale);
    }
    Some(pixmap)
}

fn draw_info(
    text: &TextRenderer,
    palette: &Palette,
    (name, details): &(String, String),
    max_width: i32,
    scale: f32,
) -> Option<Pixmap> {
    let size = UI_FONT_SIZE * scale;
    let (pad_x, pad_y) = (INFO_PAD_X * scale, INFO_PAD_Y * scale);
    let gap = 10.0 * scale;
    let (ascent, line_height) = text.line_metrics(size);
    let details_w = text.measure(details, size);
    let name_room = max_width as f32 - 2.0 * pad_x - gap - details_w;
    let name = fit_text(text, name, size, name_room);
    let name_w = text.measure(&name, size);
    let width = (2.0 * pad_x + name_w + gap + details_w).ceil().min(max_width as f32).max(1.0);
    let height = (line_height + 2.0 * pad_y).ceil();
    let mut pixmap = Pixmap::new(width as u32, height as u32)?;
    draw_panel(&mut pixmap, palette, scale);
    let baseline = (pad_y + ascent).round();
    text.draw(&mut pixmap, &name, size, pad_x.round(), baseline, palette.text);
    text.draw(&mut pixmap, details, size, (pad_x + name_w + gap).round(), baseline, palette.muted);
    Some(pixmap)
}

/// The "Open in <app>" button: the label, a divider, and the ▾.
fn draw_edit(
    text: &TextRenderer,
    palette: &Palette,
    scene: &Scene,
    edit: &EditScene,
    layout: &EditLayout,
    scale: f32,
) -> Option<Pixmap> {
    let panel = layout.panel;
    let mut pixmap = Pixmap::new(panel.w.max(1) as u32, panel.h.max(1) as u32)?;
    draw_panel(&mut pixmap, palette, scale);

    let inset = (3.0 * scale).round() as i32;
    for (button, rect) in [(Button::Edit, layout.main), (Button::EditMenu, layout.arrow)] {
        let rect = rect.offset(-panel.x, -panel.y);
        // The ▾ stays lit while its menu is open.
        let open = button == Button::EditMenu && scene.menu.is_some();
        if scene.hovered == Some(button) || open {
            let fill = if open || scene.pressed == Some(button) { palette.pressed } else { palette.hover };
            let inner = Rect { x: rect.x + inset, y: rect.y + inset, w: rect.w - 2 * inset, h: rect.h - 2 * inset };
            fill_rounded(&mut pixmap, inner, BUTTON_RADIUS * scale, fill);
        }
    }

    let arrow = layout.arrow.offset(-panel.x, -panel.y);
    let margin = (7.0 * scale).round() as i32;
    let divider = Rect { x: arrow.x, y: arrow.y + margin, w: scale.round().max(1.0) as i32, h: arrow.h - 2 * margin };
    fill_rounded(&mut pixmap, divider, 0.0, palette.separator);

    let main = layout.main.offset(-panel.x, -panel.y);
    let size = UI_FONT_SIZE * scale;
    let pad_x = INFO_PAD_X * scale;
    let (ascent, line_height) = text.line_metrics(size);
    let label = fit_text_end(text, &edit.label, size, main.w as f32 - 2.0 * pad_x);
    let baseline = main.y as f32 + (main.h as f32 - line_height) / 2.0 + ascent;
    text.draw(&mut pixmap, &label, size, (main.x as f32 + pad_x).round(), baseline.round(), palette.text);

    let icon_size = (16.0 * scale).round() as i32;
    let icon_rect = Rect {
        x: arrow.x + (arrow.w - icon_size) / 2,
        y: arrow.y + (arrow.h - icon_size) / 2,
        w: icon_size,
        h: icon_size,
    };
    stroke_icon(&mut pixmap, &button_icon(Button::EditMenu), icon_rect, opaque(palette.icon), 2.4);
    Some(pixmap)
}

/// The app menu: one row per app, a check mark on the default, and
/// "Other app…" after a separator.
fn draw_menu(text: &TextRenderer, palette: &Palette, menu: &MenuScene, layout: &MenuLayout, scale: f32) -> Option<Pixmap> {
    let panel = layout.panel;
    let mut pixmap = Pixmap::new(panel.w.max(1) as u32, panel.h.max(1) as u32)?;
    draw_panel(&mut pixmap, palette, scale);

    if let Some(separator) = layout.separator {
        let line = Rect { h: scale.round().max(1.0) as i32, ..separator.offset(-panel.x, -panel.y) };
        fill_rounded(&mut pixmap, line, 0.0, palette.separator);
    }

    let size = UI_FONT_SIZE * scale;
    let (ascent, line_height) = text.line_metrics(size);
    let text_x = (MENU_TEXT_X - MENU_PAD) * scale;
    let icon_size = (16.0 * scale).round() as i32;
    for (i, (row, rect)) in menu.rows.iter().zip(&layout.rows).enumerate() {
        let rect = rect.offset(-panel.x, -panel.y);
        if row.enabled && menu.highlighted == Some(i) {
            fill_rounded(&mut pixmap, rect, BUTTON_RADIUS * scale, palette.hover);
        }
        if menu.checked == Some(i) {
            let icon_rect = Rect {
                x: rect.x + (9.0 * scale).round() as i32,
                y: rect.y + (rect.h - icon_size) / 2,
                w: icon_size,
                h: icon_size,
            };
            stroke_icon(&mut pixmap, &check_mark(), icon_rect, opaque(palette.text), 2.4);
        }
        let room = rect.w as f32 - text_x - INFO_PAD_X * scale;
        let label = fit_text_end(text, &row.label, size, room);
        let color = if row.enabled { palette.text } else { palette.muted };
        let baseline = rect.y as f32 + (rect.h as f32 - line_height) / 2.0 + ascent;
        text.draw(&mut pixmap, &label, size, (rect.x as f32 + text_x).round(), baseline.round(), color);
    }
    Some(pixmap)
}

/// Shortens `s` with an ellipsis at the end until it fits `max_width`.
fn fit_text_end(text: &TextRenderer, s: &str, size: f32, max_width: f32) -> String {
    if text.measure(s, size) <= max_width {
        return s.to_owned();
    }
    let chars: Vec<char> = s.chars().collect();
    for keep in (0..chars.len()).rev() {
        let candidate: String = chars[..keep].iter().collect::<String>().trim_end().to_owned() + "…";
        if text.measure(&candidate, size) <= max_width {
            return candidate;
        }
    }
    "…".to_owned()
}

/// Shortens `s` with an ellipsis in the middle until it fits `max_width`,
/// so the file extension stays visible.
fn fit_text(text: &TextRenderer, s: &str, size: f32, max_width: f32) -> String {
    if text.measure(s, size) <= max_width {
        return s.to_owned();
    }
    let chars: Vec<char> = s.chars().collect();
    let tail = chars.len().min(8);
    for keep in (0..chars.len() - tail).rev() {
        let candidate: String = chars[..keep].iter().chain(['…'].iter()).chain(&chars[chars.len() - tail..]).collect();
        if text.measure(&candidate, size) <= max_width {
            return candidate;
        }
    }
    "…".to_owned()
}

fn draw_empty(
    text: &TextRenderer,
    palette: &Palette,
    scene: &Scene,
    layout: &EmptyLayout,
    scale: f32,
) -> Option<Pixmap> {
    let area = layout.area;
    if area.w <= 0 || area.h <= 0 {
        return None;
    }
    let mut pixmap = Pixmap::new(area.w as u32, area.h as u32)?;
    let (w, h) = (area.w as f32, area.h as f32);

    // A dashed outline that lights up while a file is dragged over the window.
    let border = if scene.file_hovering { palette.drop_active } else { palette.drop_border };
    if scene.file_hovering {
        let mut glow = border;
        glow[3] = 0x1A;
        fill_rounded(&mut pixmap, Rect { x: 0, y: 0, w: area.w, h: area.h }, 14.0 * scale, glow);
    }
    let width = 1.5 * scale;
    if let Some(path) = rounded_rect(width / 2.0, width / 2.0, w - width, h - width, 14.0 * scale) {
        let stroke = Stroke {
            width,
            dash: StrokeDash::new(vec![6.0 * scale, 5.0 * scale], 0.0),
            ..Stroke::default()
        };
        pixmap.stroke_path(&path, &paint(border), &stroke, Transform::identity(), None);
    }

    let icon_size = 40.0 * scale;
    let icon_rect = Rect {
        x: ((w - icon_size) / 2.0).round() as i32,
        y: (26.0 * scale).round() as i32,
        w: icon_size as i32,
        h: icon_size as i32,
    };
    let icon_color = if scene.file_hovering { palette.drop_active } else { opaque(palette.muted) };
    stroke_icon(&mut pixmap, &picture_icon(), icon_rect, icon_color, 1.5);

    let centered = |pixmap: &mut Pixmap, s: &str, size: f32, baseline: f32, color: [u8; 3]| {
        let x = ((w - text.measure(s, size)) / 2.0).round();
        text.draw(pixmap, s, size, x, baseline.round(), color);
    };
    let paste_key = if cfg!(target_os = "macos") { "Cmd+V" } else { "Ctrl+V" };
    centered(&mut pixmap, "Drop an SVG file here", 16.0 * scale, 98.0 * scale, palette.text);
    centered(&mut pixmap, &format!("or paste one with {paste_key}"), UI_FONT_SIZE * scale, 120.0 * scale, palette.muted);

    let button = layout.button.offset(-area.x, -area.y);
    let hovered = scene.hovered == Some(Button::Open);
    let fill = if hovered { palette.accent_hover } else { palette.accent };
    fill_rounded(&mut pixmap, button, 8.0 * scale, fill);
    let size = 14.0 * scale;
    let (ascent, line_height) = text.line_metrics(size);
    let label = "Open file…";
    let x = button.x as f32 + (button.w as f32 - text.measure(label, size)) / 2.0;
    let y = button.y as f32 + (button.h as f32 - line_height) / 2.0 + ascent;
    text.draw(&mut pixmap, label, size, x.round(), y.round(), WHITE);
    Some(pixmap)
}

/// Builds a path in a 24 × 24 icon grid.
fn icon(build: impl FnOnce(&mut PathBuilder)) -> Option<Path> {
    let mut pb = PathBuilder::new();
    build(&mut pb);
    pb.finish()
}

fn polyline(pb: &mut PathBuilder, points: &[(f32, f32)]) {
    if let Some(&(x, y)) = points.first() {
        pb.move_to(x, y);
        for &(x, y) in &points[1..] {
            pb.line_to(x, y);
        }
    }
}

fn picture_icon() -> Option<Path> {
    icon(|pb| {
        if let Some(rect) = rounded_rect(3.0, 4.5, 18.0, 15.0, 2.0) {
            pb.push_path(&rect);
        }
        pb.push_circle(8.5, 9.5, 1.6);
        polyline(pb, &[(3.5, 17.5), (9.0, 12.5), (12.5, 15.5), (15.5, 12.5), (20.5, 17.0)]);
    })
}

fn magnifier(pb: &mut PathBuilder) {
    pb.push_circle(10.5, 10.5, 6.5);
    polyline(pb, &[(15.3, 15.3), (20.0, 20.0)]);
    polyline(pb, &[(7.5, 10.5), (13.5, 10.5)]);
}

fn button_icon(button: Button) -> Option<Path> {
    match button {
        Button::Open => icon(|pb| {
            polyline(pb, &[(3.0, 6.5), (3.0, 18.5), (21.0, 18.5), (21.0, 8.5), (12.0, 8.5), (10.0, 5.5), (3.0, 5.5), (3.0, 6.5)]);
            polyline(pb, &[(3.0, 11.0), (21.0, 11.0)]);
        }),
        Button::EditMenu => icon(|pb| polyline(pb, &[(7.0, 10.0), (12.0, 15.0), (17.0, 10.0)])),
        Button::Previous => icon(|pb| polyline(pb, &[(14.5, 6.0), (8.5, 12.0), (14.5, 18.0)])),
        Button::Next => icon(|pb| polyline(pb, &[(9.5, 6.0), (15.5, 12.0), (9.5, 18.0)])),
        Button::ZoomOut => icon(magnifier),
        Button::ZoomIn => icon(|pb| {
            magnifier(pb);
            polyline(pb, &[(10.5, 7.5), (10.5, 13.5)]);
        }),
        Button::Fit => icon(|pb| {
            polyline(pb, &[(4.0, 9.0), (4.0, 4.0), (9.0, 4.0)]);
            polyline(pb, &[(15.0, 4.0), (20.0, 4.0), (20.0, 9.0)]);
            polyline(pb, &[(20.0, 15.0), (20.0, 20.0), (15.0, 20.0)]);
            polyline(pb, &[(9.0, 20.0), (4.0, 20.0), (4.0, 15.0)]);
        }),
        Button::Background => icon(|pb| {
            polyline(pb, &[(4.0, 4.0), (20.0, 4.0), (20.0, 20.0), (4.0, 20.0)]);
            pb.close();
        }),
        Button::Zoom | Button::Edit => None,
    }
}

fn check_mark() -> Option<Path> {
    icon(|pb| polyline(pb, &[(5.0, 12.5), (10.0, 17.5), (19.0, 7.5)]))
}

/// The transform that maps the 24 × 24 icon grid into `rect`, at `ICON` size.
fn icon_transform(rect: Rect, size: f32) -> Transform {
    let s = size / 24.0;
    let x = rect.x as f32 + (rect.w as f32 - size) / 2.0;
    let y = rect.y as f32 + (rect.h as f32 - size) / 2.0;
    Transform::from_row(s, 0.0, 0.0, s, x.round(), y.round())
}

fn stroke_icon(pixmap: &mut Pixmap, path: &Option<Path>, rect: Rect, color: [u8; 4], width: f32) {
    let Some(path) = path else { return };
    let stroke = Stroke {
        width,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    };
    let size = rect.w.min(rect.h) as f32;
    pixmap.stroke_path(path, &paint(color), &stroke, icon_transform(rect, size), None);
}

fn draw_icon(pixmap: &mut Pixmap, button: Button, background: Background, rect: Rect, color: [u8; 4], scale: f32) {
    let size = ICON * scale;
    let icon_rect = Rect {
        x: rect.x + ((rect.w as f32 - size) / 2.0).round() as i32,
        y: rect.y + ((rect.h as f32 - size) / 2.0).round() as i32,
        w: size.round() as i32,
        h: size.round() as i32,
    };
    stroke_icon(pixmap, &button_icon(button), icon_rect, color, 1.8);

    // The Background button shows the current choice inside its square.
    if button == Button::Background {
        let transform = icon_transform(icon_rect, icon_rect.w as f32);
        let squares: &[(f32, f32, f32, f32)] = match background {
            Background::Checkerboard => &[(4.0, 4.0, 8.0, 8.0), (12.0, 12.0, 8.0, 8.0)],
            Background::Dark => &[(4.0, 4.0, 16.0, 16.0)],
            Background::White => &[],
        };
        for &(x, y, w, h) in squares {
            if let Some(rect) = resvg::tiny_skia::Rect::from_xywh(x, y, w, h) {
                pixmap.fill_rect(rect, &paint(color), transform, None);
            }
        }
    }
}

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
        let max_width = (size.0 as f32 - 2.0 * EDGE * scale).max(1.0) as u32;
        let cached = matches!(&self.image, Some((s, w, _)) if *s == scale && *w == max_width);
        if !cached {
            self.image = banner_image(text, &self.message, scale, max_width as f32)
                .map(|image| (scale, max_width, image));
        }
        let Some((_, _, image)) = &self.image else { return };
        let x = (size.0 as i64 - i64::from(image.width())) / 2;
        let y = (EDGE * scale) as i64;
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
    let bounds = Rect { x: 0, y: 0, w: width as i32, h: height as i32 };
    fill_rounded(&mut pixmap, bounds, BANNER_RADIUS * scale, BANNER_FILL);
    text.draw(&mut pixmap, message, font_size, pad_x, pad_y + ascent, WHITE);
    Some(pixmap)
}

fn rounded_rect(x: f32, y: f32, width: f32, height: f32, radius: f32) -> Option<Path> {
    let r = radius.min(width / 2.0).min(height / 2.0).max(0.0);
    // Control point distance for a quarter circle drawn with one cubic curve.
    let k = r * 0.552_284_8;
    let (right, bottom) = (x + width, y + height);
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(right - r, y);
    pb.cubic_to(right - r + k, y, right, y + r - k, right, y + r);
    pb.line_to(right, bottom - r);
    pb.cubic_to(right, bottom - r + k, right - r + k, bottom, right - r, bottom);
    pb.line_to(x + r, bottom);
    pb.cubic_to(x + r - k, bottom, x, bottom - r + k, x, bottom - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
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

    fn scene(info: Option<Info>, scale: f32) -> Scene {
        Scene {
            window: (1000, 700),
            scale,
            theme: Theme::Light,
            info,
            show_empty: true,
            zoom_percent: 100.0,
            background: Background::Checkerboard,
            nav_enabled: false,
            edit: None,
            menu: None,
            hovered: None,
            pressed: None,
            file_hovering: false,
        }
    }

    fn info() -> Info {
        Info { name: "icon.svg".into(), size: (24.0, 24.0), file_size: Some(266) }
    }

    fn center(rect: Rect) -> (f32, f32) {
        (rect.x as f32 + rect.w as f32 / 2.0, rect.y as f32 + rect.h as f32 / 2.0)
    }

    #[test]
    fn toolbar_is_centered_at_the_bottom() {
        for scale in [1.0, 1.25, 2.0] {
            let window = (1000, 700);
            let layout = toolbar_layout(window, scale);
            let left = layout.panel.x;
            let right = window.0 as i32 - (layout.panel.x + layout.panel.w);
            assert!((left - right).abs() <= 2, "{left} vs {right} at {scale}");
            let bottom = window.1 as i32 - (layout.panel.y + layout.panel.h);
            assert!((bottom as f32 - TOOLBAR_BOTTOM * scale).abs() <= 1.0);
            assert_eq!(layout.buttons.len(), 8);
            assert_eq!(layout.separators.len(), 3);
        }
    }

    #[test]
    fn toolbar_buttons_are_in_order_and_inside_the_panel() {
        let layout = toolbar_layout((1000, 700), 1.5);
        let order: Vec<Button> = layout.buttons.iter().map(|(b, _)| *b).collect();
        use Button::*;
        assert_eq!(order, vec![Open, Previous, Next, ZoomOut, Zoom, ZoomIn, Fit, Background]);
        for pair in layout.buttons.windows(2) {
            assert!(pair[0].1.x + pair[0].1.w <= pair[1].1.x);
        }
        for (_, rect) in &layout.buttons {
            assert!(rect.x >= layout.panel.x && rect.x + rect.w <= layout.panel.x + layout.panel.w);
        }
    }

    #[test]
    fn hit_testing() {
        let s = scene(Some(info()), 1.0);
        let layout = toolbar_layout(s.window, 1.0);
        for &(button, rect) in &layout.buttons {
            let expected = if matches!(button, Button::Previous | Button::Next) {
                Hit::Inert
            } else {
                Hit::Button(button)
            };
            assert_eq!(hit(&s, center(rect)), Some(expected));
        }
        // On the panel between buttons, and outside it.
        let panel = layout.panel;
        assert_eq!(hit(&s, (panel.x as f32 + 1.0, panel.y as f32 + 1.0)), Some(Hit::Inert));
        assert_eq!(hit(&s, (5.0, 5.0)), None);
    }

    fn edit() -> EditScene {
        EditScene { label: "Open in Inkscape".into(), label_size: (104.0, 17.7) }
    }

    fn menu(rows: &[(&str, bool)]) -> MenuScene {
        MenuScene {
            rows: rows.iter().map(|&(label, enabled)| MenuRow { label: label.into(), enabled }).collect(),
            checked: Some(0),
            highlighted: None,
            text_width: 120.0,
        }
    }

    #[test]
    fn edit_button_is_in_the_top_right_corner() {
        for scale in [1.0, 1.25, 2.0] {
            let window = (1000, 700);
            let layout = edit_layout(window, scale, &edit());
            let right = window.0 as i32 - (layout.panel.x + layout.panel.w);
            assert!((right as f32 - EDGE * scale).abs() <= 1.0, "{right} at {scale}");
            assert!((layout.panel.y as f32 - EDGE * scale).abs() <= 1.0);
            // The main part and the ▾ fill the panel side by side.
            assert_eq!(layout.main.x, layout.panel.x);
            assert_eq!(layout.main.x + layout.main.w, layout.arrow.x);
            assert!((layout.arrow.x + layout.arrow.w - (layout.panel.x + layout.panel.w)).abs() <= 1);
        }
    }

    #[test]
    fn menu_hangs_under_the_button_inside_the_window() {
        let rows = [("Inkscape", true), ("Visual Studio Code", true), ("Other app…", true)];
        for scale in [1.0, 1.5] {
            let window = (1000, 700);
            let button = edit_layout(window, scale, &edit());
            let layout = menu_layout(window, scale, &edit(), &menu(&rows));
            let (panel, button_right) = (layout.panel, button.panel.x + button.panel.w);
            assert!((panel.x + panel.w - button_right).abs() <= 1, "right-aligned at {scale}");
            assert!(panel.y > button.panel.y + button.panel.h);
            assert!(panel.x >= 0 && panel.x + panel.w <= window.0 as i32);
            assert_eq!(layout.rows.len(), 3);
            for row in &layout.rows {
                assert!(row.x >= panel.x && row.x + row.w <= panel.x + panel.w);
                assert!(row.y >= panel.y && row.y + row.h <= panel.y + panel.h);
            }
            // "Other app…" comes after a separator.
            let separator = layout.separator.expect("a separator");
            assert!(separator.y >= layout.rows[1].y + layout.rows[1].h && separator.y <= layout.rows[2].y);
        }
        // The widest label fits its row without being shortened.
        for scale in [1.0, 1.25, 2.0] {
            let mut wide = menu(&rows);
            wide.text_width = 260.0;
            let layout = menu_layout((1600, 900), scale, &edit(), &wide);
            let room = layout.rows[0].w as f32 - (MENU_TEXT_X - MENU_PAD + INFO_PAD_X) * scale;
            assert!(room >= 260.0 * scale, "{room} at {scale}");
        }
        // A single row needs no separator.
        let layout = menu_layout((1000, 700), 1.0, &edit(), &menu(&[("Other app…", true)]));
        assert_eq!(layout.separator, None);
    }

    #[test]
    fn edit_button_hit_testing() {
        let mut s = scene(Some(info()), 1.0);
        s.edit = Some(edit());
        let layout = edit_layout(s.window, 1.0, &edit());
        assert_eq!(hit(&s, center(layout.main)), Some(Hit::Button(Button::Edit)));
        assert_eq!(hit(&s, center(layout.arrow)), Some(Hit::Button(Button::EditMenu)));
        // Without the button, the same point is just the picture.
        s.edit = None;
        assert_eq!(hit(&s, center(layout.main)), None);
    }

    #[test]
    fn open_menu_hit_testing() {
        let mut s = scene(Some(info()), 1.0);
        s.edit = Some(edit());
        s.menu = Some(menu(&[("Looking for apps…", false), ("Other app…", true)]));
        let layout = menu_layout(s.window, 1.0, &edit(), s.menu.as_ref().unwrap());
        assert_eq!(hit(&s, center(layout.rows[0])), Some(Hit::Inert));
        assert_eq!(hit(&s, center(layout.rows[1])), Some(Hit::MenuItem(1)));
        let panel = layout.panel;
        assert_eq!(hit(&s, (panel.x as f32 + 1.0, panel.y as f32 + 1.0)), Some(Hit::Inert));
        // The ▾ closes the menu again; anywhere else only dismisses it,
        // even over the toolbar.
        let arrow = edit_layout(s.window, 1.0, &edit()).arrow;
        assert_eq!(hit(&s, center(arrow)), Some(Hit::Button(Button::EditMenu)));
        assert_eq!(hit(&s, (5.0, 5.0)), Some(Hit::Dismiss));
        let toolbar = toolbar_layout(s.window, 1.0);
        assert_eq!(hit(&s, center(toolbar.buttons[0].1)), Some(Hit::Dismiss));
    }

    #[test]
    fn info_bar_stays_clear_of_the_edit_button() {
        let mut s = scene(Some(info()), 1.25);
        let full = info_max_width(&s, 1.25);
        s.edit = Some(edit());
        let narrow = info_max_width(&s, 1.25);
        let button = edit_layout(s.window, 1.25, &edit()).panel;
        assert!((EDGE * 1.25) as i32 + narrow < button.x, "{narrow} reaches the button at {}", button.x);
        assert!(narrow < full);
    }

    #[test]
    fn empty_screen_open_button() {
        let s = scene(None, 2.0);
        let layout = empty_layout(s.window, 2.0);
        assert_eq!(hit(&s, center(layout.button)), Some(Hit::Button(Button::Open)));
        assert_eq!(hit(&s, center(layout.area)), None);
        let loading = Scene { show_empty: false, ..scene(None, 2.0) };
        assert_eq!(hit(&loading, center(layout.button)), None);
    }

    #[test]
    fn empty_layout_fits_small_windows() {
        let layout = empty_layout((200, 150), 1.0);
        assert!(layout.area.w <= 200 - 48 && layout.area.h <= 150 - 48);
        assert!(layout.area.w >= 0 && layout.area.h >= 0);
    }

    #[test]
    fn formats() {
        assert_eq!(format_zoom(100.0), "100%");
        assert_eq!(format_zoom(8533.3), "8533%");
        assert_eq!(format_zoom(12.4), "12%");
        assert_eq!(format_zoom(6.25), "6.2%");
        assert_eq!(format_bytes(266), "266 bytes");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(12_600_000), "12.0 MB");
    }

    #[test]
    fn info_label_parts() {
        let (name, details) = info_label(&info());
        assert_eq!(name, "icon.svg");
        assert_eq!(details, "24 × 24  ·  266 bytes");
        let pasted = Info { name: "Pasted SVG".into(), size: (188.98, 94.49), file_size: None };
        assert_eq!(info_label(&pasted).1, "189 × 94");
    }

    #[test]
    fn long_names_are_shortened_in_the_middle() {
        let text = TextRenderer::new().unwrap();
        let name = "a-very-long-file-name-for-a-drawing-of-the-city-map.svg";
        let short = fit_text(&text, name, 13.0, 150.0);
        assert!(text.measure(&short, 13.0) <= 150.0);
        assert!(short.contains('…') && short.ends_with("-map.svg"), "{short}");
        assert_eq!(fit_text(&text, "icon.svg", 13.0, 150.0), "icon.svg");
    }

    #[test]
    fn background_cycles() {
        let b = Background::Checkerboard;
        assert_eq!(b.next().next().next(), b);
        assert_ne!(b.next(), b);
    }

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

    #[test]
    fn everything_draws_in_both_themes() {
        let text = TextRenderer::new().unwrap();
        for theme in [Theme::Light, Theme::Dark] {
            for with_file in [true, false] {
                let mut s = scene(with_file.then(info), 1.5);
                s.theme = theme;
                s.hovered = Some(Button::Open);
                if with_file {
                    s.edit = Some(edit());
                    s.menu = Some(menu(&[("Inkscape", true), ("Other app…", true)]));
                }
                let mut dst = vec![0; 1000 * 700];
                Ui::default().draw(&text, &s, &mut dst);
                assert!(dst.iter().any(|&p| p != 0));
            }
        }
    }

    #[test]
    fn long_app_names_are_cut_at_the_end() {
        let text = TextRenderer::new().unwrap();
        let name = "Inkscape vector graphics editor";
        assert_eq!(fit_text_end(&text, name, 13.0, 1000.0), name);
        let short = fit_text_end(&text, name, 13.0, 100.0);
        assert!(short.starts_with("Inkscape") && short.ends_with('…'), "{short}");
        assert!(text.measure(&short, 13.0) <= 100.0);
        assert_eq!(fit_text_end(&text, name, 13.0, 1.0), "…");
    }
}
