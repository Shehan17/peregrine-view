//! Moving a file to the Recycle Bin (Windows) or the Trash (macOS, Linux),
//! so a deleted file can still be restored.
//!
//! Windows uses `SHFileOperationW`, which also warns when a file can't be
//! recycled and would be deleted for good, as on some network drives. macOS
//! uses `NSFileManager`. Linux runs `gio trash`, which comes with GLib on
//! almost every desktop, so this needs no extra crate.

use std::path::{Path, PathBuf};

use winit::event_loop::EventLoopProxy;

use crate::AppEvent;

/// What the trash is called on this system, for messages.
pub const TRASH_NAME: &str = if cfg!(windows) { "Recycle Bin" } else { "Trash" };

/// Why a file wasn't moved to the trash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrashError {
    /// The user said no when warned the file would be deleted for good.
    Cancelled,
    Failed,
}

/// Moves `path` to the trash on its own thread, and sends how it went as
/// `AppEvent::Trashed`. `parent` is the window any warning shows over (Windows only).
pub fn spawn(path: PathBuf, parent: isize, proxy: EventLoopProxy<AppEvent>) -> std::io::Result<()> {
    std::thread::Builder::new().name("trash".into()).spawn(move || {
        let result = move_to_trash(&path, parent);
        if let Err(e) = result {
            eprintln!("could not move {} to the {TRASH_NAME}: {e:?}", path.display());
        }
        let _ = proxy.send_event(AppEvent::Trashed { path, result });
    })?;
    Ok(())
}

#[cfg(windows)]
fn move_to_trash(path: &Path, parent: isize) -> Result<(), TrashError> {
    use std::os::windows::ffi::OsStrExt;
    use win::*;

    // A list of paths, each ending in a NUL, with one more NUL at the end.
    let from: Vec<u16> = path.as_os_str().encode_wide().chain([0, 0]).collect();
    let mut op = FileOp {
        hwnd: parent,
        func: FO_DELETE,
        from: from.as_ptr(),
        to: std::ptr::null(),
        flags: FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_WANTNUKEWARNING | FOF_SILENT | FOF_NOERRORUI,
        aborted: 0,
        name_mappings: std::ptr::null_mut(),
        progress_title: std::ptr::null(),
    };
    // SAFETY: shell calls want COM on their thread. `op` and the
    // double-NUL-terminated `from` outlive the call. Uninitialize only after
    // a successful initialize.
    let result = unsafe {
        let com = CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED);
        let result = SHFileOperationW(&mut op);
        if com >= 0 {
            CoUninitialize();
        }
        result
    };
    if op.aborted != 0 {
        Err(TrashError::Cancelled)
    } else if result != 0 {
        eprintln!("SHFileOperationW failed with code {result:#x}");
        Err(TrashError::Failed)
    } else {
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn move_to_trash(path: &Path, _parent: isize) -> Result<(), TrashError> {
    use objc2_foundation::{NSFileManager, NSURL};

    let url = NSURL::from_file_path(path).ok_or(TrashError::Failed)?;
    NSFileManager::defaultManager().trashItemAtURL_resultingItemURL_error(&url, None).map_err(|e| {
        eprintln!("trashItemAtURL failed: {}", e.localizedDescription());
        TrashError::Failed
    })
}

#[cfg(not(any(windows, target_os = "macos")))]
fn move_to_trash(path: &Path, _parent: isize) -> Result<(), TrashError> {
    use std::process::{Command, Stdio};

    let status = Command::new("gio")
        .args(["trash", "--"])
        .arg(path)
        .stdin(Stdio::null())
        .status()
        .map_err(|e| {
            eprintln!("could not run gio: {e}");
            TrashError::Failed
        })?;
    if status.success() { Ok(()) } else { Err(TrashError::Failed) }
}

// The Windows declarations used above, from shellapi.h and objbase.h.
#[cfg(windows)]
mod win {
    use std::ffi::c_void;

    pub const FO_DELETE: u32 = 0x3;
    pub const FOF_SILENT: u16 = 0x4;
    pub const FOF_NOCONFIRMATION: u16 = 0x10;
    /// Recycle instead of deleting for good.
    pub const FOF_ALLOWUNDO: u16 = 0x40;
    pub const FOF_NOERRORUI: u16 = 0x400;
    /// Ask first when the file can't be recycled, despite FOF_NOCONFIRMATION.
    pub const FOF_WANTNUKEWARNING: u16 = 0x4000;
    pub const COINIT_APARTMENTTHREADED: u32 = 0x2;

    /// SHFILEOPSTRUCTW, which shellapi.h packs to 1 byte on 32-bit x86 only.
    #[cfg_attr(target_arch = "x86", repr(C, packed(1)))]
    #[cfg_attr(not(target_arch = "x86"), repr(C))]
    pub struct FileOp {
        pub hwnd: isize,
        pub func: u32,
        pub from: *const u16,
        pub to: *const u16,
        pub flags: u16,
        pub aborted: i32,
        pub name_mappings: *mut c_void,
        pub progress_title: *const u16,
    }

    #[link(name = "shell32")]
    unsafe extern "system" {
        pub fn SHFileOperationW(op: *mut FileOp) -> i32;
    }

    #[link(name = "ole32")]
    unsafe extern "system" {
        pub fn CoInitializeEx(reserved: *const c_void, flags: u32) -> i32;
        pub fn CoUninitialize();
    }
}
