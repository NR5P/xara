#[cfg(aaudio)]
pub mod aaudio;
#[cfg(aaudio)]
mod aaudio_ffi;
#[cfg(alsa)]
pub mod alsa;
#[cfg(coreaudio)]
pub mod coreaudio;
#[cfg(coreaudio)]
mod coreaudio_ffi;
#[cfg(pulse)]
pub mod pulse;
#[cfg(pulse)]
mod pulse_ffi;
pub mod tone;
pub mod trusdx;
#[cfg(wasapi)]
pub mod wasapi;

#[cfg(not(any(pulse, alsa, coreaudio, aaudio, wasapi)))]
compile_error!(
    "xara needs an audio backend: enable the pulse or alsa feature (Linux), or build for macOS, iOS, Android or Windows"
);

use std::collections::VecDeque;
use std::io;
use std::time::Duration;

pub const RATE: u32 = 48000;
pub const BLOCK: usize = 480;
pub(crate) const STALL: Duration = Duration::from_secs(1);

pub type DeviceList = Vec<(String, String)>;

fn capture_only() -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, "write to a capture-only audio device")
}

#[cfg(any(pulse, alsa, coreaudio, wasapi))]
fn remaining(until: std::time::Instant, what: &str) -> io::Result<Duration> {
    until
        .checked_duration_since(std::time::Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, format!("{what}: timed out")))
}

pub trait Audio {
    fn read(&mut self, x: &mut [i16]) -> io::Result<()>;
    fn write(&mut self, x: &[i16]) -> io::Result<()>;
    /// How long audio written now takes to reach the output (a sound server's buffer):
    /// allowed for like --audio-latency-ms, so PTT stays keyed until it has played out.
    fn output_latency(&self) -> std::time::Duration {
        std::time::Duration::ZERO
    }

    /// Delay the backend itself adds to the audio path, round trip, in samples: queues and
    /// device buffers between the modem and the sound card. The modem allows for it as it
    /// does for --audio-latency-ms. Backends that read and write the device directly add none.
    fn latency(&self) -> usize {
        0
    }

    /// Whether the block about to be written goes out keyed (the modem is transmitting, so
    /// what capture hears doesn't matter). Said before each write.
    fn keyed(&mut self, _on: bool) {}
}

/// Owed silence beyond this (100 ms) is made up from capture even while the modem may be
/// listening, so a playback clock that runs fast can't add delay without bound.
const OWED_LIMIT: usize = RATE as usize / 10;

/// Silence playback filled in when it ran dry, for the backends that keep capture and
/// playback in lockstep (WASAPI, CoreAudio). Everything written after it reaches the air that
/// much later than the modem allows for, so it has to be made up. Dropping captured audio
/// does it, but a loop turn that runs long (decoding a big frame takes tens of ms) mostly
/// lands while a frame is still coming in: cutting that audio loses the frame (the second
/// of a double frame), and a peer that sees frames lost stops raising the speed. So it is
/// made up from silence the modem writes, where there is a block to spare in the queue;
/// from capture only while transmitting (nothing to hear), or once too much is owed.
#[cfg_attr(not(any(coreaudio, wasapi)), allow(dead_code))]
#[derive(Debug, Default)]
pub(crate) struct Owed {
    samples: usize,
    /// The modem is transmitting (see `Audio::keyed`).
    sending: bool,
}

#[cfg_attr(not(any(coreaudio, wasapi)), allow(dead_code))]
impl Owed {
    pub(crate) fn ran_dry(&mut self, n: usize, limit: usize) {
        self.samples = (self.samples + n).min(limit);
    }

    pub(crate) fn keyed(&mut self, on: bool) {
        self.sending = on;
    }

    /// How many samples to take off the end of the playback queue, which has just had `x`
    /// added and now holds `queued`: silence written unkeyed only (a frame has quiet spells
    /// of its own), and never below a block.
    pub(crate) fn take_from_playback(&mut self, x: &[i16], queued: usize) -> usize {
        if self.sending || x.iter().any(|&v| v != 0) {
            return 0;
        }
        let n = self.samples.min(x.len()).min(queued.saturating_sub(BLOCK));
        self.samples -= n;
        n
    }

    /// How many of the `spare` captured samples beyond the next block to drop.
    pub(crate) fn take_from_capture(&mut self, spare: usize) -> usize {
        if !self.sending && self.samples < OWED_LIMIT {
            return 0;
        }
        let n = self.samples.min(spare);
        self.samples -= n;
        n
    }
}

pub fn sample(v: i16) -> f64 {
    f64::from(v) / 32768.0
}

pub fn quantize(v: f64) -> i16 {
    (v * 32768.0).round().clamp(-32768.0, 32767.0) as i16
}

pub struct TxAudio {
    delay: VecDeque<f64>,
}

