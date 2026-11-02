//! Code for one operating system only.

#[cfg(any(target_os = "linux", test))]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
pub mod trash;
#[cfg(windows)]
pub mod windows;

/// An app that can open the current file, such as Inkscape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Editor {
    /// What the system knows the app by: its program path, or a Store app's ID.
    pub id: String,
    /// The name to show, such as "Inkscape".
    pub name: String,
}
