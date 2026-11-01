# PeregrineView — SVG Viewer Spec

Nov 1, 2026 · @Shehan

## Overview

PeregrineView is a fast, lightweight SVG viewer for Windows, macOS and Linux, written in Rust. It does one job: open any SVG quickly and let the user look at it smoothly, at any zoom level.

The name comes from the peregrine falcon, the fastest animal, with very sharp eyes. That matches the two main goals: speed and sharp detail.

PeregrineView is a viewer only. It does not edit SVGs, run scripts, or play animations. Keeping the scope small is what keeps it fast and small.

Project name: PeregrineView. Binary and crate name: `peregrineview`.

## Goals and targets

The app must feel instant. These numbers define what "fast" and "lightweight" mean, so every change can be checked against them.

| Target | Value |
| --- | --- |
| Startup to first window | under 100 ms |
| Simple SVG open to first picture | under 50 ms |
| Frame rate while moving or zooming | 60 fps, no stutter |
| Release binary size | under 5 MB |
| Idle memory with a normal SVG open | under 50 MB |
| Idle CPU use | 0% (no redraw when nothing changes) |
| UI freeze on huge files | never; heavy work runs on a background thread |

The other goals are compatibility (open almost every real-world SVG) and ease of use (no settings needed, it just works).

## Platforms and tech stack

PeregrineView targets Windows 10/11, macOS (Intel and Apple Silicon) and Linux (X11 and Wayland). It uses no web engine and no heavy UI framework.

| Crate | Role |
| --- | --- |
| `usvg` | Parse the SVG into a clean, simplified tree |
| `resvg` | Render the tree into pixels |
| `tiny-skia` | 2D drawing library (used by resvg, also used to draw the toolbar) |
| `fontdb` | Find and load system fonts for SVG text |
| `winit` | Window, mouse, keyboard, trackpad and drag-and-drop events |
| `softbuffer` | Show the CPU pixel buffer in the window |
| `rfd` | Native Open file dialog |
| `notify` | Watch the open file for changes (auto reload) |
| `flate2` | Unzip `.svgz` files |
| `objc2` (macOS only) | Receive files opened from Finder |

Use the latest stable versions of each crate. Keep the dependency list short; any new crate needs a clear reason.

Release profile: `opt-level = 3`, `lto = true`, `codegen-units = 1`, `strip = true`, `panic = "abort"`.

## Behavior

### Opening files

A file can be opened in five ways:

- Double-click a `.svg` or `.svgz` file in Explorer, Finder or a Linux file manager
- Command line: `peregrineview path/to/file.svg`
- Drag and drop a file onto the window
- Ctrl+O (Cmd+O on macOS) or the Open button, using the native file dialog
- Paste a file or SVG text from the clipboard (Ctrl/Cmd+V)

On Windows and Linux, a double-clicked file arrives as a command line argument. On macOS it arrives through an Apple Event, not argv, so a small macOS-only handler is needed. With no file, the app shows an empty screen with an Open button and a hint to drop a file.

### On open

Every newly opened file is shown fit to the window, centered, with a small margin. The window title becomes `<file name> – PeregrineView`. If the window is resized while the view is still in fit mode, it refits. After the user zooms or moves, resizing keeps the current view.

### Navigation

- Mouse wheel: zoom in or out at the cursor point (the point under the cursor stays still)
- Trackpad two-finger scroll: move the canvas
- Trackpad pinch: zoom at the pinch center
- Ctrl + scroll (any device): zoom
- Left drag (or middle drag): move the canvas
- Double-click: zoom in 2x at that point; Shift + double-click zooms out

Tell mouse and trackpad apart with winit's scroll delta: `LineDelta` means a mouse wheel, `PixelDelta` means a trackpad. Zoom limits: from 1/16 of fit size up to 512x, but never larger than about 64,000 px on the longest side.

### Rendering pipeline

This is the key to feeling fast while staying sharp.

1. Parse the file once into a `usvg` tree on a background thread.
2. While the user moves or zooms, redraw the last rendered bitmap with a simple transform. This is very cheap and must hit 60 fps.
3. When input stops for about 100 ms, render only the visible part of the SVG at the real zoom level, at physical screen pixels, on a background thread.
4. When that render is done, swap it in. If the view changed in the meantime, drop the old result and start again.

Always render at physical pixels (use the window scale factor), so HiDPI and Retina screens stay sharp. Draw nothing when nothing changes, so idle CPU stays at 0%.

### Fonts

Load system fonts only when the SVG contains text, and do it on the background thread. This keeps startup fast for icon files.

## Version 1 features and UI

### Features

- [ ] Open from double-click, command line, drag and drop, Open dialog and clipboard paste
- [ ] Fit to window on open
- [ ] Smooth zoom and move with mouse, trackpad and keyboard
- [ ] Sharp redraw after the user stops moving
- [ ] Next and previous SVG in the same folder (sorted by name, wraps around)
- [ ] Background switch: checkerboard, white, dark (remembered between runs)
- [ ] Info bar: file name, width × height, file size
- [ ] Auto reload when the file changes on disk, keeping the current zoom and position
- [ ] `.svgz` support
- [ ] Light and dark UI that follows the system theme

### UI layout

The SVG fills the whole window. The UI floats on top and stays small.

- Top left: info bar with file name and size details
- Bottom center: toolbar with Open, Previous, Next, Zoom out, zoom %, Zoom in, Fit, Background
- Clicking the zoom % switches between 100% and fit
- Error messages appear as a short banner at the top for about 6 seconds

