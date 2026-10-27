use std::collections::VecDeque;
use std::ffi::c_void;
use std::io;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use super::coreaudio_ffi::{
    self as ffi, DeviceInfo, Queue, QueueBuffer, QueueRef, StreamFormat, TRANSPORT_AGGREGATE, TRANSPORT_VIRTUAL,
};
use super::{Audio, BLOCK, DeviceList, RATE, STALL, capture_only, remaining};

const RX_BUFFERS: usize = 4;
const TX_BUFFERS: usize = 3;
const BUFFER_FRAMES: usize = BLOCK;
const TX_TARGET: usize = 2 * BLOCK;
const RX_LIMIT: usize = 2 * RATE as usize;

#[derive(Default)]
struct State {
    rx: VecDeque<i16>,
    tx: VecDeque<i16>,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    moved: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

extern "C" fn captured(
    user: *mut c_void,
    q: QueueRef,
    b: *mut QueueBuffer,
    _: *const c_void,
    _: u32,
    _: *const c_void,
) {
    // SAFETY: user is the Shared the queue was created with and outlives it; b is a filled buffer of q.
    unsafe {
        let sh = &*user.cast::<Shared>();
        let buf = &*b;
        let x = std::slice::from_raw_parts(buf.data.cast::<i16>(), buf.size as usize / 2);
        let mut st = sh.lock();
        st.rx.extend(x);
        let over = st.rx.len().saturating_sub(RX_LIMIT);
        st.rx.drain(..over);
        drop(st);
        sh.moved.notify_all();
        let _ = ffi::enqueue(q, b);
    }
}

extern "C" fn drained(user: *mut c_void, q: QueueRef, b: *mut QueueBuffer) {
    // SAFETY: as for captured; the buffer holds capacity bytes for us to fill.
    unsafe {
        let sh = &*user.cast::<Shared>();
        let buf = &mut *b;
        let out = std::slice::from_raw_parts_mut(buf.data.cast::<i16>(), buf.capacity as usize / 2);
        let mut st = sh.lock();
        let n = st.tx.len().min(out.len());
        for (o, v) in out.iter_mut().zip(st.tx.drain(..n)) {
            *o = v;
        }
        out[n..].fill(0);
        drop(st);
        buf.size = buf.capacity;
        sh.moved.notify_all();
        let _ = ffi::enqueue(q, b);
    }
}

fn is_default(name: &str) -> bool {
    name.is_empty() || name == "default"
}

fn refuse_default(name: &str, radio: bool) -> io::Result<()> {
    if radio && is_default(name) {
        return Err(io::Error::other("--radio needs the playback device named explicitly, not a default"));
    }
    Ok(())
}

fn find<'a>(devs: &'a [DeviceInfo], name: &str, input: bool) -> io::Result<&'a DeviceInfo> {
    let what = if input { "capture" } else { "playback" };
    let fits = |d: &&DeviceInfo| if input { d.inputs } else { d.outputs };
    if let Some(d) = devs.iter().filter(fits).find(|d| d.uid == name) {
        return Ok(d);
    }
    let named: Vec<_> = devs.iter().filter(fits).filter(|d| d.name == name).collect();
    match named[..] {
        [d] => Ok(d),
        [] => Err(io::Error::other(format!("no {what} device named {name:?} (xara --list-devices)"))),
        _ => {
            let uids: Vec<_> = named.iter().map(|d| d.uid.as_str()).collect();
            Err(io::Error::other(format!("{what} name {name:?} is ambiguous; use a device UID: {}", uids.join(", "))))
        }
    }
}

fn check_playback(devs: &[DeviceInfo], name: &str, radio: bool) -> io::Result<()> {
    refuse_default(name, radio)?;
    if is_default(name) {
        return Err(io::Error::other(
            "refusing playback to the default output: name a virtual device such as BlackHole \
             (a real radio interface needs --radio)",
        ));
    }
    let d = find(devs, name, false)?;
    if !radio && d.transport != TRANSPORT_VIRTUAL {
        return Err(io::Error::other(format!(
            "refusing playback to {name:?}: not a virtual audio device such as BlackHole \
             (a real radio interface needs --radio)"
        )));
    }
    Ok(())
}

pub fn list() -> io::Result<(DeviceList, DeviceList)> {
    let devs = ffi::devices()?;
    let desc = |d: &DeviceInfo| match d.transport {
        TRANSPORT_VIRTUAL => format!("{} (virtual)", d.name),
        TRANSPORT_AGGREGATE => format!("{} (aggregate)", d.name),
        _ => d.name.clone(),
    };
    let pick = |input: bool| {
        devs.iter().filter(|d| if input { d.inputs } else { d.outputs }).map(|d| (d.uid.clone(), desc(d))).collect()
    };
    Ok((pick(true), pick(false)))
}

pub fn check(playback: &str, radio: bool) -> io::Result<()> {
    refuse_default(playback, radio)?;
    check_playback(&ffi::devices()?, playback, radio)
}

