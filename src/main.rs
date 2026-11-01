#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod loader;
mod render;
mod view;

use std::path::PathBuf;
use std::time::Instant;

use winit::event_loop::{ControlFlow, EventLoop};

/// Messages from background threads to the event loop.
pub enum AppEvent {
    Loaded(Result<loader::Document, loader::LoadError>),
    Rendered(render::Frame),
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

    // Parse while the window is being created.
    if let Some(path) = path {
        loader::spawn_load(path, event_loop.create_proxy());
    }

    let renderer = match render::Renderer::spawn(event_loop.create_proxy()) {
        Ok(renderer) => renderer,
        Err(e) => {
            eprintln!("could not start render thread: {e}");
            std::process::exit(1);
        }
    };

    let mut app = app::App::new(start, renderer);
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("event loop error: {e}");
        std::process::exit(1);
    }
}
