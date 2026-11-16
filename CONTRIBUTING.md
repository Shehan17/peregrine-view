# Contributing to PeregrineView

Thanks for helping! Bug reports, hard SVG files, fixes and features are all welcome. For a bigger change, open an issue first so we can agree on the idea.

## What fits

PeregrineView is a fast SVG **viewer**. Changes should keep it that way: the window opens in under 100 ms, zooming stays at 60 fps, and the exe stays under 5 MB.

Editing, animation, scripts, tabs, multiple windows, a settings window and a GPU renderer are out of scope.

## Build and test

```sh
cargo run -- test-svgs/icon.svg   # open a file
cargo test
cargo clippy --all-targets        # must have no warnings
```

## Rules

- Don't run `cargo fmt`. Match the style of the code around your change.
- Don't add a new crate without a good reason.
- Never crash on a bad file: no `unwrap()` on user files. Show a short, plain message instead.
- Don't re-save files in `test-svgs/`. Some have odd encodings on purpose.

## Sending a change

1. Fork the repo on GitHub, then clone your fork:
   ```sh
   git clone https://github.com/<your-name>/peregrine-view.git
   cd peregrine-view
   ```
2. Make a branch for your change:
   ```sh
   git checkout -b fix-zoom-jump
   ```
3. Make your change, run the tests, then commit and push:
   ```sh
   git add .
   git commit -m "Fix zoom jumping on trackpads"
   git push origin fix-zoom-jump
   ```
4. On GitHub, open a pull request from your branch to `main`. Say what you changed and how you tested it.

Keep each pull request to one change. GitHub runs the tests on Windows, macOS and Linux.

By contributing, you agree to release your work into the public domain under the [Unlicense](LICENSE).
