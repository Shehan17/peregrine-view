//! Saves the background choice, the default editor and the window's place
//! in the user's config folder.

use std::path::PathBuf;

use crate::platform::Editor;
use crate::ui::Background;

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub background: Background,
    /// The app the "Open in" button uses: the last one picked from its menu.
    pub editor: Option<Editor>,
    /// Where the window was when the app last closed.
    pub window: Option<WindowPlace>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { background: Background::Checkerboard, editor: None, window: None }
    }
}

/// The window's place on screen, when it isn't maximized or full screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowPlace {
    /// The top left corner of the outer frame, in physical pixels. None
    /// where the system doesn't tell apps, as on Wayland.
    pub position: Option<(i32, i32)>,
    /// The inner size in logical pixels, so the window keeps its size on a
    /// screen with a different scale.
    pub size: (u32, u32),
    /// True when the window was maximized; `position` and `size` are then
    /// where it goes back to when restored.
    pub maximized: bool,
}

/// A monitor's area in physical pixels, and its scale factor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Monitor {
    pub position: (i32, i32),
    pub size: (u32, u32),
    pub scale: f64,
}

impl WindowPlace {
    /// True when enough of the window's top edge, where the title bar is,
    /// would be on one of the `monitors` to grab it. It isn't when the
    /// monitor it was on has been unplugged.
    pub fn is_on_screen(&self, monitors: &[Monitor]) -> bool {
        /// How much of the title bar must show, in logical pixels.
        const MIN_WIDTH: f64 = 64.0;
        const MIN_HEIGHT: f64 = 16.0;
        /// How tall a title bar is taken to be, in logical pixels.
        const TITLE_HEIGHT: f64 = 32.0;
        let Some((x, y)) = self.position else { return true };
        monitors.iter().any(|m| {
            let (x, y) = (x as f64, y as f64);
            let (left, top) = (m.position.0 as f64, m.position.1 as f64);
            let (right, bottom) = (left + m.size.0 as f64, top + m.size.1 as f64);
            let width = self.size.0 as f64 * m.scale;
            let overlap_w = (x + width).min(right) - x.max(left);
            let overlap_h = (y + TITLE_HEIGHT * m.scale).min(bottom) - y.max(top);
            overlap_w >= MIN_WIDTH * m.scale && overlap_h >= MIN_HEIGHT * m.scale
        })
    }
}

/// The settings file: `%APPDATA%\PeregrineView` on Windows,
/// `~/Library/Application Support/PeregrineView` on macOS, and
/// `$XDG_CONFIG_HOME/peregrineview` (or `~/.config/peregrineview`) on Linux.
fn settings_path() -> Option<PathBuf> {
    let env_dir = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    let dir = if cfg!(windows) {
        env_dir("APPDATA")?.join("PeregrineView")
    } else if cfg!(target_os = "macos") {
        env_dir("HOME")?.join("Library/Application Support/PeregrineView")
    } else {
        env_dir("XDG_CONFIG_HOME")
            .or_else(|| env_dir("HOME").map(|home| home.join(".config")))?
            .join("peregrineview")
    };
    Some(dir.join("settings.txt"))
}

/// Reads the settings. A missing or unreadable file gives the defaults.
pub fn load() -> Settings {
    settings_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|text| parse(&text))
        .unwrap_or_default()
}

/// Writes the settings on a background thread, so the window never waits on the disk.
pub fn save(settings: Settings) {
    let spawned = std::thread::Builder::new().name("settings".into()).spawn(move || save_now(&settings));
    if let Err(e) = spawned {
        eprintln!("could not start settings thread: {e}");
    }
}

/// Writes the settings and waits until they are on disk. For when the app
/// closes, which would stop a background thread halfway.
pub fn save_now(settings: &Settings) {
    let Some(path) = settings_path() else { return };
    let write = || -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // Write a temporary file and rename it, so a crash never leaves half a file.
        let temp = path.with_extension("txt.tmp");
        std::fs::write(&temp, serialize(settings))?;
        std::fs::rename(&temp, &path)
    };
    if let Err(e) = write() {
        eprintln!("could not save settings to {}: {e}", path.display());
    }
}

fn parse(text: &str) -> Settings {
    let mut settings = Settings::default();
    let (mut editor_id, mut editor_name) = (None, None);
    let (mut position, mut size, mut maximized) = (None, None, false);
    // Some editors start the file with a byte order mark.
    for line in text.trim_start_matches('\u{feff}').lines() {
        let Some((key, value)) = line.split_once('=') else { continue };
        let value = value.trim();
        match key.trim() {
            "background" => match value {
                "checkerboard" => settings.background = Background::Checkerboard,
                "white" => settings.background = Background::White,
                "dark" => settings.background = Background::Dark,
                _ => {}
            },
            "editor" if !value.is_empty() => editor_id = Some(value.to_owned()),
            "editor_name" if !value.is_empty() => editor_name = Some(value.to_owned()),
            "window_position" => position = parse_pair(value),
            "window_size" => size = parse_pair::<u32>(value).filter(|&(w, h)| w > 0 && h > 0),
            "window_maximized" => maximized = value == "true",
            _ => {}
        }
    }
    // A window place needs at least a size.
    settings.window = size.map(|size| WindowPlace { position, size, maximized });
    // An editor needs both lines.
    if let (Some(id), Some(name)) = (editor_id, editor_name) {
        settings.editor = Some(Editor { id, name });
    }
    settings
}

