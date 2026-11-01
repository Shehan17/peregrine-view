//! Watches the open file for changes, so it can reload automatically.

use std::ffi::OsString;
use std::path::Path;

use notify::event::{EventKind, ModifyKind};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use winit::event_loop::EventLoopProxy;

use crate::AppEvent;

/// Watches one file until dropped. It watches the file's folder rather than
/// the file itself, because many editors save by writing a new file and
/// renaming it over the old one.
pub struct FileWatcher {
    _watcher: RecommendedWatcher,
}

pub fn watch(path: &Path, proxy: EventLoopProxy<AppEvent>) -> Option<FileWatcher> {
    let name = path.file_name()?.to_owned();
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty())?;
    let changed_path = path.to_path_buf();
    let handler = move |result: notify::Result<notify::Event>| match result {
        Ok(event) if is_change(&event.kind) && event.paths.iter().any(|p| same_name(p, &name)) => {
            let _ = proxy.send_event(AppEvent::FileChanged(changed_path.clone()));
        }
        Ok(_) => {}
        Err(e) => eprintln!("file watcher: {e}"),
    };
    let mut watcher = match notify::recommended_watcher(handler) {
        Ok(watcher) => watcher,
        Err(e) => {
            eprintln!("could not start file watcher: {e}");
            return None;
        }
    };
    if let Err(e) = watcher.watch(dir, RecursiveMode::NonRecursive) {
        eprintln!("could not watch {}: {e}", dir.display());
        return None;
    }
    Some(FileWatcher { _watcher: watcher })
}

/// True for events that can change what the file shows. Reads are ignored,
/// so loading the file never triggers another reload.
fn is_change(kind: &EventKind) -> bool {
    match kind {
        EventKind::Create(_) | EventKind::Remove(_) | EventKind::Any => true,
        EventKind::Modify(ModifyKind::Metadata(_)) => false,
        EventKind::Modify(_) => true,
        EventKind::Access(access) => {
            matches!(access, notify::event::AccessKind::Close(notify::event::AccessMode::Write))
        }
        EventKind::Other => false,
    }
}

fn same_name(path: &Path, name: &OsString) -> bool {
    match path.file_name() {
        Some(n) if n == name => true,
        // Windows and macOS file names usually ignore case.
        Some(n) if cfg!(not(target_os = "linux")) => {
            n.to_string_lossy().to_lowercase() == name.to_string_lossy().to_lowercase()
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{AccessKind, AccessMode, CreateKind, DataChange, MetadataKind};

    #[test]
    fn only_writes_count_as_changes() {
        assert!(is_change(&EventKind::Modify(ModifyKind::Data(DataChange::Content))));
        assert!(is_change(&EventKind::Create(CreateKind::File)));
        assert!(is_change(&EventKind::Access(AccessKind::Close(AccessMode::Write))));
        assert!(!is_change(&EventKind::Access(AccessKind::Read)));
        assert!(!is_change(&EventKind::Access(AccessKind::Close(AccessMode::Read))));
        assert!(!is_change(&EventKind::Modify(ModifyKind::Metadata(MetadataKind::AccessTime))));
    }

    #[test]
    fn matches_the_file_name() {
        let name = OsString::from("icon.svg");
        assert!(same_name(Path::new("/a/icon.svg"), &name));
        assert!(!same_name(Path::new("/a/icon.svg.tmp"), &name));
        assert_eq!(same_name(Path::new("/a/ICON.svg"), &name), cfg!(not(target_os = "linux")));
    }
}
