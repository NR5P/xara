//! Ctrl-C and friends: the first stops xara cleanly (`stop` is set), a second ends it at once.

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::trap;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::trap;

// The signals xara stops on (Windows: console Ctrl-C, Ctrl-Break, close, logoff and shutdown).
#[cfg(unix)]
pub const STOP: [libc::c_int; 4] = [libc::SIGINT, libc::SIGTERM, libc::SIGHUP, libc::SIGQUIT];
#[cfg(windows)]
pub const STOP: [libc::c_int; 2] = [libc::SIGINT, libc::SIGTERM];
