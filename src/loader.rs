//! Reads an SVG and parses it into a usvg tree on a background thread.
//! Handles gzip, text encodings and the spec's size rules.

use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use resvg::tiny_skia::Transform;
use resvg::usvg;
use winit::event_loop::EventLoopProxy;

use crate::AppEvent;

/// Unzipped files larger than this are refused, which also stops zip bombs.
const MAX_UNZIPPED_BYTES: u64 = 512 * 1024 * 1024;

/// Where an SVG comes from.
pub enum Source {
    File(PathBuf),
    /// SVG code pasted from the clipboard.
    Text(String),
}

pub struct Document {
    pub tree: Arc<usvg::Tree>,
    /// The file, or None for pasted SVG code.
    pub path: Option<PathBuf>,
    pub name: String,
    /// The picture's size in CSS pixels, as shown at 100%.
    pub size: (f32, f32),
    /// Maps tree coordinates to picture coordinates, for SVGs whose size had
    /// to be worked out from the viewBox or the content.
    pub base: Transform,
}

#[derive(Debug)]
pub enum LoadError {
    NotFound,
    Unreadable,
    TooLarge,
    NotSvg,
    Broken { line: u32 },
    Empty,
    TooManyElements,
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::NotFound => write!(f, "The file was deleted or moved."),
            LoadError::Unreadable => write!(f, "This file can't be read."),
            LoadError::TooLarge => write!(f, "This file is too large to open."),
            LoadError::NotSvg => write!(f, "This file isn't an SVG."),
            LoadError::Broken { line } => write!(f, "This SVG has broken code near line {line}."),
            LoadError::Empty => write!(f, "This SVG has nothing to draw."),
            LoadError::TooManyElements => write!(f, "This SVG has too many elements to draw."),
        }
    }
}

/// Starts a thread that loads `source` and sends the result, tagged with
/// `id`, to the event loop.
pub fn spawn_load(id: u64, source: Source, proxy: EventLoopProxy<AppEvent>) {
    let spawned = std::thread::Builder::new()
        .name("loader".into())
        .spawn(move || {
            let result = load(&source);
            if let Err(e) = &result {
                let what = match &source {
                    Source::File(path) => path.display().to_string(),
                    Source::Text(_) => "pasted text".into(),
                };
                eprintln!("load failed: {what}: {e:?}");
            }
            // Fails only when the event loop has already exited.
            let _ = proxy.send_event(AppEvent::Loaded { id, result });
        });
    if let Err(e) = spawned {
        eprintln!("could not start loader thread: {e}");
    }
}

pub fn load(source: &Source) -> Result<Document, LoadError> {
    let start = Instant::now();
    let doc = match source {
        Source::File(path) => {
            let bytes = std::fs::read(path).map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => LoadError::NotFound,
                _ => {
                    eprintln!("read failed: {}: {e}", path.display());
                    LoadError::Unreadable
                }
            })?;
            let bytes = if is_gzip(&bytes) { gunzip(&bytes)? } else { bytes };
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            parse(&decode_text(&bytes), path.parent(), name, Some(path.clone()))?
        }
        Source::Text(text) => parse(text, None, "Pasted SVG".into(), None)?,
    };
    eprintln!("parsed {} in {:?}", doc.name, start.elapsed());
    Ok(doc)
}

fn parse(
    text: &str,
    resources_dir: Option<&Path>,
    name: String,
    path: Option<PathBuf>,
) -> Result<Document, LoadError> {
    let root = root_svg_tag(text).ok_or(LoadError::NotSvg)?;

    let mut options = usvg::Options {
        resources_dir: resources_dir.map(Path::to_path_buf),
        ..usvg::Options::default()
    };
    // System fonts are slow to load, so only load them when the SVG has text.
    if text.contains("<text") || text.contains(":text") {
        let font_start = Instant::now();
        let mut fontdb = usvg::fontdb::Database::new();
        fontdb.load_system_fonts();
        eprintln!("loaded {} fonts in {:?}", fontdb.len(), font_start.elapsed());
        options.fontdb = Arc::new(fontdb);
    }

    let tree = usvg::Tree::from_str(text, &options).map_err(|e| match e {
        usvg::Error::ParsingFailed(e) => LoadError::Broken { line: e.pos().row },
        usvg::Error::ElementsLimitReached => LoadError::TooManyElements,
        usvg::Error::InvalidSize => LoadError::Empty,
        usvg::Error::NotAnUtf8Str
        | usvg::Error::MalformedGZip
        | usvg::Error::SvgzFeatureNotEnabled => LoadError::NotSvg,
    })?;

    let (size, base) = picture_size(&tree, &root)?;
    Ok(Document { tree: Arc::new(tree), path, name, size, base })
}

