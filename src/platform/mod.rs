//! Host information, privilege detection, output decoding, elevation.
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;
