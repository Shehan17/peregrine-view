# Contributing to PeregrineView

Thanks for wanting to help! Bug reports, test files, fixes and features are all welcome. This page explains what the project is aiming for and how to send a change.

## Ways to help

- **Report a bug.** Open an issue and attach the SVG that shows it, if you can share it. A file that opens wrong is the most useful bug report there is.
- **Send a hard SVG.** Real-world files that break viewers (odd sizes, encodings, fonts, huge maps) make good additions to `test-svgs/`.
- **Fix an issue.** Issues labeled `good first issue` are a good place to start, and `help wanted` marks bigger ones.
- **Port something to macOS or Linux.** Only Windows is packaged so far, and the "Open in" editor button is Windows only.

If you plan a bigger change, open an issue first so we can agree on the idea before you spend time on it.

## What PeregrineView is for

PeregrineView is an SVG **viewer** that opens files fast and stays smooth. Every change is checked against these targets:

| Target | Value |
| --- | --- |
| Startup to first window | under 100 ms |
| Simple SVG open to first picture | under 50 ms |
| Moving and zooming | 60 fps, no stutter |
| Release exe size | under 5 MB |
| Memory with a normal SVG open | under 50 MB |
| CPU when idle | 0% (nothing redraws when nothing changes) |
| Huge files | never freeze the window; heavy work runs in the background |

To keep it that way, these are **out of scope**: editing, animation, scripts, tabs or multiple windows, a settings window, and a GPU renderer. Pull requests that add them will be closed, even if they look small.

Ideas that do fit, if you want something to build:

- Export or copy the picture as PNG at a chosen size
- A thumbnail grid of the SVGs in a folder
- Show the SVG source code
- A color picker for the pixel under the cursor
- Compare two SVGs side by side

## Building and testing

You need [Rust](https://rustup.rs) (stable).

```sh
cargo build                                   # debug build; prints timings to the terminal
cargo run -- test-svgs/icon.svg               # open a file
cargo test                                    # all unit tests
cargo clippy --all-targets                    # must stay warning-free
cargo run --release --example large_svg       # makes a 6 MB test map for speed checks
```

Debug builds print `window created …` and `first picture … after start` times, and the time of each render, so you can check a change against the targets above. Check speed in a release build (`cargo run --release`).

If you change code in `src/platform/`, check that it still compiles for the other systems:

```sh
rustup target add aarch64-apple-darwin x86_64-unknown-linux-gnu   # once
cargo clippy --target aarch64-apple-darwin --all-targets
cargo clippy --target x86_64-unknown-linux-gnu --all-targets
```

[CLAUDE.md](CLAUDE.md) has an overview of how the code is put together: the threads, how rendering works, and where each feature lives.

## Rules for the code

- **Don't run `cargo fmt`.** The code isn't formatted with rustfmt (it uses longer lines and one-line `let ... else { return };`), and running it would rewrite every file. Match the style of the code around your change.
- **No new crates without a clear reason.** Each one adds size and build time. Say why in the pull request, and check that `Cargo.lock` doesn't gain a pile of new packages.
- **Never crash on a bad file.** Don't use `unwrap()` or `expect()` on anything that depends on the user's files or the system. Log the details with `eprintln!` and show the user a short message in the banner.
- **Write messages in plain words** that say what went wrong, such as "This file isn't an SVG." or "This SVG has broken code near line 12."
- **Keep the main thread free.** Loading, rendering and anything else slow runs on a background thread and reports back through an event.
- **Comments say why**, in short plain sentences.
- **Don't change the bytes of files in `test-svgs/` or `assets/`.** Some are UTF-16, have a BOM or are gzipped on purpose, so make sure your editor doesn't re-save them.

## Compatibility

PeregrineView should open almost any real-world SVG. When a file doesn't work, these are the rules it follows (in `src/loader.rs`):

- `.svgz` is detected by its gzip bytes, not just the file extension.
- UTF-8 (with or without a BOM) and UTF-16 files both work.
- A missing width or height comes from the `viewBox`. With no size and no `viewBox`, the drawing's own bounds are used.
- Sizes in `mm`, `cm`, `in`, `pt` and `pc` are converted at 96 DPI. Percentage sizes count as missing.
- Linked images are found relative to the SVG's folder.
- Scripts and animation are ignored, and the rest of the picture is drawn.

When you fix a file that didn't open, add a small copy of the problem to `test-svgs/` if you can.

## Sending a pull request

1. Fork the repo and make a branch for your change.
2. Keep the pull request to one fix or feature.
3. Run `cargo test` and `cargo clippy --all-targets` before you push. GitHub runs both on Windows, macOS and Linux for every pull request.
4. Write commit messages in plain words that say what changed, such as "Fix zoom jumping on trackpads".
5. In the pull request, say what you changed and how you tested it. For anything visible, a screenshot helps.

## License

PeregrineView is released into the public domain under the [Unlicense](LICENSE). By sending a contribution, you agree to release it into the public domain under the same terms.