/// Applies the spec's size rules on top of the size usvg worked out.
fn picture_size(tree: &usvg::Tree, root: &RootTag) -> Result<((f32, f32), Transform), LoadError> {
    let tree_size = (tree.size().width(), tree.size().height());
    let width_missing = is_missing_length(root.width.as_deref());
    let height_missing = is_missing_length(root.height.as_deref());
    let view_box = root.view_box.as_deref().and_then(parse_view_box);

    match view_box {
        // Missing or percentage sizes: use the viewBox size.
        Some((vw, vh)) if width_missing && height_missing => {
            let base = Transform::from_scale(vw / tree_size.0, vh / tree_size.1);
            Ok(((vw, vh), base))
        }
        Some(_) => Ok((tree_size, Transform::identity())),
        // No viewBox and no complete size: measure the drawing itself.
        None if width_missing || height_missing => {
            let bounds = tree.root().abs_stroke_bounding_box();
            if !(bounds.width() > 0.0 && bounds.height() > 0.0) {
                return Err(LoadError::Empty);
            }
            let base = Transform::from_translate(-bounds.x(), -bounds.y());
            Ok(((bounds.width(), bounds.height()), base))
        }
        None => Ok((tree_size, Transform::identity())),
    }
}

fn is_gzip(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x1f, 0x8b])
}

fn gunzip(bytes: &[u8]) -> Result<Vec<u8>, LoadError> {
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .take(MAX_UNZIPPED_BYTES + 1)
        .read_to_end(&mut out)
        .map_err(|e| {
            eprintln!("gunzip failed: {e}");
            LoadError::NotSvg
        })?;
    if out.len() as u64 > MAX_UNZIPPED_BYTES {
        return Err(LoadError::TooLarge);
    }
    Ok(out)
}

/// Decodes UTF-8 (with or without BOM) and UTF-16 (LE or BE, with or without
/// BOM). Anything that isn't valid UTF-8 is read as Latin-1, which keeps
/// older files with a few accented characters readable.
fn decode_text(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return decode_utf16(rest, u16::from_le_bytes);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return decode_utf16(rest, u16::from_be_bytes);
    }
    // UTF-16 without a BOM: the first character is `<`, which is ASCII.
    match bytes {
        [b'<', 0, ..] => return decode_utf16(bytes, u16::from_le_bytes),
        [0, b'<', ..] => return decode_utf16(bytes, u16::from_be_bytes),
        _ => {}
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
    }
}

