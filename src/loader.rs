//! Reads an SVG file and parses it into a usvg tree on a background thread.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use resvg::usvg;
use winit::event_loop::EventLoopProxy;

use crate::AppEvent;

pub struct Document {
    pub tree: usvg::Tree,
    pub path: PathBuf,
    pub file_name: String,
}

#[derive(Debug)]
pub enum LoadError {
    NotFound,
    Unreadable,
    NotSvg,
    Broken { line: u32 },
    NoSize,
    TooManyElements,
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::NotFound => write!(f, "The file was deleted or moved."),
            LoadError::Unreadable => write!(f, "This file can't be read."),
            LoadError::NotSvg => write!(f, "This file isn't an SVG."),
            LoadError::Broken { line } => write!(f, "This SVG has broken code near line {line}."),
            LoadError::NoSize => write!(f, "This SVG has no size to draw."),
            LoadError::TooManyElements => write!(f, "This SVG has too many elements to draw."),
        }
    }
}

/// Starts a thread that loads `path` and sends the result to the event loop.
pub fn spawn_load(path: PathBuf, proxy: EventLoopProxy<AppEvent>) {
    let spawned = std::thread::Builder::new()
        .name("loader".into())
        .spawn(move || {
            let result = load(&path);
            if let Err(e) = &result {
                eprintln!("load failed: {}: {e:?}", path.display());
            }
            // Fails only when the event loop has already exited.
            let _ = proxy.send_event(AppEvent::Loaded(result));
        });
    if let Err(e) = spawned {
        eprintln!("could not start loader thread: {e}");
    }
}

pub fn load(path: &Path) -> Result<Document, LoadError> {
    let start = Instant::now();
    let data = std::fs::read(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => LoadError::NotFound,
        _ => {
            eprintln!("read failed: {}: {e}", path.display());
            LoadError::Unreadable
        }
    })?;

    let is_gzip = data.starts_with(&[0x1f, 0x8b]);
    if !is_gzip && !contains(&data, b"<svg") {
        return Err(LoadError::NotSvg);
    }

    let mut options = usvg::Options {
        resources_dir: path.parent().map(Path::to_path_buf),
        ..usvg::Options::default()
    };
    // System fonts are slow to load, so only load them when the SVG has text.
    // Compressed files can't be scanned cheaply, so assume they might.
    if is_gzip || contains(&data, b"<text") {
        let font_start = Instant::now();
        let mut fontdb = usvg::fontdb::Database::new();
        fontdb.load_system_fonts();
        eprintln!("loaded {} fonts in {:?}", fontdb.len(), font_start.elapsed());
        options.fontdb = Arc::new(fontdb);
    }

    let tree = usvg::Tree::from_data(&data, &options).map_err(|e| match e {
        usvg::Error::ParsingFailed(e) => LoadError::Broken { line: e.pos().row },
        usvg::Error::InvalidSize => LoadError::NoSize,
        usvg::Error::ElementsLimitReached => LoadError::TooManyElements,
        usvg::Error::NotAnUtf8Str
        | usvg::Error::MalformedGZip
        | usvg::Error::SvgzFeatureNotEnabled => LoadError::NotSvg,
    })?;

    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    eprintln!("parsed {file_name} in {:?}", start.elapsed());
    Ok(Document { tree, path: path.to_path_buf(), file_name })
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}
