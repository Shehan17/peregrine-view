# Releasing PeregrineView

How to put out a new version. For now only Windows is packaged; GitHub still tests macOS and Linux on every push.

## What happens on a release

Pushing a tag like `v0.2.0` starts the **Release** workflow (`.github/workflows/release.yml`) on GitHub. It:

1. checks that the tag matches `version` in `Cargo.toml`
2. builds the release exe and checks it is under 5 MB
3. builds the installer with cargo-packager
4. zips the exe as the portable version
5. uploads both to a **draft** GitHub Release

Nothing is public until you publish the draft by hand.

The files it makes:

| File | What it is |
| --- | --- |
| `peregrineview_<version>_x64-setup.exe` | Installer. Installs for the current user (no admin prompt), registers `.svg` and `.svgz`, and adds PeregrineView to "Open with". |
| `peregrineview_<version>_x64-portable.zip` | Just `PeregrineView.exe`. Runs without installing and changes nothing on the PC. |

## Steps

### 1. Check that main is ready

- All changes are committed and pushed, and the latest **CI** run on GitHub is green.
- `cargo test` and `cargo clippy --all-targets` pass locally.

### 2. Pick the new version

Use `MAJOR.MINOR.PATCH`:

- **Patch** (`0.1.0` → `0.1.1`): bug fixes only.
- **Minor** (`0.1.0` → `0.2.0`): new features.
- **Major** (`0.x` → `1.0.0`): when you call it finished and stable.

### 3. Change the version

Edit `version` in `Cargo.toml`, then build once so `Cargo.lock` is updated too:

```sh
cargo build --release
```

Commit **both** files. The release build runs with `--locked`, so it fails if `Cargo.lock` still has the old version.

```sh
git add Cargo.toml Cargo.lock
git commit -m "Version 0.2.0"
git push
```

### 4. (Optional) Test the installer locally

```sh
cargo packager --release
```

This writes the installer to `dist/`. Install it, open an `.svg` with it, then uninstall it from Windows Settings → Apps. The uninstaller removes everything it added. The first run needs `cargo install cargo-packager --version 0.11.8 --locked`.

### 5. Tag and push

```sh
git tag v0.2.0
git push origin v0.2.0
```

The tag must be `v` + the exact version in `Cargo.toml`.

### 6. Wait for the build

Watch it on GitHub under **Actions**, or:

```sh
gh run list --limit 3
gh run watch <run id>
```

It takes about 7 minutes.

### 7. Check and publish the draft

1. Open https://github.com/Shehan17/peregrine-view/releases. The new draft is at the top.
2. Read the notes (GitHub fills them from the commits since the last release) and edit them if you like.
3. Download the installer and try it once.
4. Click **Publish release**.

## If something goes wrong

**The tag doesn't match the version.** The first step fails with "Tag … doesn't match version …". Delete the tag, fix the version, and tag again:

```sh
git tag -d v0.2.0
git push origin :refs/tags/v0.2.0
```

**The build failed for another reason.** Open the failed run under **Actions** to see which step failed. If it was a one-off GitHub problem, click **Re-run jobs**. If the code needs a fix: delete the tag (as above), delete the draft release if one was made, fix and push, then tag again.

**A published release has a bad file.** Don't reuse the version number. Fix the problem and release the next patch version, such as `0.2.1`.

## Good to know

- **Windows warning on download.** The installer and exe aren't code-signed, so Windows SmartScreen shows "Windows protected your PC". Users click **More info → Run anyway**. Signing needs a paid certificate and can come later.
- **Publisher name.** The installer shows "Shehan" in the Apps list and keeps its install folder in the registry under `Software\Shehan\PeregrineView`. Changing `publisher` in `Cargo.toml` later works, but leaves that old registry key behind on PCs that had an earlier version installed.
- **Installer template.** `assets/installer.nsi` is a fixed copy of cargo-packager 0.11.8's template; the changes are listed at its top. If you update cargo-packager in `release.yml`, compare the new version's template and carry the changes over.
- **App icon.** After changing `assets/App_Icon.png`, run `powershell -File assets/icons/make-ico.ps1` to rebuild the Windows icon, and commit `assets/icons/icon.ico`.
- **Before the first macOS release.** Replace the placeholder `identifier` in `Cargo.toml` with the final one. macOS treats an app whose identifier changes as a different app.
