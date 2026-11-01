//! Opening the file in another app, such as Inkscape or Illustrator.
//!
//! The apps come from the shell's own list for the file type
//! (`SHAssocEnumHandlers`), the same list as its "Open with" chooser, and are
//! started through the shell, which works for Store apps too. "Other app…"
//! shows the chooser itself. There is no COM crate in the project, so the few
//! COM calls needed are declared here by hand.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::{self, NonNull};
use std::sync::mpsc::{self, Receiver, Sender};

use winit::event_loop::EventLoopProxy;

use super::Editor;
use crate::AppEvent;

/// Why a file couldn't be opened in an app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenError {
    /// The app is no longer in the shell's list, usually because it was uninstalled.
    NotInstalled,
    /// The shell failed with this HRESULT.
    Failed(i32),
}

enum Request {
    List { ext: String },
    Open { editor: Editor, path: PathBuf },
}

/// A thread that talks to the shell, so the window never waits on it.
pub struct Shell {
    requests: Sender<Request>,
}

impl Shell {
    pub fn spawn(proxy: EventLoopProxy<AppEvent>) -> std::io::Result<Shell> {
        let (requests, rx) = mpsc::channel();
        std::thread::Builder::new().name("shell".into()).spawn(move || worker(rx, proxy))?;
        Ok(Shell { requests })
    }

    /// Lists the apps for files ending in `ext` (".svg"), as `AppEvent::Editors`.
    pub fn list(&self, ext: String) {
        self.send(Request::List { ext });
    }

    /// Opens `path` in `editor`, and reports how it went as `AppEvent::EditorOpened`.
    pub fn open(&self, editor: Editor, path: PathBuf) {
        self.send(Request::Open { editor, path });
    }

    fn send(&self, request: Request) {
        if self.requests.send(request).is_err() {
            eprintln!("shell thread has stopped");
        }
    }
}

fn worker(rx: Receiver<Request>, proxy: EventLoopProxy<AppEvent>) {
    // SAFETY: called once on this thread, before any other COM call on it.
    let com = unsafe { CoInitializeEx(ptr::null(), COINIT_APARTMENTTHREADED) };
    while let Ok(request) = rx.recv() {
        let event = match request {
            Request::List { ext } => {
                let list = list_editors(&ext);
                AppEvent::Editors { ext, list }
            }
            Request::Open { editor, path } => {
                let result = open_in(&editor.id, &path);
                if let Err(e) = result {
                    eprintln!("could not open {} in {}: {e:?}", path.display(), editor.id);
                }
                AppEvent::EditorOpened { editor, result }
            }
        };
        if proxy.send_event(event).is_err() {
            break; // The event loop has exited.
        }
    }
    if com >= 0 {
        // SAFETY: balances the successful CoInitializeEx above, on the same thread.
        unsafe { CoUninitialize() };
    }
}

/// The apps the shell recommends for files ending in `ext`, without this app.
fn list_editors(ext: &str) -> Vec<Editor> {
    let own = std::env::current_exe().ok().map(|exe| exe.to_string_lossy().to_lowercase());
    let mut editors: Vec<Editor> = Vec::new();
    for (editor, _) in handlers(ext) {
        let is_own = own.as_deref() == Some(editor.id.to_lowercase().as_str());
        if !is_own && !editors.iter().any(|e| e.id == editor.id) {
            editors.push(editor);
        }
    }
    editors
}

/// Opens `path` with the app whose id is `id`.
fn open_in(id: &str, path: &Path) -> Result<(), OpenError> {
    let ext = path.extension().map_or(".svg".to_owned(), |ext| format!(".{}", ext.to_string_lossy()));
    let (_, handler) = handlers(&ext).into_iter().find(|(editor, _)| editor.id == id).ok_or(OpenError::NotInstalled)?;
    let wide_path = wide(path.as_os_str());
    // SAFETY: the out-pointers are valid, each interface pointer is used with
    // the vtable of its own interface, and `Com` releases every one exactly once.
    unsafe {
        let mut item: *mut Object<ShellItemVtbl> = ptr::null_mut();
        check(SHCreateItemFromParsingName(wide_path.as_ptr(), ptr::null_mut(), &IID_ISHELLITEM, (&raw mut item).cast()))?;
        let item = Com::new(item).ok_or(OpenError::Failed(E_POINTER))?;
        let mut data: *mut Object<UnknownVtbl> = ptr::null_mut();
        let bind = item.vtbl().bind_to_handler;
        check(bind(item.ptr(), ptr::null_mut(), &BHID_DATA_OBJECT, &IID_IDATA_OBJECT, (&raw mut data).cast()))?;
        let data = Com::new(data).ok_or(OpenError::Failed(E_POINTER))?;
        check((handler.vtbl().invoke)(handler.ptr(), data.ptr().cast()))
    }
}

