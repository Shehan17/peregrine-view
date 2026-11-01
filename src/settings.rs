//! Saves the background choice in the user's config folder.

use std::path::PathBuf;

use crate::ui::Background;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    pub background: Background,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { background: Background::Checkerboard }
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
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else { continue };
        if key.trim() == "background" {
            match value.trim() {
                "checkerboard" => settings.background = Background::Checkerboard,
                "white" => settings.background = Background::White,
                "dark" => settings.background = Background::Dark,
                _ => {}
            }
        }
    }
    settings
}

fn serialize(settings: &Settings) -> String {
    let background = match settings.background {
        Background::Checkerboard => "checkerboard",
        Background::White => "white",
        Background::Dark => "dark",
    };
    format!("background={background}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        for background in [Background::Checkerboard, Background::White, Background::Dark] {
            let settings = Settings { background };
            assert_eq!(parse(&serialize(&settings)), settings);
        }
    }

    #[test]
    fn bad_or_unknown_lines_are_ignored() {
        assert_eq!(parse(""), Settings::default());
        assert_eq!(parse("background=purple\nnonsense\nother=1"), Settings::default());
        assert_eq!(parse(" background = dark \r\n").background, Background::Dark);
    }
}
