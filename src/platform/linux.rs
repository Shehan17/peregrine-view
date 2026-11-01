//! The system light or dark preference on Linux.
//!
//! winit reports no theme on X11, and on Wayland only one the app set itself,
//! so the desktop is asked through the XDG Settings portal, which GNOME, KDE
//! and most other desktops provide. `dbus-send` comes with every D-Bus
//! install, so this needs no D-Bus crate.

// Built for tests on every system, but only used on Linux.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::process::{Command, Stdio};

#[cfg(target_os = "linux")]
use winit::event_loop::EventLoopProxy;
use winit::window::Theme;

#[cfg(target_os = "linux")]
use crate::AppEvent;

/// Asks for the preference on a background thread and sends it as
/// `AppEvent::SystemTheme`. Sends nothing when the desktop doesn't say.
#[cfg(target_os = "linux")]
pub fn spawn_theme_query(proxy: EventLoopProxy<AppEvent>) {
    let spawned = std::thread::Builder::new().name("theme".into()).spawn(move || {
        if let Some(theme) = query_theme() {
            let _ = proxy.send_event(AppEvent::SystemTheme(theme));
        }
    });
    if let Err(e) = spawned {
        eprintln!("could not start theme thread: {e}");
    }
}

fn query_theme() -> Option<Theme> {
    let output = Command::new("dbus-send")
        .args([
            "--reply-timeout=200",
            "--print-reply=literal",
            "--dest=org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.Settings.Read",
            "string:org.freedesktop.appearance",
            "string:color-scheme",
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| eprintln!("could not ask the desktop for its theme: {e}"))
        .ok()?;
    parse_color_scheme(&String::from_utf8_lossy(&output.stdout))
}

/// Reads a reply like `variant       variant          uint32 1`, where
/// 1 means dark, 2 light and 0 no preference.
fn parse_color_scheme(reply: &str) -> Option<Theme> {
    let mut words = reply.split_whitespace().rev();
    let value = words.next()?;
    if words.next()? != "uint32" {
        return None;
    }
    match value {
        "1" => Some(Theme::Dark),
        "0" | "2" => Some(Theme::Light),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_scheme_replies() {
        let read = |value| format!("   variant       variant          uint32 {value}\n");
        assert_eq!(parse_color_scheme(&read(1)), Some(Theme::Dark));
        assert_eq!(parse_color_scheme(&read(2)), Some(Theme::Light));
        assert_eq!(parse_color_scheme(&read(0)), Some(Theme::Light));
        assert_eq!(parse_color_scheme(&read(7)), None);
        // The newer ReadOne call drops one `variant`.
        assert_eq!(parse_color_scheme("   variant       uint32 1\n"), Some(Theme::Dark));
        // No portal, or an error reply.
        assert_eq!(parse_color_scheme(""), None);
        assert_eq!(parse_color_scheme("Error org.freedesktop.DBus.Error.UnknownMethod"), None);
    }
}
