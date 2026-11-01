#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod icon;
mod loader;
mod nav;
mod render;
mod settings;
mod text;
mod ui;
mod view;
mod watch;

use std::path::PathBuf;
use std::time::Instant;

use winit::event_loop::{ControlFlow, EventLoop};

/// Messages from background threads to the event loop.
pub enum AppEvent {
    /// A load finished. `path` is the file it tried, when there was one.
    Loaded { id: u64, path: Option<PathBuf>, result: Result<loader::Document, loader::LoadError> },
    /// Next or Previous picked this file; its load is still running.
    Resolved { id: u64, path: PathBuf },
    /// The app icon is ready to put on the window.
    Icons(icon::IconImages),
    /// The open file changed on disk.
    FileChanged(PathBuf),
    Rendered(render::Frame),
    /// The Open dialog closed, with the chosen file if there is one.
    DialogClosed(Option<PathBuf>),
}

fn main() {
    let start = Instant::now();
    let path = std::env::args_os().nth(1).map(PathBuf::from);

    let event_loop = match EventLoop::<AppEvent>::with_user_event().build() {
        Ok(event_loop) => event_loop,
        Err(e) => {
            eprintln!("could not create event loop: {e}");
            std::process::exit(1);
        }
    };
    event_loop.set_control_flow(ControlFlow::Wait);

    let renderer = match render::Renderer::spawn(event_loop.create_proxy()) {
        Ok(renderer) => renderer,
        Err(e) => {
            eprintln!("could not start render thread: {e}");
            std::process::exit(1);
        }
    };

    icon::spawn_load(event_loop.create_proxy());

    // The app starts parsing the file right away, while the window is being created.
    let mut app = app::App::new(start, renderer, event_loop.create_proxy(), path);
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("event loop error: {e}");
        std::process::exit(1);
    }
}
