//! Windows audio through WASAPI in shared mode. Windows converts our 48 kHz mono 16-bit
//! stream to and from each device's own format (AUTOCONVERTPCM), so any endpoint works,
//! including virtual cables such as VB-Audio's that VARA itself is usually set up with.
//!
//! Each stream runs on its own thread, which owns its COM objects (they are not Send), and is
//! woken by the device's event. The read/write side keeps capture in step with playback the
//! same way the CoreAudio backend does; see `State` there.

use std::collections::VecDeque;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
    AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, DEVICE_STATE_ACTIVE, EDataFlow,
    IAudioCaptureClient, IAudioClient, IAudioRenderClient, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator,
    WAVE_FORMAT_PCM, WAVEFORMATEX, eCapture, eConsole, eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, STGM_READ,
};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::core::HSTRING;

use super::{Audio, BLOCK, DeviceList, RATE, STALL, capture_only, remaining};

/// The device buffer WASAPI is asked for, in 100 ns units: 40 ms.
const BUFFER_HNS: i64 = 400_000;
const TX_TARGET: usize = 2 * BLOCK;
const RX_LIMIT: usize = 2 * RATE as usize;
/// How often a stream thread looks at the closed flag when its device is quiet, in ms.
const POLL_MS: u32 = 100;

#[derive(Default)]
struct State {
    rx: VecDeque<i16>,
    tx: VecDeque<i16>,
    started: bool,
    flush: bool,
    skip: usize,
    failed: Option<String>,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    moved: Condvar,
    closed: AtomicBool,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn fail(&self, why: String) {
        self.lock().failed.get_or_insert(why);
        self.moved.notify_all();
    }
}

fn win(what: &str) -> impl Fn(windows::core::Error) -> io::Error + '_ {
    move |e| io::Error::other(format!("{what}: {e}"))
}

fn com_init() {
    // SAFETY: plain COM initialisation of the calling thread; S_FALSE (already done) is fine too.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
}

fn enumerator() -> io::Result<IMMDeviceEnumerator> {
    com_init();
    // SAFETY: MMDeviceEnumerator is the documented CLSID for IMMDeviceEnumerator.
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.map_err(win("audio devices"))
}

#[derive(Clone, Debug)]
struct DeviceInfo {
    id: String,
    name: String,
}

impl DeviceInfo {
    /// Virtual cables (VB-Audio CABLE, Hi-Fi Cable, VoiceMeeter, Virtual Audio Cable) are
    /// safe to play into without a radio: nothing goes on the air.
    fn is_virtual(&self) -> bool {
        let n = self.name.to_lowercase();
        ["cable", "virtual", "vb-audio", "voicemeeter", "blackhole"].iter().any(|k| n.contains(k))
    }
}

fn describe(d: &IMMDevice) -> io::Result<DeviceInfo> {
    // SAFETY: d is a live endpoint; GetId's string is ours to free.
    unsafe {
        let p = d.GetId().map_err(win("device id"))?;
        let id = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as *const _));
        let name = d
            .OpenPropertyStore(STGM_READ)
            .and_then(|s| s.GetValue(&PKEY_Device_FriendlyName))
            .map(|v| v.to_string())
            .unwrap_or_else(|_| id.clone());
        Ok(DeviceInfo { id, name })
    }
}

fn devices(flow: EDataFlow) -> io::Result<Vec<DeviceInfo>> {
    let e = enumerator()?;
    // SAFETY: plain enumeration calls on live COM objects.
    unsafe {
        let c = e.EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE).map_err(win("audio devices"))?;
        let n = c.GetCount().map_err(win("audio devices"))?;
        (0..n).map(|i| describe(&c.Item(i).map_err(win("audio device"))?)).collect()
    }
}

fn is_default(name: &str) -> bool {
    name.is_empty() || name == "default"
}