pub struct CoreAudioIo {
    _rec: Queue,
    _play: Option<Queue>,
    shared: Arc<Shared>,
    playing: bool,
}

impl CoreAudioIo {
    pub fn open(capture: &str, playback: &str, radio: bool) -> io::Result<Self> {
        Self::connect(capture, Some((playback, radio)))
    }

    pub fn open_capture(capture: &str) -> io::Result<Self> {
        Self::connect(capture, None)
    }

    fn connect(capture: &str, playback: Option<(&str, bool)>) -> io::Result<Self> {
        if let Some((name, radio)) = playback {
            refuse_default(name, radio)?;
        }
        let devs = ffi::devices()?;
        let rec_uid = if is_default(capture) { None } else { Some(find(&devs, capture, true)?.uid.clone()) };
        let play_uid = match playback {
            Some((name, radio)) => {
                check_playback(&devs, name, radio)?;
                Some(find(&devs, name, false)?.uid.clone())
            }
            None => None,
        };
        let shared = Arc::new(Shared::default());
        let user = Arc::as_ptr(&shared).cast_mut().cast::<c_void>();
        let fmt = StreamFormat::s16_mono(RATE);
        let bytes = (BUFFER_FRAMES * 2) as u32;
        // SAFETY: shared outlives both queues, which are declared before it in Self and so dropped first.
        let rec = unsafe { Queue::input(&fmt, captured, user)? };
        if let Some(uid) = &rec_uid {
            rec.set_device(uid)?;
        }
        for _ in 0..RX_BUFFERS {
            // SAFETY: the buffer was just allocated from rec.
            unsafe { ffi::enqueue(rec.raw(), rec.allocate(bytes)?)? };
        }
        let play = match &play_uid {
            Some(uid) => {
                // SAFETY: as for rec.
                let p = unsafe { Queue::output(&fmt, drained, user)? };
                p.set_device(uid)?;
                for _ in 0..TX_BUFFERS {
                    let b = p.allocate(bytes)?;
                    drained(user, p.raw(), b);
                }
                Some(p)
            }
            None => None,
        };
        rec.start().map_err(|e| io::Error::other(format!("capture {capture:?}: {e}")))?;
        if let Some(p) = &play {
            p.start().map_err(|e| io::Error::other(format!("playback {:?}: {e}", play_uid.unwrap_or_default())))?;
        }
        Ok(Self { playing: play.is_some(), _rec: rec, _play: play, shared })
    }
}

impl Audio for CoreAudioIo {
    fn read(&mut self, x: &mut [i16]) -> io::Result<()> {
        let until = Instant::now() + STALL;
        let mut st = self.shared.lock();
        while st.rx.len() < x.len() {
            let wait = remaining(until, "capture (no audio)")?;
            st = self.shared.moved.wait_timeout(st, wait).unwrap_or_else(PoisonError::into_inner).0;
        }
        let n = x.len();
        for (v, s) in x.iter_mut().zip(st.rx.drain(..n)) {
            *v = s;
        }
        Ok(())
    }

    fn write(&mut self, x: &[i16]) -> io::Result<()> {
        if !self.playing {
            return Err(capture_only());
        }
        let until = Instant::now() + STALL;
        let mut st = self.shared.lock();
        st.tx.extend(x);
        while st.tx.len() > TX_TARGET {
            let wait = remaining(until, "playback (device not draining)")?;
            st = self.shared.moved.wait_timeout(st, wait).unwrap_or_else(PoisonError::into_inner).0;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(uid: &str, name: &str, outputs: bool, transport: u32) -> DeviceInfo {
        DeviceInfo { uid: uid.into(), name: name.into(), inputs: !outputs, outputs, transport }
    }

    #[test]
    fn playback_device_checks() {
        let usb = u32::from_be_bytes(*b"usb ");
        let devs = [
            dev("BlackHole2ch_UID", "BlackHole 2ch", true, TRANSPORT_VIRTUAL),
            dev("AppleUSBAudioEngine:1", "USB Audio CODEC", true, usb),
            dev("AppleUSBAudioEngine:2", "USB Audio CODEC", true, usb),
            dev("AppleUSBAudioEngine:3", "Mic", false, usb),
        ];
        let err = |name: &str, radio: bool| check_playback(&devs, name, radio).err().map(|e| e.to_string());
        for name in ["default", ""] {
            assert!(err(name, true).is_some_and(|e| e.contains("named explicitly")), "{name}");
            assert!(err(name, false).is_some_and(|e| e.contains("default output")), "{name}");
        }
        assert!(err("nothing", true).is_some_and(|e| e.contains("no playback device")));
        assert!(err("Mic", true).is_some_and(|e| e.contains("no playback device")));
        assert!(err("USB Audio CODEC", true).is_some_and(|e| e.contains("ambiguous")));
        assert!(err("AppleUSBAudioEngine:1", false).is_some_and(|e| e.contains("not a virtual audio device")));
        assert_eq!(
            (err("AppleUSBAudioEngine:2", true), err("BlackHole 2ch", false), err("BlackHole2ch_UID", true)),
            (None, None, None)
        );
    }
}
