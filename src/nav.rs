//! Lists the SVGs in a folder, for Next and Previous.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

/// The SVG files in `dir`, sorted by name.
pub fn list_svgs(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_file() || t.is_symlink()))
        .map(|entry| entry.path())
        .filter(|path| is_svg_name(path))
        .collect();
    files.sort_by(|a, b| compare_names(a, b));
    Ok(files)
}

fn is_svg_name(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("svg") || e.eq_ignore_ascii_case("svgz"))
}

fn name_of(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Compares file names the way people sort them: ignoring case, and with
/// numbers in order by value, so `icon2` comes before `icon10`.
pub fn compare_names(a: &Path, b: &Path) -> Ordering {
    let (a, b) = (name_of(a), name_of(b));
    natural_cmp(&a, &b).then_with(|| a.cmp(&b))
}

fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take_number = |chars: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut digits = String::new();
                    while let Some(c) = chars.peek().copied().filter(char::is_ascii_digit) {
                        digits.push(c);
                        chars.next();
                    }
                    digits
                };
                let (da, db) = (take_number(&mut a), take_number(&mut b));
                let (ta, tb) = (da.trim_start_matches('0'), db.trim_start_matches('0'));
                let order = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                let order = x.to_lowercase().cmp(y.to_lowercase());
                if order != Ordering::Equal {
                    return order;
                }
                a.next();
                b.next();
            }
        }
    }
}

/// The file after (or before) `current` in the sorted `files`, wrapping
/// around at the ends. Works even if `current` is no longer in the list.
pub fn neighbor<'a>(files: &'a [PathBuf], current: &Path, forward: bool) -> Option<&'a PathBuf> {
    if files.is_empty() {
        return None;
    }
    let n = files.len();
    let index = match files.iter().position(|f| f.file_name() == current.file_name()) {
        Some(i) => {
            if forward { (i + 1) % n } else { (i + n - 1) % n }
        }
        // The current file is gone: step to where it would have been.
        None => {
            let insert = files.partition_point(|f| compare_names(f, current) == Ordering::Less);
            if forward { insert % n } else { (insert + n - 1) % n }
        }
    };
    files.get(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(|n| PathBuf::from("dir").join(n)).collect()
    }

    fn names(files: &[PathBuf]) -> Vec<String> {
        files.iter().map(|f| name_of(f)).collect()
    }

    #[test]
    fn natural_order() {
        let mut files = paths(&["icon10.svg", "Icon2.svg", "b.svg", "icon1.svg", "A.svg", "icon02.svg"]);
        files.sort_by(|a, b| compare_names(a, b));
        // "Icon2" and "icon02" tie by value, then sort by their exact names.
        assert_eq!(names(&files), ["A.svg", "b.svg", "icon1.svg", "Icon2.svg", "icon02.svg", "icon10.svg"]);
    }

    #[test]
    fn next_and_previous_wrap_around() {
        let files = paths(&["a.svg", "b.svg", "c.svg"]);
        let at = |n: &str| PathBuf::from("dir").join(n);
        assert_eq!(neighbor(&files, &at("a.svg"), true), Some(&at("b.svg")));
        assert_eq!(neighbor(&files, &at("c.svg"), true), Some(&at("a.svg")));
        assert_eq!(neighbor(&files, &at("a.svg"), false), Some(&at("c.svg")));
        assert_eq!(neighbor(&files, &at("b.svg"), false), Some(&at("a.svg")));
    }

    #[test]
    fn deleted_current_file_steps_from_its_place() {
        let files = paths(&["a.svg", "c.svg", "e.svg"]);
        let gone = PathBuf::from("dir").join("d.svg");
        assert_eq!(neighbor(&files, &gone, true).map(|p| name_of(p)), Some("e.svg".into()));
        assert_eq!(neighbor(&files, &gone, false).map(|p| name_of(p)), Some("c.svg".into()));
        let last = PathBuf::from("dir").join("z.svg");
        assert_eq!(neighbor(&files, &last, true).map(|p| name_of(p)), Some("a.svg".into()));
    }

    #[test]
    fn empty_and_single() {
        assert_eq!(neighbor(&[], Path::new("a.svg"), true), None);
        let one = paths(&["a.svg"]);
        assert_eq!(neighbor(&one, &one[0], true), Some(&one[0]));
    }

    #[test]
    fn lists_only_svg_files_in_order() {
        let dir = std::env::temp_dir().join(format!("peregrineview-nav-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub.svg")).unwrap();
        for name in ["b.SVG", "a.svgz", "c.png", "notes.txt", "a10.svg", "a9.svg"] {
            std::fs::write(dir.join(name), "x").unwrap();
        }
        let files = list_svgs(&dir).unwrap();
        assert_eq!(names(&files), ["a.svgz", "a9.svg", "a10.svg", "b.SVG"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