The toolbar is drawn with tiny-skia. Icons are simple vector shapes drawn in code, so no image files are needed. A window with no file shows a centered drop area with an Open button.

## Keyboard shortcuts

On macOS, Cmd replaces Ctrl in every shortcut.

| Key | Action |
| --- | --- |
| Ctrl+O | Open file |
| Ctrl+V | Paste SVG file or SVG text |
| `+` or `=` | Zoom in at window center |
| `-` | Zoom out at window center |
| `0` or `F` | Fit to window |
| `1` | Switch between 100% and fit |
| Arrow keys | Move the view by 60 px |
| Page Down / Space | Next SVG in folder |
| Page Up / Backspace | Previous SVG in folder |
| `B` | Change background |
| `R` | Reload file |
| F11 | Full screen on or off |
| Esc | Leave full screen |
| Ctrl+W or Ctrl+Q | Close the app |

Left and Right arrows move the view, so next/previous file uses Page Down and Page Up. This avoids a conflict when the user is zoomed in.

## Architecture

The main thread only handles input and shows pixels, so it never blocks. Loading and rendering run on background threads.

&#91;embedded content: Peregrine threads and data flow\]

While the user moves, the main thread shows a fast preview of the last bitmap. After 100 ms of no input, it asks the renderer for a sharp image of the visible area. Threads talk with `std::sync::mpsc` channels, and background threads wake the event loop with winit's `EventLoopProxy`.

| Module | Job |
| --- | --- |
| `main.rs` | Read arguments, create the window, start the event loop |
| `app.rs` | App state and event handling |
| `view.rs` | Zoom and pan math, fit, zoom limits (pure code, unit tested) |
| `loader.rs` | Read file, gzip and encoding detection, usvg parse, size rules |
| `render.rs` | Render worker thread, job queue, cancel old jobs |
| `ui.rs` | Toolbar, info bar, banner, empty screen |
| `text.rs` | UI text with one small embedded font (`ab_glyph`) |
| `nav.rs` | List SVGs in the folder for next and previous |
| `watch.rs` | File watcher with `notify` |
| `settings.rs` | Save the background choice in the user config folder |
| `platform/macos.rs` | Finder open-file handler |

## Errors and compatibility

The app must never crash on a bad file. Every error becomes a short, clear message in the banner, and the previous picture stays on screen.

### Compatibility rules

- Detect `.svgz` by its gzip bytes (`1F 8B`), not only by the file extension
- Read UTF-8 (with or without BOM) and UTF-16 files
- Missing width or height: take the size from the `viewBox`
- No size and no `viewBox`: measure the content bounding box and use that
- Sizes in `mm`, `cm`, `in`, `pt`, `pc`: convert to pixels at 96 DPI
- Percentage sizes: treat as missing
- Linked images with relative paths: resolve them from the SVG file's folder (set `resources_dir` in usvg options)
- Unsupported features (scripts, animation, SMIL): ignore them and draw the rest

### Error messages

Write messages in plain words that say what went wrong:

- "This file isn't an SVG."
- "This SVG has broken code near line 12."
- "The file was deleted or moved."
- "This SVG is too large to draw at this zoom."

Never use `unwrap()` on anything that depends on user files. Log details to stderr for debugging.

## Out of scope for v1

Version 1 does not include editing, animation, script support, tabs or multiple windows, a settings window, or a GPU renderer. Do not add these, even if they look easy.

Ideas for later versions:

- Export or copy as PNG at a chosen size
- Folder thumbnail grid
- Show the SVG source code
- Color picker for a pixel under the cursor
- Compare two SVGs side by side

## Milestones

Build in this order. Each milestone must compile, run, and be tested on at least one OS before the next one starts.

1. **Window and first picture.** Open a file from the command line, parse with usvg, render with resvg, show it fit to the window with softbuffer. Handle resize and HiDPI.
2. **Navigation.** Wheel zoom at cursor, drag to move, trackpad scroll and pinch, keyboard zoom, fit and 100%, zoom limits.
3. **Fast and sharp rendering.** Background render thread, cheap transform preview while moving, sharp redraw of the visible area after 100 ms idle, cancel old renders.
4. **Opening and compatibility.** Drag and drop, Open dialog, clipboard paste, `.svgz`, UTF-16, missing size rules, relative image paths, error banner.
5. **UI.** Toolbar, info bar, empty screen, background switch, light and dark theme, UI text with an embedded font.
6. **Viewer features.** Next and previous in folder, auto reload, full screen, remembered background.
7. **Platform polish.** macOS Finder open handler, Cmd shortcuts, Wayland and X11 checks, file associations.
8. **Release.** Size and speed checks against the targets, packaging, GitHub Actions builds for all three systems.

## Testing and packaging

### Testing

Keep a `test-svgs/` folder with real files that cover the hard cases: icons, a large map or diagram (5 MB or more), text-heavy SVGs, files without width, height or viewBox, `.svgz`, UTF-16, files with linked images, and broken files. Unit test the pure logic: size parsing, zoom math, file navigation order, and gzip detection. Check each milestone against the targets in Goals and targets.

### Packaging

| System | Output | File association |
| --- | --- | --- |
| Windows | Portable `.exe` and an installer | Installer registers `.svg` and `.svgz` |
| macOS | `.app` bundle in a `.dmg` | `Info.plist` declares SVG document types |
| Linux | AppImage and `.deb` | `.desktop` file with `image/svg+xml` MIME type |

Use `cargo-packager` (or `cargo-bundle`) to make the packages. GitHub Actions builds all three on every release tag. Without an Apple Developer account, macOS users will see a security warning on first open; signing can come later.
