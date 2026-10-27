//! Android audio through AAudio. Android has no device names a command line could use, so
//! devices are AudioDeviceInfo ids that the app looks up, or "default" for the system's choice.

use std::io;

use super::aaudio_ffi::{DIRECTION_INPUT, DIRECTION_OUTPUT, PRESET_UNPROCESSED, PRESET_VOICE_RECOGNITION, Stream};
use super::{Audio, BLOCK, DeviceList, RATE, STALL, capture_only};

const TX_BUFFER: usize = 2 * BLOCK;

fn device_id(name: &str, what: &str) -> io::Result<i32> {
    if name.is_empty() || name == "default" {
        return Ok(0);
    }
    name.parse::<i32>()
        .ok()
        .filter(|&id| id > 0)
        .ok_or_else(|| io::Error::other(format!("{what} {name:?}: expected an Android audio device id or default")))
}

fn check_playback(name: &str, radio: bool) -> io::Result<i32> {
    if !radio {
        return Err(io::Error::other(
            "refusing playback: Android has no virtual audio devices, so playback needs --radio",
        ));
    }
    match device_id(name, "playback")? {
        0 => Err(io::Error::other("--radio needs the playback device named explicitly, not a default")),
        id => Ok(id),
    }
}

pub fn check(playback: &str, radio: bool) -> io::Result<()> {
    check_playback(playback, radio).map(drop)
}

pub fn list() -> io::Result<(DeviceList, DeviceList)> {
    let d = || vec![("default".to_string(), "the system's choice; the app lists device ids".to_string())];
    Ok((d(), d()))
}

fn capture(name: &str) -> io::Result<Stream> {
    let id = device_id(name, "capture")?;
    // UNPROCESSED skips Android's input processing but is optional; voice recognition is
    // the closest preset every device has.
    Stream::open(DIRECTION_INPUT, id, RATE, PRESET_UNPROCESSED)
        .or_else(|_| Stream::open(DIRECTION_INPUT, id, RATE, PRESET_VOICE_RECOGNITION))
}

pub struct AAudioIo {
    rec: Stream,
    play: Option<Stream>,
}

impl AAudioIo {
    pub fn open(capture_name: &str, playback: &str, radio: bool) -> io::Result<Self> {
        let id = check_playback(playback, radio)?;
        let play = Stream::open(DIRECTION_OUTPUT, id, RATE, 0)?;
        if play.device() != id {
            return Err(io::Error::other(format!("playback {id}: Android opened device {} instead", play.device())));
        }
        play.limit_buffer(TX_BUFFER);
        Ok(Self { rec: capture(capture_name)?, play: Some(play) })
    }

    pub fn open_capture(capture_name: &str) -> io::Result<Self> {
        Ok(Self { rec: capture(capture_name)?, play: None })
    }
}

fn nanos(d: std::time::Duration) -> i64 {
    d.as_nanos().min(i64::MAX as u128) as i64
}

impl Audio for AAudioIo {
    fn read(&mut self, x: &mut [i16]) -> io::Result<()> {
        if self.rec.read(x, nanos(STALL))? < x.len() {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "capture (no audio): timed out"));
        }
        Ok(())
    }

    fn write(&mut self, x: &[i16]) -> io::Result<()> {
        let Some(play) = &self.play else { return Err(capture_only()) };
        if play.write(x, nanos(STALL))? < x.len() {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "playback (device not draining): timed out"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_device_checks() {
        let err = |name: &str, radio: bool| check_playback(name, radio).err().map(|e| e.to_string());
        assert!(err("12", false).is_some_and(|e| e.contains("needs --radio")));
        for name in ["", "default"] {
            assert!(err(name, true).is_some_and(|e| e.contains("named explicitly")), "{name}");
        }
        for name in ["usb", "-3", "0"] {
            assert!(err(name, true).is_some_and(|e| e.contains("device id")), "{name}");
        }
        assert_eq!(check_playback("12", true).ok(), Some(12));
    }
}
