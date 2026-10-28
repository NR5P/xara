//! C interface for embedding xara; see include/xara.h. On Android, JNI entry points too.

#[cfg(target_os = "android")]
mod jni;

use std::collections::VecDeque;
use std::ffi::{CStr, c_char};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Once, PoisonError};
use std::thread::{self, JoinHandle};

const LOG_LINES: usize = 1000;
const RX_PIECES: usize = 2000;
/// Received text is queued in pieces of at most this many bytes, so an app's buffer fits one.
pub const RX_PIECE: usize = 1024;

static RUNNING: AtomicBool = AtomicBool::new(false);
static LOG: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
static RX: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
static CAPTURE: Once = Once::new();

pub struct XaraModem {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<u8>>,
}

fn push(q: &Mutex<VecDeque<String>>, limit: usize, item: String) {
    let mut q = q.lock().unwrap_or_else(PoisonError::into_inner);
    if q.len() == limit {
        q.pop_front();
    }
    q.push_back(item);
}

fn keep(line: String) {
    push(&LOG, LOG_LINES, line);
}

fn received(mut s: &str) {
    while !s.is_empty() {
        let mut n = s.len().min(RX_PIECE);
        while !s.is_char_boundary(n) {
            n -= 1;
        }
        push(&RX, RX_PIECES, s[..n].to_string());
        s = &s[n..];
    }
}

/// Takes xara's log lines for the app, still copying them to stderr for a debugger's console.
fn capture_log() {
    CAPTURE.call_once(|| {
        xara_host::log::set_sink(|s| {
            eprintln!("{s}");
            s.lines().for_each(|l| keep(l.to_string()));
        });
        xara::rxtext::set_sink(received);
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn xara_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr().cast()
}

/// # Safety
/// `argv` must point to `argc` valid NUL-terminated strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xara_start(argv: *const *const c_char, argc: usize) -> *mut XaraModem {
    let args: Vec<String> = (0..argc)
        // SAFETY: guaranteed by the caller.
        .map(|i| unsafe { CStr::from_ptr(*argv.add(i)) }.to_string_lossy().into_owned())
        .collect();
    match start(args) {
        Some(m) => Box::into_raw(Box::new(m)),
        None => std::ptr::null_mut(),
    }
}

pub fn start(args: Vec<String>) -> Option<XaraModem> {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return None;
    }
    capture_log();
    let stop = Arc::new(AtomicBool::new(false));
    let s = stop.clone();
    let thread = thread::Builder::new()
        .name("xara".into())
        .spawn(move || {
            let code = xara::daemon::run_embedded(&args, &s);
            RUNNING.store(false, Ordering::SeqCst);
            code
        })
        .inspect_err(|_| RUNNING.store(false, Ordering::SeqCst))
        .ok()?;
    Some(XaraModem { stop, thread: Some(thread) })
}

impl XaraModem {
    pub fn running(&self) -> bool {
        self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }

    pub fn stop(mut self) -> u8 {
        self.stop.store(true, Ordering::SeqCst);
        self.thread.take().map_or(0, |t| t.join().unwrap_or(101))
    }
}

/// # Safety
/// `modem` must come from `xara_start` and not have been stopped.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xara_running(modem: *const XaraModem) -> bool {
    // SAFETY: guaranteed by the caller.
    unsafe { modem.as_ref() }.is_some_and(XaraModem::running)
}

/// # Safety
/// `modem` must come from `xara_start` and not have been stopped; it is freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xara_stop(modem: *mut XaraModem) -> u8 {
    if modem.is_null() {
        return 0;
    }
    // SAFETY: guaranteed by the caller.
    unsafe { Box::from_raw(modem) }.stop()
}

pub fn next_log() -> Option<String> {
    LOG.lock().unwrap_or_else(PoisonError::into_inner).pop_front()
}

/// The next piece of received data, rendered as text (see `xara::rxtext`).
pub fn next_received() -> Option<String> {
    RX.lock().unwrap_or_else(PoisonError::into_inner).pop_front()
}

/// # Safety
/// `buf` must have room for `cap` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xara_next_log(buf: *mut c_char, cap: usize) -> bool {
    // SAFETY: guaranteed by the caller.
    unsafe { copy_out(next_log(), buf, cap) }
}

/// # Safety
/// `buf` must have room for `cap` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xara_next_received(buf: *mut c_char, cap: usize) -> bool {
    // SAFETY: guaranteed by the caller.
    unsafe { copy_out(next_received(), buf, cap) }
}

/// # Safety
/// `buf` must have room for `cap` bytes.
unsafe fn copy_out(item: Option<String>, buf: *mut c_char, cap: usize) -> bool {
    if cap == 0 {
        return false;
    }
    let Some(line) = item else { return false };
    let mut n = line.len().min(cap - 1);
    while !line.is_char_boundary(n) {
        n -= 1;
    }
    // SAFETY: buf has room for cap > n bytes.
    unsafe {
        std::ptr::copy_nonoverlapping(line.as_ptr(), buf.cast(), n);
        *buf.add(n) = 0;
    }
    true
}