impl TxAudio {
    pub fn new(lead_samples: usize) -> Self {
        Self { delay: std::iter::repeat_n(0.0, lead_samples).collect() }
    }

    pub fn process(&mut self, out: &mut [f64], mute: bool) {
        if mute {
            out.fill(0.0);
            self.delay.iter_mut().for_each(|v| *v = 0.0);
        }
        if !self.delay.is_empty() {
            self.delay.extend(out.iter().copied());
            for o in out.iter_mut() {
                *o = self.delay.pop_front().unwrap();
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Pulse,
    Alsa,
    CoreAudio,
    AAudio,
    Wasapi,
    /// A (tr)uSDX's audio inside its CAT serial stream, on any platform.
    Trusdx,
}

impl Default for Backend {
    fn default() -> Self {
        if cfg!(coreaudio) {
            Self::CoreAudio
        } else if cfg!(aaudio) {
            Self::AAudio
        } else if cfg!(wasapi) {
            Self::Wasapi
        } else if cfg!(pulse) {
            Self::Pulse
        } else {
            Self::Alsa
        }
    }
}

impl std::str::FromStr for Backend {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        let b = match s {
            "pulse" => Self::Pulse,
            "alsa" => Self::Alsa,
            "coreaudio" => Self::CoreAudio,
            "aaudio" => Self::AAudio,
            "wasapi" => Self::Wasapi,
            "trusdx" => Self::Trusdx,
            _ => return Err(format!("--audio {s:?}: expected pulse, alsa, coreaudio, aaudio, wasapi or trusdx")),
        };
        if b.built() { Ok(b) } else { Err(format!("--audio {s}: {}", unbuilt_why(b))) }
    }
}

impl Backend {
    pub fn built(self) -> bool {
        match self {
            Self::Pulse => cfg!(pulse),
            Self::Alsa => cfg!(alsa),
            Self::CoreAudio => cfg!(coreaudio),
            Self::AAudio => cfg!(aaudio),
            Self::Wasapi => cfg!(wasapi),
            Self::Trusdx => true,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Pulse => "pulse",
            Self::Alsa => "alsa",
            Self::CoreAudio => "coreaudio",
            Self::AAudio => "aaudio",
            Self::Wasapi => "wasapi",
            Self::Trusdx => "trusdx",
        }
    }
}

fn unbuilt_why(b: Backend) -> String {
    match b {
        Backend::CoreAudio => "CoreAudio is only available on macOS and iOS".into(),
        Backend::AAudio => "AAudio is only available on Android".into(),
        Backend::Wasapi => "WASAPI is only available on Windows".into(),
        _ if !cfg!(target_os = "linux") => format!("{} is only available on Linux", b.name()),
        _ => format!("this xara was built without the {} feature", b.name()),
    }
}

#[cfg(not(all(pulse, alsa, coreaudio, aaudio, wasapi)))]
fn unbuilt(b: Backend) -> io::Error {
    io::Error::other(unbuilt_why(b))
}

pub fn open(b: Backend, capture: &str, playback: &str, radio: bool) -> io::Result<Box<dyn Audio>> {
    match b {
        #[cfg(pulse)]
        Backend::Pulse => Ok(Box::new(pulse::PulseIo::open(capture, playback, radio)?)),
        #[cfg(alsa)]
        Backend::Alsa => Ok(Box::new(alsa::AlsaIo::open(capture, playback, radio)?)),
        #[cfg(coreaudio)]
        Backend::CoreAudio => Ok(Box::new(coreaudio::CoreAudioIo::open(capture, playback, radio)?)),
        #[cfg(aaudio)]
        Backend::AAudio => Ok(Box::new(aaudio::AAudioIo::open(capture, playback, radio)?)),
        #[cfg(wasapi)]
        Backend::Wasapi => Ok(Box::new(wasapi::WasapiIo::open(capture, playback, radio)?)),
        Backend::Trusdx => Ok(Box::new(trusdx::TrusdxIo::open(capture, playback, radio)?)),
        #[cfg(not(all(pulse, alsa, coreaudio, aaudio, wasapi)))]
        _ => Err(unbuilt(b)),
    }
}

pub fn check_playback(b: Backend, playback: &str, radio: bool) -> io::Result<()> {
    match b {
        #[cfg(pulse)]
        Backend::Pulse => pulse::check(playback, radio),
        #[cfg(alsa)]
        Backend::Alsa => alsa::resolve_playback(playback, radio).map(drop),
        #[cfg(coreaudio)]
        Backend::CoreAudio => coreaudio::check(playback, radio),
        #[cfg(aaudio)]
        Backend::AAudio => aaudio::check(playback, radio),
        #[cfg(wasapi)]
        Backend::Wasapi => wasapi::check(playback, radio),
        Backend::Trusdx => trusdx::check(playback, radio),
        #[cfg(not(all(pulse, alsa, coreaudio, aaudio, wasapi)))]
        _ => Err(unbuilt(b)),
    }
}

pub fn list(b: Backend) -> io::Result<(DeviceList, DeviceList)> {
    match b {
        #[cfg(pulse)]
        Backend::Pulse => pulse::list(),
        #[cfg(alsa)]
        Backend::Alsa => alsa::list(),
        #[cfg(coreaudio)]
        Backend::CoreAudio => coreaudio::list(),
        #[cfg(aaudio)]
        Backend::AAudio => aaudio::list(),
        #[cfg(wasapi)]
        Backend::Wasapi => wasapi::list(),
        Backend::Trusdx => trusdx::list(),
        #[cfg(not(all(pulse, alsa, coreaudio, aaudio, wasapi)))]
        _ => Err(unbuilt(b)),
    }
}

pub fn open_capture(b: Backend, capture: &str) -> io::Result<Box<dyn Audio>> {
    match b {
        #[cfg(pulse)]
        Backend::Pulse => Ok(Box::new(pulse::PulseIo::open_capture(capture)?)),
        #[cfg(alsa)]
        Backend::Alsa => Ok(Box::new(alsa::AlsaIo::open_capture(capture)?)),
        #[cfg(coreaudio)]
        Backend::CoreAudio => Ok(Box::new(coreaudio::CoreAudioIo::open_capture(capture)?)),
        #[cfg(aaudio)]
        Backend::AAudio => Ok(Box::new(aaudio::AAudioIo::open_capture(capture)?)),
        #[cfg(wasapi)]
        Backend::Wasapi => Ok(Box::new(wasapi::WasapiIo::open_capture(capture)?)),
        Backend::Trusdx => Ok(Box::new(trusdx::TrusdxIo::open_capture(capture)?)),
        #[cfg(not(all(pulse, alsa, coreaudio, aaudio, wasapi)))]
        _ => Err(unbuilt(b)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owed_silence_comes_out_of_silence_while_listening() {
        let (quiet, sound) = ([0i16; BLOCK], [1i16; BLOCK]);
        let mut o = Owed::default();
        o.ran_dry(4 * BLOCK, 2 * RATE as usize);
        // Listening: captured audio is never cut, however much is spare.
        assert_eq!(o.take_from_capture(10 * BLOCK), 0);
        // Silence written comes off the queue, keeping a block queued.
        assert_eq!(o.take_from_playback(&quiet, BLOCK), 0);
        assert_eq!(o.take_from_playback(&quiet, 2 * BLOCK), BLOCK);
        assert_eq!(o.take_from_playback(&quiet, 3 * BLOCK), BLOCK);
        assert_eq!(o.take_from_playback(&sound, 3 * BLOCK), 0);
        assert_eq!(o.samples, 2 * BLOCK);
        // Transmitting: nothing written is dropped, quiet spells in a frame included;
        // capture makes up the rest.
        o.keyed(true);
        assert_eq!(o.take_from_playback(&quiet, 3 * BLOCK), 0);
        assert_eq!(o.take_from_capture(BLOCK / 2), BLOCK / 2);
        assert_eq!(o.take_from_capture(2 * BLOCK), 3 * BLOCK / 2);
        assert_eq!(o.samples, 0);
    }

    #[test]
    fn owed_silence_is_made_up_from_capture_at_once_while_keyed() {
        let mut o = Owed::default();
        o.keyed(true);
        o.ran_dry(BLOCK, 2 * RATE as usize);
        assert_eq!(o.take_from_capture(BLOCK / 2), BLOCK / 2);
        assert_eq!(o.take_from_capture(BLOCK), BLOCK / 2);
        assert_eq!(o.samples, 0);
    }

    #[test]
    fn too_much_owed_comes_out_of_capture_even_while_listening() {
        let mut o = Owed::default();
        o.ran_dry(OWED_LIMIT - 1, 2 * RATE as usize);
        assert_eq!(o.take_from_capture(RATE as usize), 0);
        o.ran_dry(1, 2 * RATE as usize);
        assert_eq!(o.take_from_capture(RATE as usize), OWED_LIMIT);
        o.ran_dry(5 * RATE as usize, 2 * RATE as usize);
        assert_eq!(o.samples, 2 * RATE as usize, "capped");
    }

    #[test]
    fn samples_convert_at_the_device_boundary() {
        for s in [i16::MIN, -1, 0, 1, 12345, i16::MAX] {
            assert_eq!(quantize(sample(s)), s);
        }
        assert_eq!((quantize(1.0), quantize(-1.0), quantize(4.0), quantize(-4.0)), (32767, -32768, 32767, -32768));
        assert_eq!((quantize(0.5 / 32768.0), quantize(-0.4 / 32768.0), quantize(f64::NAN)), (1, 0, 0));
        assert_eq!(sample(-32768), -1.0);
    }
}
