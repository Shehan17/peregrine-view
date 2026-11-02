# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

PeregrineView is a fast, lightweight SVG viewer in Rust (winit + softbuffer + resvg, CPU rendering, no GPU or web engine). `SPEC.md` is the source of truth: performance targets, behavior, shortcuts, error wording, the allowed crate list, milestones, and what is out of scope (editing, animation, scripts, tabs, multiple windows, settings window, GPU renderer). Work is done milestone by milestone, in the order SPEC.md lists, and each commit is titled `Milestone N: <name>`.

## Commands

```sh
cargo build                                   # debug build; logs to stderr
cargo build --release                         # release profile: lto, strip, panic=abort
cargo run -- test-svgs/icon.svg               # open a file (also: .svgz, drag/drop, Ctrl+O, Ctrl+V)
cargo test                                    # all unit tests
cargo test natural_order                      # one test, by name
cargo test platform::linux                    # one module's tests
cargo clippy --all-targets                    # must stay warning-free
cargo run --release --example large_svg       # writes test-svgs/large-map.svg (6 MB, git-ignored) for speed checks
cargo packager --release                      # Windows installer in dist/ (once: cargo install cargo-packager --locked)
powershell -File assets/icons/make-ico.ps1    # rebuild assets/icons/icon.ico after changing assets/App_Icon.png
```

The platform code is checked from any OS by cross-target type-checking (`rustup target add aarch64-apple-darwin x86_64-unknown-linux-gnu` once):

```sh
cargo clippy --target aarch64-apple-darwin --all-targets
cargo clippy --target x86_64-unknown-linux-gnu --all-targets
```

Do not run `cargo fmt`: the code is not rustfmt-formatted (it uses longer lines and one-line `let ... else { return };`), and running it would rewrite every file.

Release builds on Windows use `windows_subsystem = "windows"`, so stderr is only visible in debug builds. The debug log prints `window created …` and `first picture … after start` timings, plus per-render and per-present times. Use these to check against the SPEC targets (first window under 100 ms, first picture under 50 ms in release). To see the log of a release build, read stderr through a pipe (e.g. .NET `Process` with `RedirectStandardError`): redirecting it to a file costs about 3 ms per line on Windows and makes startup look much slower than it is. Most of the time to first window is Windows creating the window; the app reaches `create_window` in a few milliseconds.

## Architecture

**Threads.** The main thread (`app.rs`, the winit `ApplicationHandler`) only handles input and presents pixels. Everything slow runs on short-lived or worker threads, and they report back as `AppEvent` variants (defined in `main.rs`) through an `EventLoopProxy`. These threads are: loader (one per open), render worker (one, long-lived), Open dialog, file watcher, icon decode, and the Linux theme query. The event loop uses `ControlFlow::Wait`/`WaitUntil`, so nothing redraws when nothing changes; `about_to_wait` schedules the timers (`settle_at`, banner expiry, `reload_at`).

**Stale-result counters.** Several monotonically increasing ids in `App` decide whether a result from a thread is still wanted:
- `load_id`: a `Loaded`/`Resolved` event with an older id is dropped (a newer open won).
- `doc_id`: identifies the open document; frames of another document aren't shown, except the pre-reload document while a reload is in progress (`reload_of`).
- `generation`: render jobs. The worker also drains its queue and renders only the newest job.

**Preview vs sharp rendering.** A `Target` is (doc, view, window size, backdrop). While the user zooms or pans, `redraw` scales and shifts the last `Frame` with `render::blit_preview` (cheap, nearest-neighbor). `view_changed` sets `settle_at` 100 ms ahead; when it fires, `request_sharp` submits a `Job` unless a frame for the same `Target` is shown or already in flight. A finished frame is shown only if its `Target` still matches the current one. The UI (`ui.rs`, tiny-skia shapes plus `text.rs` with the embedded Noto Sans subset) is drawn over the picture on every present, not by the render thread.

**Coordinates.** All view math works in physical pixels. `View { scale, tx, ty }` maps picture CSS pixels to window pixels, and 100% zoom means `scale == window.scale_factor()`. `view.rs` is pure and unit-tested (fit, zoom at anchor, limits). `Document.base` maps usvg tree coordinates to picture coordinates, for SVGs whose size the loader had to derive from the viewBox or the content bounds; the render transform is `view.transform().pre_concat(base)`.