fn decode_utf16(bytes: &[u8], from_bytes: fn([u8; 2]) -> u16) -> String {
    let (units, _) = bytes.as_chunks::<2>();
    char::decode_utf16(units.iter().map(|&pair| from_bytes(pair)))
        .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

/// The size attributes of the root `<svg>` element.
#[derive(Debug, Default, PartialEq)]
struct RootTag {
    width: Option<String>,
    height: Option<String>,
    view_box: Option<String>,
}

/// Finds the root element and returns its size attributes, or None if the
/// root element isn't `<svg>`. Skips the XML declaration, comments,
/// processing instructions and the DOCTYPE.
fn root_svg_tag(text: &str) -> Option<RootTag> {
    let mut rest = text;
    loop {
        rest = &rest[rest.find('<')?..];
        if let Some(after) = rest.strip_prefix("<!--") {
            rest = &after[after.find("-->")? + 3..];
        } else if let Some(after) = rest.strip_prefix("<?") {
            rest = &after[after.find("?>")? + 2..];
        } else if let Some(after) = rest.strip_prefix("<!") {
            // A DOCTYPE, possibly with an internal subset in brackets.
            let end = match (after.find('['), after.find('>')) {
                (Some(open), Some(close)) if open < close => {
                    let close_bracket = open + after[open..].find(']')?;
                    close_bracket + after[close_bracket..].find('>')?
                }
                (_, close) => close?,
            };
            rest = &after[end + 1..];
        } else {
            break;
        }
    }

    let tag = &rest[1..];
    let name_end = tag.find(|c: char| c.is_whitespace() || c == '>' || c == '/')?;
    let name = &tag[..name_end];
    let local_name = name.rsplit(':').next()?;
    if local_name != "svg" {
        return None;
    }

    let mut root = RootTag::default();
    let mut attrs = &tag[name_end..];
    loop {
        attrs = attrs.trim_start();
        if attrs.is_empty() || attrs.starts_with('>') || attrs.starts_with('/') {
            break;
        }
        let eq = attrs.find('=')?;
        let key = attrs[..eq].trim();
        let after_eq = attrs[eq + 1..].trim_start();
        let quote = after_eq.chars().next().filter(|&q| q == '"' || q == '\'')?;
        let value_end = after_eq[1..].find(quote)?;
        let value = after_eq[1..1 + value_end].trim().to_owned();
        match key {
            "width" => root.width = Some(value),
            "height" => root.height = Some(value),
            "viewBox" => root.view_box = Some(value),
            _ => {}
        }
        attrs = &after_eq[value_end + 2..];
    }
    Some(root)
}

/// True for a missing, empty, `auto` or percentage width or height.
fn is_missing_length(value: Option<&str>) -> bool {
    match value.map(str::trim) {
        None | Some("") | Some("auto") => true,
        Some(v) => v.ends_with('%'),
    }
}

/// Returns the width and height of a valid viewBox.
fn parse_view_box(value: &str) -> Option<(f32, f32)> {
    let numbers: Vec<f32> = value
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    match numbers[..] {
        [_, _, w, h] if w > 0.0 && h > 0.0 && w.is_finite() && h.is_finite() => Some((w, h)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    fn load_text(text: &str) -> Result<Document, LoadError> {
        parse(text, None, "test".into(), None)
    }

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    const SQUARE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="30"><rect width="40" height="30"/></svg>"#;

    #[test]
    fn gzip_is_detected_by_bytes() {
        assert!(is_gzip(&gzip(b"<svg/>")));
        assert!(!is_gzip(b"<svg/>"));
        assert!(!is_gzip(&[0x1f]));
        assert_eq!(gunzip(&gzip(SQUARE.as_bytes())).unwrap(), SQUARE.as_bytes());
        assert!(matches!(gunzip(&[0x1f, 0x8b, 0, 0]), Err(LoadError::NotSvg)));
    }

    #[test]
    fn decodes_utf8_with_and_without_bom() {
        assert_eq!(decode_text("<svg>é</svg>".as_bytes()), "<svg>é</svg>");
        let mut with_bom = vec![0xEF, 0xBB, 0xBF];
        with_bom.extend_from_slice("<svg>é</svg>".as_bytes());
        assert_eq!(decode_text(&with_bom), "<svg>é</svg>");
    }

    #[test]
    fn decodes_utf16() {
        let text = "<svg>é</svg>";
        let le: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let be: Vec<u8> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
        assert_eq!(decode_text(&[&[0xFF, 0xFE][..], &le].concat()), text);
        assert_eq!(decode_text(&[&[0xFE, 0xFF][..], &be].concat()), text);
        assert_eq!(decode_text(&le), text);
        assert_eq!(decode_text(&be), text);
    }

    #[test]
    fn invalid_utf8_reads_as_latin1() {
        assert_eq!(decode_text(b"<svg>caf\xe9</svg>"), "<svg>café</svg>");
    }

    #[test]
    fn finds_root_svg_tag() {
        let text = r#"<?xml version="1.0"?>
            <!-- <svg width="1"> in a comment -->
            <!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" "x.dtd" [ <!ENTITY a "b"> ]>
            <svg:svg xmlns:svg="http://www.w3.org/2000/svg" width='10mm'
                height = "50%" viewBox="0 0 10 20"><rect/></svg:svg>"#;
        let root = root_svg_tag(text).unwrap();
        assert_eq!(root.width.as_deref(), Some("10mm"));
        assert_eq!(root.height.as_deref(), Some("50%"));
        assert_eq!(root.view_box.as_deref(), Some("0 0 10 20"));
        assert_eq!(root_svg_tag("<svg/>"), Some(RootTag::default()));
    }

    #[test]
    fn rejects_non_svg_roots() {
        assert_eq!(root_svg_tag("<html><svg/></html>"), None);
        assert_eq!(root_svg_tag("just text"), None);
        assert_eq!(root_svg_tag("<svgfoo/>"), None);
        assert_eq!(root_svg_tag(""), None);
    }

    #[test]
    fn missing_lengths() {
        assert!(is_missing_length(None));
        assert!(is_missing_length(Some("")));
        assert!(is_missing_length(Some("100%")));
        assert!(is_missing_length(Some(" auto ")));
        assert!(!is_missing_length(Some("10")));
        assert!(!is_missing_length(Some("2in")));
    }

    #[test]
    fn view_box_parsing() {
        assert_eq!(parse_view_box("0 0 100 50"), Some((100.0, 50.0)));
        assert_eq!(parse_view_box("-5,-5, 10,20"), Some((10.0, 20.0)));
        assert_eq!(parse_view_box("0 0 0 50"), None);
        assert_eq!(parse_view_box("0 0 100"), None);
        assert_eq!(parse_view_box("a b c d"), None);
    }

    #[test]
    fn absolute_size() {
        let doc = load_text(SQUARE).unwrap();
        assert_eq!(doc.size, (40.0, 30.0));
        assert_eq!(doc.base, Transform::identity());
    }

    #[test]
    fn physical_units_convert_at_96_dpi() {
        let doc = load_text(r#"<svg xmlns="http://www.w3.org/2000/svg" width="1in" height="2.54cm"><rect width="5" height="5"/></svg>"#).unwrap();
        assert!(approx(doc.size.0, 96.0) && approx(doc.size.1, 96.0));
        let doc = load_text(r#"<svg xmlns="http://www.w3.org/2000/svg" width="72pt" height="6pc"><rect width="5" height="5"/></svg>"#).unwrap();
        assert!(approx(doc.size.0, 96.0) && approx(doc.size.1, 96.0));
    }

    #[test]
    fn missing_size_uses_view_box() {
        let doc = load_text(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 300 150"><rect width="300" height="150"/></svg>"#).unwrap();
        assert_eq!(doc.size, (300.0, 150.0));
    }

    #[test]
    fn percentage_size_counts_as_missing() {
        let doc = load_text(r#"<svg xmlns="http://www.w3.org/2000/svg" width="50%" height="25%" viewBox="0 0 300 150"><rect width="300" height="150"/></svg>"#).unwrap();
        assert_eq!(doc.size, (300.0, 150.0));
        // The base transform undoes the percentage scale usvg applied.
        let tree = doc.tree.size();
        assert!(approx(tree.width() * doc.base.sx, 300.0));
        assert!(approx(tree.height() * doc.base.sy, 150.0));
    }

    #[test]
    fn no_size_and_no_view_box_uses_content_bounds() {
        let doc = load_text(r#"<svg xmlns="http://www.w3.org/2000/svg"><rect x="500" y="200" width="80" height="40"/></svg>"#).unwrap();
        assert_eq!(doc.size, (80.0, 40.0));
        assert_eq!(doc.base, Transform::from_translate(-500.0, -200.0));
    }

    #[test]
    fn empty_svg_without_size_is_an_error() {
        let result = load_text(r#"<svg xmlns="http://www.w3.org/2000/svg"></svg>"#);
        assert!(matches!(result, Err(LoadError::Empty)));
    }

    #[test]
    fn errors_have_plain_messages() {
        assert!(matches!(load_text("hello"), Err(LoadError::NotSvg)));
        assert!(matches!(load_text("<html/>"), Err(LoadError::NotSvg)));
        let broken = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\">\n<rect/>\n<circle r=\"4\"\n</svg>";
        let err = load_text(broken).err().unwrap();
        assert!(matches!(err, LoadError::Broken { line: 4 }), "{err:?}");
        assert_eq!(err.to_string(), "This SVG has broken code near line 4.");
    }

    #[test]
    fn loads_svgz_and_utf16_files() {
        let dir = std::env::temp_dir().join(format!("peregrineview-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let svgz = dir.join("plain-name.svg");
        std::fs::write(&svgz, gzip(SQUARE.as_bytes())).unwrap();
        assert_eq!(load(&Source::File(svgz)).unwrap().size, (40.0, 30.0));

        let utf16 = dir.join("utf16.svg");
        let bytes: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain(SQUARE.encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        std::fs::write(&utf16, bytes).unwrap();
        assert_eq!(load(&Source::File(utf16)).unwrap().size, (40.0, 30.0));

        let missing = dir.join("missing.svg");
        assert!(matches!(load(&Source::File(missing)), Err(LoadError::NotFound)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
