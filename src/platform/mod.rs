//! Code for one operating system only.

#[cfg(any(target_os = "linux", test))]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
