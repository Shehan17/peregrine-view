//! The app icon for the window's title bar and taskbar button. It is decoded
//! and scaled down on a background thread so it never slows down startup.

use resvg::tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};
use winit::event_loop::EventLoopProxy;

use crate::AppEvent;

static APP_ICON: &[u8] = include_bytes!("../assets/App_Icon.png");

/// Title bar icon size, in pixels.
const SMALL: u32 = 32;
/// Taskbar and Alt+Tab icon size, in pixels.
const LARGE: u32 = 64;

/// A square icon as straight (not premultiplied) RGBA bytes.
pub struct IconImage {
    pub rgba: Vec<u8>,
    pub size: u32,
}

pub struct IconImages {
    pub small: IconImage,
    pub large: IconImage,
}

impl IconImage {
    pub fn to_icon(&self) -> Option<winit::window::Icon> {
        winit::window::Icon::from_rgba(self.rgba.clone(), self.size, self.size)
            .map_err(|e| eprintln!("bad app icon: {e}"))
            .ok()
    }
}

pub fn spawn_load(proxy: EventLoopProxy<AppEvent>) {
    let spawned = std::thread::Builder::new().name("icon".into()).spawn(move || {
        if let Some(icons) = load() {
            let _ = proxy.send_event(AppEvent::Icons(icons));
        }
    });
    if let Err(e) = spawned {
        eprintln!("could not start icon thread: {e}");
    }
}

fn load() -> Option<IconImages> {
    let source = Pixmap::decode_png(APP_ICON)
        .map_err(|e| eprintln!("could not read the app icon: {e}"))
        .ok()?;
    Some(IconImages { small: scaled(&source, SMALL)?, large: scaled(&source, LARGE)? })
}

/// Scales `source` down to `size` × `size`, halving at most each step so
/// every source pixel counts and the small icon stays smooth.
fn scaled(source: &Pixmap, size: u32) -> Option<IconImage> {
    let mut current = source.clone();
    while current.width() > size || current.height() > size {
        let w = (current.width() / 2).max(size);
        let h = (current.height() / 2).max(size);
        let mut next = Pixmap::new(w, h)?;
        let paint = PixmapPaint { quality: FilterQuality::Bilinear, ..PixmapPaint::default() };
        let sx = w as f32 / current.width() as f32;
        let sy = h as f32 / current.height() as f32;
        next.draw_pixmap(0, 0, current.as_ref(), &paint, Transform::from_scale(sx, sy), None);
        current = next;
    }
    if current.width() != size || current.height() != size {
        return None;
    }
    let rgba = current
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    Some(IconImage { rgba, size })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_decodes_and_scales() {
        let icons = load().unwrap();
        assert_eq!(icons.small.size, SMALL);
        assert_eq!(icons.small.rgba.len(), (SMALL * SMALL * 4) as usize);
        assert_eq!(icons.large.rgba.len(), (LARGE * LARGE * 4) as usize);
        // The icon is opaque, and its top left corner is the bright green.
        assert_eq!(icons.small.rgba[3], 255);
        assert!(icons.small.rgba[1] > 200 && icons.small.rgba[0] < 100);
    }
}