/// Every recommended handler for `ext`, with its id and name.
fn handlers(ext: &str) -> Vec<(Editor, Com<HandlerVtbl>)> {
    let mut found = Vec::new();
    let ext = wide(ext.as_ref());
    // SAFETY: as in `open_in`. Strings from GetName and GetUIName are read up
    // to their NUL and then freed with CoTaskMemFree, as the API requires.
    unsafe {
        let mut list: *mut Object<EnumVtbl> = ptr::null_mut();
        let result = SHAssocEnumHandlers(ext.as_ptr(), ASSOC_FILTER_RECOMMENDED, &mut list);
        if result < 0 {
            eprintln!("could not list apps: HRESULT {:#010x}", result as u32);
            return found;
        }
        let Some(list) = Com::new(list) else { return found };
        loop {
            let mut handler: *mut Object<HandlerVtbl> = ptr::null_mut();
            let mut fetched = 0;
            if (list.vtbl().next)(list.ptr(), 1, &mut handler, &mut fetched) != S_OK || fetched != 1 {
                break;
            }
            let Some(handler) = Com::new(handler) else { break };
            let id = take_string(handler.ptr(), handler.vtbl().get_name);
            let name = take_string(handler.ptr(), handler.vtbl().get_ui_name);
            if let (Some(id), Some(name)) = (id, name) {
                found.push((Editor { id, name }, handler));
            }
        }
    }
    found
}

/// Calls a getter that returns a CoTaskMemAlloc'd string, and frees it.
unsafe fn take_string(handler: *mut Object<HandlerVtbl>, getter: GetString) -> Option<String> {
    let mut raw: *mut u16 = ptr::null_mut();
    // SAFETY: the caller passes a live handler and one of its own getters.
    unsafe {
        if getter(handler, &mut raw) < 0 || raw.is_null() {
            return None;
        }
        let len = (0..).take_while(|&i| *raw.add(i) != 0).count();
        let text = String::from_utf16_lossy(std::slice::from_raw_parts(raw, len));
        CoTaskMemFree(raw.cast());
        (!text.is_empty()).then_some(text)
    }
}

fn check(result: i32) -> Result<(), OpenError> {
    if result < 0 { Err(OpenError::Failed(result)) } else { Ok(()) }
}

fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
    s.encode_wide().chain([0]).collect()
}

/// Shows Windows' own "How do you want to open this file?" chooser for
/// `path` over the window `hwnd`, and sends `AppEvent::OpenWithClosed` when
/// it closes. The chooser blocks its thread until then, so it gets its own.
pub fn spawn_open_with(hwnd: isize, path: PathBuf, proxy: EventLoopProxy<AppEvent>) -> std::io::Result<()> {
    std::thread::Builder::new().name("open-with".into()).spawn(move || {
        let ok = open_with(hwnd, &path);
        let _ = proxy.send_event(AppEvent::OpenWithClosed(ok));
    })?;
    Ok(())
}

/// Returns false when the chooser failed, but not when the user cancelled it.
fn open_with(hwnd: isize, path: &Path) -> bool {
    let file = wide(path.as_os_str());
    let info = OpenAsInfo {
        file: file.as_ptr(),
        class: ptr::null(),
        flags: OAIF_EXEC | OAIF_HIDE_REGISTRATION,
    };
    // SAFETY: shell UI needs COM on this thread. `info` and the NUL-terminated
    // `file` outlive the call. Uninitialize only after a successful initialize.
    let result = unsafe {
        let com = CoInitializeEx(ptr::null(), COINIT_APARTMENTTHREADED);
        let result = SHOpenWithDialog(hwnd, &info);
        if com >= 0 {
            CoUninitialize();
        }
        result
    };
    eprintln!("open with: chooser closed, HRESULT {:#010x}", result as u32);
    result >= 0 || result == CANCELLED
}

// The Windows declarations used above, from shobjidl_core.h and objidl.h.

#[repr(C)]
struct Guid(u32, u16, u16, [u8; 8]);