/// A device by its endpoint id, its exact name, or a piece of its name that fits only one.
fn find(devs: &[DeviceInfo], name: &str, input: bool) -> io::Result<DeviceInfo> {
    let what = if input { "capture" } else { "playback" };
    if let Some(d) = devs.iter().find(|d| d.id == name || d.name == name) {
        return Ok(d.clone());
    }
    let low = name.to_lowercase();
    let named: Vec<_> = devs.iter().filter(|d| d.name.to_lowercase().contains(&low)).collect();
    match named[..] {
        [d] => Ok(d.clone()),
        [] => Err(io::Error::other(format!("no {what} device named {name:?} (xara --list-devices)"))),
        _ => {
            let names: Vec<_> = named.iter().map(|d| d.name.as_str()).collect();
            Err(io::Error::other(format!("{what} name {name:?} is ambiguous: {}", names.join(", "))))
        }
    }
}

fn check_playback(devs: &[DeviceInfo], name: &str, radio: bool) -> io::Result<DeviceInfo> {
    if is_default(name) {
        return Err(io::Error::other(if radio {
            "--radio needs the playback device named explicitly, not a default"
        } else {
            "refusing playback to the default output: name a virtual device such as \
             CABLE Input (VB-Audio Virtual Cable) (a real radio interface needs --radio)"
        }));
    }
    let d = find(devs, name, false)?;
    if !radio && !d.is_virtual() {
        return Err(io::Error::other(format!(
            "refusing playback to {:?}: not a virtual audio device such as VB-Audio CABLE \
             (a real radio interface needs --radio)",
            d.name
        )));
    }
    Ok(d)
}

pub fn list() -> io::Result<(DeviceList, DeviceList)> {
    let pick = |devs: Vec<DeviceInfo>| {
        devs.into_iter()
            .map(|d| {
                let desc = if d.is_virtual() { format!("{} (virtual)", d.name) } else { d.name.clone() };
                (d.name, desc)
            })
            .collect()
    };
    Ok((pick(devices(eCapture)?), pick(devices(eRender)?)))
}

pub fn check(playback: &str, radio: bool) -> io::Result<()> {
    check_playback(&devices(eRender)?, playback, radio).map(drop)
}

/// One running WASAPI stream: its own thread with its own COM objects.
struct Stream {
    thread: Option<JoinHandle<()>>,
}

struct Event(HANDLE);

impl Drop for Event {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateEventW and is closed only here.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

fn open_client(id: Option<&str>, flow: EDataFlow) -> io::Result<(IAudioClient, Event, u32)> {
    let e = enumerator()?;
    // SAFETY: COM calls on live objects; fmt and the event outlive Initialize/SetEventHandle.
    unsafe {
        let dev = match id {
            Some(id) => e.GetDevice(&HSTRING::from(id)).map_err(win("audio device"))?,
            None => e.GetDefaultAudioEndpoint(flow, eConsole).map_err(win("default audio device"))?,
        };
        let client: IAudioClient = dev.Activate(CLSCTX_ALL, None).map_err(win("audio client"))?;
        let fmt = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_PCM as u16,
            nChannels: 1,
            nSamplesPerSec: RATE,
            nAvgBytesPerSec: RATE * 2,
            nBlockAlign: 2,
            wBitsPerSample: 16,
            cbSize: 0,
        };
        let flags = AUDCLNT_STREAMFLAGS_EVENTCALLBACK
            | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
            | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
        client.Initialize(AUDCLNT_SHAREMODE_SHARED, flags, BUFFER_HNS, 0, &fmt, None).map_err(win("audio format"))?;
        let ev = Event(CreateEventW(None, false, false, None).map_err(win("audio event"))?);
        client.SetEventHandle(ev.0).map_err(win("audio event"))?;
        let frames = client.GetBufferSize().map_err(win("audio buffer"))?;
        Ok((client, ev, frames))
    }
}

