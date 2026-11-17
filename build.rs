//! Builds the Windows resources into the exe: the app icon, which Explorer
//! shows for the exe, the file icon (resource 2), which the installer points
//! the .svg/.svgz file types at, and the version info, whose description is
//! the name Task Manager and "Open with" show. Other systems get their icon
//! from the package instead.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/icons/icon.ico");
    println!("cargo:rerun-if-changed=assets/icons/file.ico");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        windows_resources();
    }
}

/// Writes the resource script to the build folder, with the version from
/// Cargo.toml, and compiles it into the exe.
#[cfg(windows)]
fn windows_resources() {
    use std::path::PathBuf;

    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let icon_path = |name: &str| manifest_dir.join("assets").join("icons").join(name).to_string_lossy().replace('\\', "\\\\");
    let icon = icon_path("icon.ico");
    let file_icon = icon_path("file.ico");

    let version = env!("CARGO_PKG_VERSION");
    let numbers: Vec<u16> = version.split(['.', '-', '+']).take(3).map(|part| part.parse().unwrap_or(0)).collect();
    let (major, minor, patch) = (numbers[0], numbers.get(1).copied().unwrap_or(0), numbers.get(2).copied().unwrap_or(0));

    let script = format!(
        r#"#pragma code_page(65001)
1 ICON "{icon}"
2 ICON "{file_icon}"
1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "FileDescription", "PeregrineView"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "peregrineview"
      VALUE "OriginalFilename", "peregrineview.exe"
      VALUE "ProductName", "PeregrineView"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    );
    let rc = out_dir.join("peregrineview.rc");
    std::fs::write(&rc, script).unwrap();
    embed_resource::compile(&rc, embed_resource::NONE).manifest_optional().unwrap();
}