**Opening.** Every way of opening a file goes through `App::open(Source)`. A `Source` is `File`, pasted `Text`, or `Neighbor` (Next/Previous: the loader lists the folder via `nav.rs`, reports the chosen file early with `Resolved`, then parses). `loader.rs` owns all compatibility rules: gzip detection by magic bytes, UTF-8/UTF-16 decoding, size rules, `resources_dir`, and loading system fonts only when the SVG has text. Its `LoadError` `Display` strings are the user-facing banner messages, in SPEC wording. A failed load keeps the previous picture on screen.

**Reload.** `watch.rs` watches the file's folder, not the file, so editors that save by renaming still trigger it. Changes are debounced by `RELOAD_DELAY`, and a reload keeps the view (`load_is_reload`).

**Platform code.** Platform code lives in `src/platform/`. `macos.rs` adds `application:openURLs:` to winit's NSApplication delegate class at runtime, because Finder sends files as Apple Events, not in argv. `windows.rs` lists the apps for a file type and opens the file in one (`SHAssocEnumHandlers` / `IAssocHandler::Invoke`, COM vtables declared by hand since there is no COM crate) on a long-lived STA `shell` thread, and shows Windows' own chooser (`SHOpenWithDialog`) for "Other app…". This drives the top-right "Open in" button and its menu, which `ui.rs` draws on every OS; `App::editable_path` keeps it hidden off Windows. The last app picked is saved as `editor=`/`editor_name=` in the settings file. `linux.rs` reads the light/dark preference through `dbus-send` (winit reports no theme on Linux) and is also compiled for tests on every OS. Other per-OS bits are inline `#[cfg]` in `app.rs`: the Windows taskbar icon, the Linux Wayland app ID / X11 WM_CLASS `peregrineview`, and the Cmd-vs-Ctrl primary modifier. Known winit 0.30 gaps: no drag and drop on Wayland, no pinch gesture on Linux.

**Packaging and release.** Only Windows is packaged for now; `RELEASE.md` has the steps for putting out a new version. `[package.metadata.packager]` in `Cargo.toml` holds the cargo-packager settings: an NSIS installer for the current user (no admin prompt) that registers `.svg`/`.svgz` from the `file-associations` entries. It uses `assets/installer.nsi`, a copy of cargo-packager 0.11.8's template whose changes are listed at its top and marked `PeregrineView` (the stock uninstaller removes no file associations). Keep it in step with the cargo-packager version pinned in `release.yml`, and in a handlebars template write `\\{{` after a backslash, since `\{{` prints the braces literally. The `identifier` is a placeholder and must be final before the first macOS release, since macOS treats a changed identifier as a different app. `build.rs` embeds `assets/icons/icon.ico` and version info (the "PeregrineView" name that Task Manager and "Open with" show) into the Windows exe through the build-only `embed-resource` crate. The window icon at runtime still comes from `App_Icon.png` (`icon.rs`). `.github/workflows/ci.yml` runs tests and clippy on Windows, macOS and Linux on every push. `release.yml` runs on a `v*` tag that must match the Cargo.toml version, checks the 5 MB size target, and uploads the installer and a portable zip to a draft GitHub Release. macOS-only dependencies deliberately reuse the objc2 0.6 / objc2-foundation 0.3 / objc2-app-kit 0.3 versions that rfd, arboard and softbuffer already pull in.

## Conventions

- New crates need a clear reason (SPEC rule). Check that `Cargo.lock` gains no new packages, and keep the release binary under 5 MB.
- Never `unwrap()`/`expect()` on anything that depends on user files or the OS. Log details with `eprintln!` and show users a short plain-language banner via `show_error`.
- Comments are short, plain sentences explaining why; doc comments on most fields and functions. User-facing text and commit messages use plain words, not jargon.
- `test-svgs/` and `assets/` are marked `-text` in `.gitattributes` because their exact bytes matter (UTF-16, BOMs, gzip). Never re-encode or normalize them. Add new hard cases there.