fn run_capture(sh: &Shared, client: &IAudioClient, ev: &Event) -> windows::core::Result<()> {
    // SAFETY: the capture client belongs to client; each buffer is released before the next.
    unsafe {
        let cap: IAudioCaptureClient = client.GetService()?;
        client.Start()?;
        while !sh.closed.load(Ordering::SeqCst) {
            if WaitForSingleObject(ev.0, POLL_MS) != WAIT_OBJECT_0 {
                continue;
            }
            while cap.GetNextPacketSize()? > 0 {
                let (mut data, mut frames, mut flags) = (std::ptr::null_mut(), 0u32, 0u32);
                cap.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                let mut st = sh.lock();
                if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null() {
                    st.rx.extend(std::iter::repeat_n(0, frames as usize));
                } else {
                    st.rx.extend(std::slice::from_raw_parts(data.cast::<i16>(), frames as usize));
                }
                let over = st.rx.len().saturating_sub(RX_LIMIT);
                st.rx.drain(..over);
                drop(st);
                cap.ReleaseBuffer(frames)?;
                sh.moved.notify_all();
            }
        }
        let _ = client.Stop();
    }
    Ok(())
}

fn run_render(sh: &Shared, client: &IAudioClient, ev: &Event, size: u32) -> windows::core::Result<()> {
    // SAFETY: the render client belongs to client; GetBuffer's space is filled and released at once.
    unsafe {
        let ren: IAudioRenderClient = client.GetService()?;
        // Prime the buffer with silence so the device starts with something to play.
        ren.GetBuffer(size)?;
        ren.ReleaseBuffer(size, AUDCLNT_BUFFERFLAGS_SILENT.0 as u32)?;
        client.Start()?;
        while !sh.closed.load(Ordering::SeqCst) {
            if WaitForSingleObject(ev.0, POLL_MS) != WAIT_OBJECT_0 {
                continue;
            }
            let free = size - client.GetCurrentPadding()?;
            if free == 0 {
                continue;
            }
            let out = std::slice::from_raw_parts_mut(ren.GetBuffer(free)?.cast::<i16>(), free as usize);
            let mut st = sh.lock();
            let n = st.tx.len().min(out.len());
            for (o, v) in out.iter_mut().zip(st.tx.drain(..n)) {
                *o = v;
            }
            out[n..].fill(0);
            if st.started {
                st.skip = (st.skip + out.len() - n).min(RX_LIMIT);
            } else {
                st.started = true;
                st.flush = true;
            }
            drop(st);
            ren.ReleaseBuffer(free, 0)?;
            sh.moved.notify_all();
        }
        let _ = client.Stop();
    }
    Ok(())
}

impl Stream {
    fn start(sh: &Arc<Shared>, id: Option<String>, input: bool, what: String) -> io::Result<Self> {
        let (ready_tx, ready_rx) = mpsc::channel();
        let sh = sh.clone();
        let name = if input { "wasapi-capture" } else { "wasapi-render" };
        let thread = thread::Builder::new().name(name.into()).spawn(move || {
            let flow = if input { eCapture } else { eRender };
            let (client, ev, size) = match open_client(id.as_deref(), flow) {
                Ok(c) => {
                    let _ = ready_tx.send(Ok(()));
                    c
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            let r = if input { run_capture(&sh, &client, &ev) } else { run_render(&sh, &client, &ev, size) };
            if let Err(e) = r {
                sh.fail(format!("{what}: {e}"));
            }
        })?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self { thread: Some(thread) }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => Err(io::Error::other("audio thread ended")),
        }
    }
}

pub struct WasapiIo {
    shared: Arc<Shared>,
    streams: Vec<Stream>,
    playing: bool,
}

impl WasapiIo {
    pub fn open(capture: &str, playback: &str, radio: bool) -> io::Result<Self> {
        Self::connect(capture, Some((playback, radio)))
    }

    pub fn open_capture(capture: &str) -> io::Result<Self> {
        Self::connect(capture, None)
    }