fn serialize(settings: &Settings) -> String {
    let background = match settings.background {
        Background::Checkerboard => "checkerboard",
        Background::White => "white",
        Background::Dark => "dark",
    };
    let mut text = format!("background={background}\n");
    if let Some(editor) = &settings.editor {
        // A line break would split a value; real program paths and names have none.
        let line = |s: &str| s.replace(['\r', '\n'], " ");
        text.push_str(&format!("editor={}\neditor_name={}\n", line(&editor.id), line(&editor.name)));
    }
    if let Some(place) = &settings.window {
        if let Some((x, y)) = place.position {
            text.push_str(&format!("window_position={x},{y}\n"));
        }
        let (w, h) = place.size;
        text.push_str(&format!("window_size={w},{h}\nwindow_maximized={}\n", place.maximized));
    }
    text
}

/// Reads two numbers written as "a,b".
fn parse_pair<T: std::str::FromStr>(value: &str) -> Option<(T, T)> {
    let (a, b) = value.split_once(',')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        for background in [Background::Checkerboard, Background::White, Background::Dark] {
            let settings = Settings { background, ..Settings::default() };
            assert_eq!(parse(&serialize(&settings)), settings);
        }
        let editor = Editor {
            id: r"C:\Program Files\Inkscape\bin\inkscape.exe".into(),
            name: "Inkscape vector graphics editor".into(),
        };
        let window = WindowPlace { position: Some((-1500, 40)), size: (1024, 768), maximized: true };
        let settings = Settings { background: Background::Dark, editor: Some(editor), window: Some(window) };
        assert_eq!(parse(&serialize(&settings)), settings);
        // Wayland gives no position.
        let window = WindowPlace { position: None, size: (800, 600), maximized: false };
        let settings = Settings { window: Some(window), ..Settings::default() };
        assert_eq!(parse(&serialize(&settings)), settings);
    }

    #[test]
    fn a_window_place_needs_a_good_size() {
        assert_eq!(parse("window_position=10,20\n").window, None);
        assert_eq!(parse("window_size=0,600\n").window, None);
        assert_eq!(parse("window_size=800\n").window, None);
        assert_eq!(parse("window_size=-800,600\n").window, None);
        assert_eq!(parse("window_position=a,b\nwindow_size=800,600\n").window.map(|w| w.position), Some(None));
        let place = parse("window_size= 800 , 600 \nwindow_maximized=true\n").window;
        assert_eq!(place, Some(WindowPlace { position: None, size: (800, 600), maximized: true }));
    }

    #[test]
    fn window_place_on_screen() {
        let main = Monitor { position: (0, 0), size: (1920, 1080), scale: 1.0 };
        // A second screen to the left, at 150%.
        let left = Monitor { position: (-2560, 0), size: (2560, 1440), scale: 1.5 };
        let at = |x, y| WindowPlace { position: Some((x, y)), size: (1024, 768), maximized: false };
        assert!(at(100, 100).is_on_screen(&[main]));
        assert!(at(-1500, 40).is_on_screen(&[main, left]));
        // Its screen was unplugged.
        assert!(!at(-1500, 40).is_on_screen(&[main]));
        // Snapped to an edge, the frame sits a few pixels off screen.
        assert!(at(-7, 0).is_on_screen(&[main]));
        // Only the bottom of the window shows, but not its title bar.
        assert!(!at(100, -500).is_on_screen(&[main]));
        assert!(!at(100, 1075).is_on_screen(&[main]));
        // Almost all of it is off the left or right edge.
        assert!(!at(-1000, 100).is_on_screen(&[main]));
        assert!(!at(1900, 100).is_on_screen(&[main]));
        assert!(at(1800, 100).is_on_screen(&[main]));
        // No position, as on Wayland: the system places it.
        let unplaced = WindowPlace { position: None, size: (1024, 768), maximized: false };
        assert!(unplaced.is_on_screen(&[]));
    }

    #[test]
    fn an_editor_needs_an_id_and_a_name() {
        assert_eq!(parse("editor=C:\\a.exe\n").editor, None);
        assert_eq!(parse("editor_name=A\n").editor, None);
        assert_eq!(parse("editor=\neditor_name=A\n").editor, None);
        // Store app IDs can contain `=`; only the first one splits the line.
        let editor = parse("editor=App!a=b\neditor_name=A\n").editor;
        assert_eq!(editor.map(|e| e.id), Some("App!a=b".into()));
    }

    #[test]
    fn bad_or_unknown_lines_are_ignored() {
        assert_eq!(parse(""), Settings::default());
        assert_eq!(parse("background=purple\nnonsense\nother=1"), Settings::default());
        assert_eq!(parse(" background = dark \r\n").background, Background::Dark);
        assert_eq!(parse("\u{feff}background=dark\n").background, Background::Dark);
    }
}
