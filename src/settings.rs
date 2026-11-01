//! Saves the background choice and the default editor in the user's config folder.

use std::path::PathBuf;

use crate::platform::Editor;
use crate::ui::Background;

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub background: Background,
    /// The app the "Open in" button uses: the last one picked from its menu.
    pub editor: Option<Editor>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { background: Background::Checkerboard, editor: None }
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
    let Some(path) = settings_path() else { return };
    let spawned = std::thread::Builder::new().name("settings".into()).spawn(move || {
        let write = || -> std::io::Result<()> {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            // Write a temporary file and rename it, so a crash never leaves half a file.
            let temp = path.with_extension("txt.tmp");
            std::fs::write(&temp, serialize(&settings))?;
            std::fs::rename(&temp, &path)
        };
        if let Err(e) = write() {
            eprintln!("could not save settings to {}: {e}", path.display());
        }
    });
    if let Err(e) = spawned {
        eprintln!("could not start settings thread: {e}");
    }
}

fn parse(text: &str) -> Settings {
    let mut settings = Settings::default();
    let (mut editor_id, mut editor_name) = (None, None);
    for line in text.lines() {
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
            _ => {}
        }
    }
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
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        for background in [Background::Checkerboard, Background::White, Background::Dark] {
            let settings = Settings { background, editor: None };
            assert_eq!(parse(&serialize(&settings)), settings);
        }
        let editor = Editor {
            id: r"C:\Program Files\Inkscape\bin\inkscape.exe".into(),
            name: "Inkscape vector graphics editor".into(),
        };
        let settings = Settings { background: Background::Dark, editor: Some(editor) };
        assert_eq!(parse(&serialize(&settings)), settings);
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
    }
}