    fn connect(capture: &str, playback: Option<(&str, bool)>) -> io::Result<Self> {
        let rec = if is_default(capture) { None } else { Some(find(&devices(eCapture)?, capture, true)?) };
        let play = match playback {
            Some((name, radio)) => Some(check_playback(&devices(eRender)?, name, radio)?),
            None => None,
        };
        let shared = Arc::new(Shared::default());
        let mut io = Self { shared: shared.clone(), streams: Vec::new(), playing: play.is_some() };
        let rec_name = rec.as_ref().map_or("default".into(), |d| d.name.clone());
        io.streams.push(
            Stream::start(&shared, rec.map(|d| d.id), true, format!("capture {rec_name:?}"))
                .map_err(|e| io::Error::other(format!("capture {rec_name:?}: {e}")))?,
        );
        if let Some(d) = play {
            let what = format!("playback {:?}", d.name);
            io.streams.push(
                Stream::start(&shared, Some(d.id), false, what.clone())
                    .map_err(|e| io::Error::other(format!("{what}: {e}")))?,
            );
        }
        Ok(io)
    }
}

impl Drop for WasapiIo {
    fn drop(&mut self) {
        self.shared.closed.store(true, Ordering::SeqCst);
        for s in &mut self.streams {
            if let Some(t) = s.thread.take() {
                let _ = t.join();
            }
        }
    }
}

impl Audio for WasapiIo {
    fn read(&mut self, x: &mut [i16]) -> io::Result<()> {
        let until = Instant::now() + STALL;
        let mut st = self.shared.lock();
        while st.rx.len() < x.len() {
            if let Some(why) = &st.failed {
                return Err(io::Error::other(why.clone()));
            }
            let wait = remaining(until, "capture (no audio)")?;
            st = self.shared.moved.wait_timeout(st, wait).unwrap_or_else(PoisonError::into_inner).0;
        }
        let spare = st.rx.len() - x.len();
        let ms = |n: usize| n * 1000 / RATE as usize;
        if st.flush {
            st.flush = false;
            st.skip = 0;
            st.rx.drain(..spare);
            if spare >= BLOCK {
                crate::say!("playback started {} ms after capture; capture skipped ahead to match", ms(spare));
            }
        } else if st.skip > 0 {
            let n = st.skip.min(spare);
            st.rx.drain(..n);
            st.skip -= n;
            if n >= 5 * BLOCK {
                crate::say!("playback ran dry for {} ms; capture skipped ahead to match", ms(n));
            }
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
        if let Some(why) = &st.failed {
            return Err(io::Error::other(why.clone()));
        }
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

    fn dev(id: &str, name: &str) -> DeviceInfo {
        DeviceInfo { id: id.into(), name: name.into() }
    }

    #[test]
    fn playback_device_checks() {
        let devs = [
            dev("{0.0.0.00000000}.{a}", "CABLE Input (VB-Audio Virtual Cable)"),
            dev("{0.0.0.00000000}.{b}", "Speakers (USB Audio CODEC )"),
            dev("{0.0.0.00000000}.{c}", "Speakers (Realtek(R) Audio)"),
        ];
        let err = |name: &str, radio: bool| check_playback(&devs, name, radio).err().map(|e| e.to_string());
        for name in ["default", ""] {
            assert!(err(name, true).is_some_and(|e| e.contains("named explicitly")), "{name}");
            assert!(err(name, false).is_some_and(|e| e.contains("default output")), "{name}");
        }
        assert!(err("nothing", true).is_some_and(|e| e.contains("no playback device")));
        assert!(err("Speakers", true).is_some_and(|e| e.contains("ambiguous")));
        assert!(err("USB Audio CODEC", false).is_some_and(|e| e.contains("not a virtual audio device")));
        assert_eq!((err("USB Audio CODEC", true), err("CABLE Input", false), err("{0.0.0.00000000}.{a}", false)), (
            None, None, None
        ));
    }
}
