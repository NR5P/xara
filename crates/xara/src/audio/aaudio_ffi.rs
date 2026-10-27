use std::ffi::{CStr, c_char, c_void};
use std::io;
use std::ptr::{NonNull, null_mut};

type Result = i32;

pub const DIRECTION_OUTPUT: i32 = 0;
pub const DIRECTION_INPUT: i32 = 1;
const FORMAT_PCM_I16: i32 = 1;
const SHARING_SHARED: i32 = 1;
const PERFORMANCE_NONE: i32 = 10;
pub const PRESET_VOICE_RECOGNITION: i32 = 6;
pub const PRESET_UNPROCESSED: i32 = 9;
const USAGE_MEDIA: i32 = 1;

#[link(name = "aaudio")]
unsafe extern "C" {
    fn AAudio_createStreamBuilder(b: *mut *mut c_void) -> Result;
    fn AAudio_convertResultToText(r: Result) -> *const c_char;
    fn AAudioStreamBuilder_setDirection(b: *mut c_void, d: i32);
    fn AAudioStreamBuilder_setDeviceId(b: *mut c_void, id: i32);
    fn AAudioStreamBuilder_setSampleRate(b: *mut c_void, rate: i32);
    fn AAudioStreamBuilder_setChannelCount(b: *mut c_void, n: i32);
    fn AAudioStreamBuilder_setFormat(b: *mut c_void, f: i32);
    fn AAudioStreamBuilder_setSharingMode(b: *mut c_void, m: i32);
    fn AAudioStreamBuilder_setPerformanceMode(b: *mut c_void, m: i32);
    fn AAudioStreamBuilder_setInputPreset(b: *mut c_void, p: i32);
    fn AAudioStreamBuilder_setUsage(b: *mut c_void, u: i32);
    fn AAudioStreamBuilder_openStream(b: *mut c_void, s: *mut *mut c_void) -> Result;
    fn AAudioStreamBuilder_delete(b: *mut c_void) -> Result;
    fn AAudioStream_requestStart(s: *mut c_void) -> Result;
    fn AAudioStream_close(s: *mut c_void) -> Result;
    fn AAudioStream_read(s: *mut c_void, buf: *mut c_void, frames: i32, timeout_ns: i64) -> Result;
    fn AAudioStream_write(s: *mut c_void, buf: *const c_void, frames: i32, timeout_ns: i64) -> Result;
    fn AAudioStream_getSampleRate(s: *mut c_void) -> i32;
    fn AAudioStream_getChannelCount(s: *mut c_void) -> i32;
    fn AAudioStream_getFormat(s: *mut c_void) -> i32;
    fn AAudioStream_getDeviceId(s: *mut c_void) -> i32;
    fn AAudioStream_getFramesPerBurst(s: *mut c_void) -> i32;
    fn AAudioStream_setBufferSizeInFrames(s: *mut c_void, frames: i32) -> Result;
}

pub fn error(what: &str, r: Result) -> io::Error {
    // SAFETY: AAudio returns a static string for any result code.
    let text = unsafe { CStr::from_ptr(AAudio_convertResultToText(r)) }.to_string_lossy();
    io::Error::other(format!("{what}: {text}"))
}

fn check(what: &str, r: Result) -> io::Result<i32> {
    if r < 0 { Err(error(what, r)) } else { Ok(r) }
}

/// One open AAudio stream of mono 16-bit samples.
pub struct Stream(NonNull<c_void>);

// SAFETY: an AAudio stream may be used from any one thread at a time.
unsafe impl Send for Stream {}

impl Drop for Stream {
    fn drop(&mut self) {
        // SAFETY: the stream is open and owned by us.
        unsafe { AAudioStream_close(self.0.as_ptr()) };
    }
}

impl Stream {
    /// Opens a stream on `device` (0 for the system's choice) and starts it.
    pub fn open(direction: i32, device: i32, rate: u32, preset: i32) -> io::Result<Self> {
        let what = if direction == DIRECTION_INPUT { "opening capture" } else { "opening playback" };
        let mut b = null_mut();
        // SAFETY: the builder is used only between its creation and deletion below.
        unsafe {
            check(what, AAudio_createStreamBuilder(&mut b))?;
            AAudioStreamBuilder_setDirection(b, direction);
            AAudioStreamBuilder_setDeviceId(b, device);
            AAudioStreamBuilder_setSampleRate(b, rate as i32);
            AAudioStreamBuilder_setChannelCount(b, 1);
            AAudioStreamBuilder_setFormat(b, FORMAT_PCM_I16);
            AAudioStreamBuilder_setSharingMode(b, SHARING_SHARED);
            AAudioStreamBuilder_setPerformanceMode(b, PERFORMANCE_NONE);
            if direction == DIRECTION_INPUT {
                AAudioStreamBuilder_setInputPreset(b, preset);
            } else {
                AAudioStreamBuilder_setUsage(b, USAGE_MEDIA);
            }
            let mut s = null_mut();
            let r = AAudioStreamBuilder_openStream(b, &mut s);
            AAudioStreamBuilder_delete(b);
            check(what, r)?;
            let s = Stream(NonNull::new(s).ok_or_else(|| io::Error::other(format!("{what}: no stream")))?);
            let p = s.0.as_ptr();
            let (got_rate, ch, fmt) =
                (AAudioStream_getSampleRate(p), AAudioStream_getChannelCount(p), AAudioStream_getFormat(p));
            if got_rate != rate as i32 || ch != 1 || fmt != FORMAT_PCM_I16 {
                return Err(io::Error::other(format!(
                    "{what}: the device gave {got_rate} Hz, {ch} channel(s), format {fmt} instead of {rate} Hz mono 16-bit"
                )));
            }
            check(what, AAudioStream_requestStart(p))?;
            Ok(s)
        }
    }

    pub fn device(&self) -> i32 {
        // SAFETY: the stream is open.
        unsafe { AAudioStream_getDeviceId(self.0.as_ptr()) }
    }

    /// Caps how much playback AAudio holds, so writes pace the modem instead of piling up.
    pub fn limit_buffer(&self, frames: usize) {
        // SAFETY: the stream is open.
        unsafe {
            let burst = AAudioStream_getFramesPerBurst(self.0.as_ptr()).max(1) as usize;
            AAudioStream_setBufferSizeInFrames(self.0.as_ptr(), frames.div_ceil(burst) as i32 * burst as i32);
        }
    }

    /// Blocks until `x` is full or `timeout_ns` passes; returns the frames read.
    pub fn read(&self, x: &mut [i16], timeout_ns: i64) -> io::Result<usize> {
        // SAFETY: x has room for x.len() mono frames.
        let r = unsafe { AAudioStream_read(self.0.as_ptr(), x.as_mut_ptr().cast(), x.len() as i32, timeout_ns) };
        check("capture", r).map(|n| n as usize)
    }

    /// Blocks until all of `x` is queued or `timeout_ns` passes; returns the frames written.
    pub fn write(&self, x: &[i16], timeout_ns: i64) -> io::Result<usize> {
        // SAFETY: x holds x.len() mono frames.
        let r = unsafe { AAudioStream_write(self.0.as_ptr(), x.as_ptr().cast(), x.len() as i32, timeout_ns) };
        check("playback", r).map(|n| n as usize)
    }
}
