#![deny(rust_2018_idioms, unused, unused_crate_dependencies, unused_import_braces, unused_lifetimes, unused_qualifications, warnings)]
#![forbid(unsafe_code)]

pub const ERRORS_DIR_LINUX: &str = "/home/fenhl/.local/share/syncbin";
pub const ERRORS_DIR_MACOS: &str = "/Users/fenhl/Desktop";

#[cfg(target_os = "linux")] pub const ERRORS_DIR: &str = ERRORS_DIR_LINUX;
#[cfg(target_os = "macos")] pub const ERRORS_DIR: &str = ERRORS_DIR_MACOS;
