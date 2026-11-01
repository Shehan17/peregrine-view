//! Files opened from Finder.
//!
//! A file double-clicked in Finder doesn't arrive in argv but as an Apple
//! Event, which NSApplication passes on to its delegate as
//! `application:openURLs:`. The delegate is winit's, which has no such method
//! in winit 0.30, so one is added to its class before the event loop runs.
//! This also catches files opened while the app is already running, which
//! macOS sends to the running app instead of starting a new one.

use std::cell::RefCell;
use std::path::PathBuf;

use objc2::MainThreadMarker;
use objc2::ffi::class_addMethod;
use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::sel;
use objc2_app_kit::NSApplication;
use objc2_foundation::{NSArray, NSURL};
use winit::event_loop::EventLoopProxy;

use crate::AppEvent;

thread_local! {
    /// Where opened files go. Cocoa calls the handler on the main thread.
    static PROXY: RefCell<Option<EventLoopProxy<AppEvent>>> = const { RefCell::new(None) };
}

type OpenUrls = unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, *const NSArray<NSURL>);

/// Sends files opened from Finder to the app as `AppEvent::OpenFile`.
/// Call on the main thread, after the event loop is created and before it runs.
pub fn install_open_handler(proxy: EventLoopProxy<AppEvent>) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    // winit sets its delegate when the event loop is created.
    let Some(delegate) = NSApplication::sharedApplication(mtm).delegate() else {
        eprintln!("Finder open handler: the app has no delegate");
        return;
    };
    let class: &AnyClass = AsRef::<AnyObject>::as_ref(&*delegate).class();
    PROXY.with(|p| *p.borrow_mut() = Some(proxy));

    let handler: OpenUrls = open_urls;
    // SAFETY: the handler matches the "v@:@@" type: no result, then the
    // receiver, the selector, the NSApplication and an NSArray of NSURL.
    let added = unsafe {
        let imp = std::mem::transmute::<OpenUrls, Imp>(handler);
        let class = class as *const AnyClass as *mut AnyClass;
        class_addMethod(class, sel!(application:openURLs:), imp, c"v@:@@".as_ptr())
    };
    if !added.as_bool() {
        eprintln!("Finder open handler: the app delegate already handles opened files");
    }
}

unsafe extern "C-unwind" fn open_urls(
    _this: *mut AnyObject,
    _cmd: Sel,
    _app: *mut AnyObject,
    urls: *const NSArray<NSURL>,
) {
    // SAFETY: Cocoa passes an array of URLs, or nil.
    let Some(urls) = (unsafe { urls.as_ref() }) else { return };
    // There is one window, so only the first file opens.
    let path: Option<PathBuf> = urls.iter().find_map(|url| url.to_file_path());
    if let Some(path) = path {
        PROXY.with(|p| {
            if let Some(proxy) = &*p.borrow() {
                let _ = proxy.send_event(AppEvent::OpenFile(path));
            }
        });
    }
}