const IID_ISHELLITEM: Guid = Guid(0x43826d1e, 0xe718, 0x42ee, [0xbc, 0x55, 0xa1, 0xe2, 0x61, 0xc3, 0x7b, 0xfe]);
const IID_IDATA_OBJECT: Guid = Guid(0x0000010e, 0, 0, [0xc0, 0, 0, 0, 0, 0, 0, 0x46]);
const BHID_DATA_OBJECT: Guid = Guid(0xb8c0bd9f, 0xed24, 0x455c, [0x83, 0xe6, 0xd5, 0x39, 0x0c, 0x4f, 0xe8, 0xc4]);

const S_OK: i32 = 0;
const E_POINTER: i32 = 0x8000_4003_u32 as i32;
/// HRESULT_FROM_WIN32(ERROR_CANCELLED): the user closed the chooser.
const CANCELLED: i32 = 0x8007_04C7_u32 as i32;
const COINIT_APARTMENTTHREADED: u32 = 0x2;
const ASSOC_FILTER_RECOMMENDED: i32 = 0x1;
/// Start the chosen app.
const OAIF_EXEC: u32 = 0x04;
/// Hide "Always use this app", so the chooser can't replace the default viewer.
const OAIF_HIDE_REGISTRATION: u32 = 0x20;

/// OPENASINFO
#[repr(C)]
struct OpenAsInfo {
    file: *const u16,
    class: *const u16,
    flags: u32,
}

/// A COM object: a pointer to its vtable comes first.
#[repr(C)]
struct Object<V> {
    vtbl: *const V,
}

/// Every vtable starts with IUnknown's three methods. Slots this file never
/// calls are kept as plain pointers so the ones after them line up.
#[repr(C)]
struct UnknownVtbl {
    query_interface: usize,
    add_ref: usize,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
}

#[repr(C)]
struct EnumVtbl {
    unknown: UnknownVtbl,
    next: unsafe extern "system" fn(*mut Object<EnumVtbl>, u32, *mut *mut Object<HandlerVtbl>, *mut u32) -> i32,
}

type GetString = unsafe extern "system" fn(*mut Object<HandlerVtbl>, *mut *mut u16) -> i32;

#[repr(C)]
struct HandlerVtbl {
    unknown: UnknownVtbl,
    get_name: GetString,
    get_ui_name: GetString,
    get_icon_location: usize,
    is_recommended: usize,
    make_default: usize,
    invoke: unsafe extern "system" fn(*mut Object<HandlerVtbl>, *mut c_void) -> i32,
}

#[repr(C)]
struct ShellItemVtbl {
    unknown: UnknownVtbl,
    bind_to_handler:
        unsafe extern "system" fn(*mut Object<ShellItemVtbl>, *mut c_void, *const Guid, *const Guid, *mut *mut c_void) -> i32,
}

/// An owned interface pointer, released when dropped.
struct Com<V>(NonNull<Object<V>>);

impl<V> Com<V> {
    /// # Safety
    /// `ptr` is null or an interface pointer of the type `V` describes, whose
    /// reference the new `Com` takes over.
    unsafe fn new(ptr: *mut Object<V>) -> Option<Com<V>> {
        NonNull::new(ptr).map(Com)
    }

    fn ptr(&self) -> *mut Object<V> {
        self.0.as_ptr()
    }

    fn vtbl(&self) -> &V {
        // SAFETY: a live COM object's vtable pointer is valid while it lives.
        unsafe { &*(*self.0.as_ptr()).vtbl }
    }
}

impl<V> Drop for Com<V> {
    fn drop(&mut self) {
        // SAFETY: every vtable starts with IUnknown's, and this `Com` owns one reference.
        unsafe {
            let unknown = self.0.as_ptr().cast::<Object<UnknownVtbl>>();
            ((*(*unknown).vtbl).release)(unknown.cast());
        }
    }
}

#[link(name = "shell32")]
unsafe extern "system" {
    fn SHOpenWithDialog(parent: isize, info: *const OpenAsInfo) -> i32;
    fn SHAssocEnumHandlers(extension: *const u16, filter: i32, list: *mut *mut Object<EnumVtbl>) -> i32;
    fn SHCreateItemFromParsingName(path: *const u16, bind: *mut c_void, iid: *const Guid, out: *mut *mut c_void) -> i32;
}

#[link(name = "ole32")]
unsafe extern "system" {
    fn CoInitializeEx(reserved: *const c_void, flags: u32) -> i32;
    fn CoUninitialize();
    fn CoTaskMemFree(ptr: *mut c_void);
}
