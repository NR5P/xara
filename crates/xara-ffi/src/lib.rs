//! C interface for embedding xara; see include/xara.h.

use std::collections::VecDeque;
use std::ffi::{CStr, c_char};
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::os::fd::FromRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Once, PoisonError};
use std::thread::{self, JoinHandle};

const LOG_LINES: usize = 1000;

static RUNNING: AtomicBool = AtomicBool::new(false);
static LOG: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
static CAPTURE: Once = Once::new();

pub struct XaraModem {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<u8>>,
}

fn keep(line: String) {
    let mut log = LOG.lock().unwrap_or_else(PoisonError::into_inner);
    if log.len() == LOG_LINES {
        log.pop_front();
    }
    log.push_back(line);
}

/// Sends stderr through a pipe so the app can show xara's log, still copying it to the old stderr.
fn capture_stderr() {
    CAPTURE.call_once(|| {
        let mut fds = [0; 2];
        // SAFETY: plain descriptor calls; fds has room for the pipe's two ends.
        let old = unsafe {
            if libc::pipe(fds.as_mut_ptr()) != 0 {
                return;
            }
            let old = libc::dup(2);
            if old < 0 || libc::dup2(fds[1], 2) < 0 {
                return;
            }
            libc::close(fds[1]);
            old
        };
        // SAFETY: both descriptors are ours alone from here on.
        let (rx, mut old) = unsafe { (File::from_raw_fd(fds[0]), File::from_raw_fd(old)) };
        let _ = thread::Builder::new().name("xara-log".into()).spawn(move || {
            for line in BufReader::new(rx).lines().map_while(Result::ok) {
                let _ = writeln!(old, "{line}");
                keep(line);
            }
        });
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
    capture_stderr();
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

/// # Safety
/// `buf` must have room for `cap` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xara_next_log(buf: *mut c_char, cap: usize) -> bool {
    if cap == 0 {
        return false;
    }
    let Some(line) = next_log() else { return false };
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
