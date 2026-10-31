pub mod bus;
pub mod json;
pub mod messages;
pub mod server;
pub mod settings;
pub mod telemetry;
pub(crate) mod waterfall;
pub mod ws;

/// A new file (it must not exist), with Unix permission bits `mode` where there are any.
pub(crate) fn new_file(path: &std::path::Path, mode: u32) -> std::io::Result<std::fs::File> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, mode);
    #[cfg(not(unix))]
    let _ = mode;
    o.open(path)
}
