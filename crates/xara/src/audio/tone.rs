//! PTT by a tone on the right channel, for interfaces that key the radio from audio (the
//! Digirig Lite and other "VOX by tone" cables): with it on, playback is stereo, the modem's
//! audio on the left and, while keyed, a steady tone on the right. The interface turns the
//! tone into PTT and doesn't pass it to the radio. Core Audio (macOS, iOS) and AAudio
//! (Android) play it.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// The tone when none is given: 1 kHz is known to key the Digirig Lite.
pub const DEFAULT_HZ: u32 = 1000;
/// Its level: near full scale, so the interface keys at any sensible volume.
const LEVEL: f64 = 0.8 * i16::MAX as f64;

/// The tone in Hz while PTT by tone is on; 0 = off (playback is mono).
static HZ: AtomicU32 = AtomicU32::new(0);
static KEYED: AtomicBool = AtomicBool::new(false);

/// Set before the audio opens: Some(hz) plays in stereo with the tone, None in mono.
pub fn configure(hz: Option<u32>) {
    HZ.store(hz.unwrap_or(0), Ordering::SeqCst);
    KEYED.store(false, Ordering::SeqCst);
}

pub fn enabled() -> bool {
    HZ.load(Ordering::SeqCst) != 0
}

/// Set by the `tone` PTT, read by the audio.
pub fn set_keyed(on: bool) {
    KEYED.store(on, Ordering::SeqCst);
}

/// Stereo playback for the modem's mono samples: left the modem, right the tone while
/// keyed (else silence). `phase` (0..1) carries the tone from one call to the next.
#[derive(Default)]
pub struct Stereo {
    phase: f64,
}

impl Stereo {
    /// `out` holds two samples per sample of `mono`.
    pub fn fill(&mut self, mono: &[i16], out: &mut [i16]) {
        let hz = HZ.load(Ordering::Relaxed);
        let keyed = hz != 0 && KEYED.load(Ordering::Relaxed);
        let step = f64::from(hz) / f64::from(super::RATE);
        for (frame, &m) in out.chunks_exact_mut(2).zip(mono) {
            frame[0] = m;
            frame[1] = if keyed {
                let v = (self.phase * std::f64::consts::TAU).sin() * LEVEL;
                self.phase = (self.phase + step).fract();
                v as i16
            } else {
                // (starts at a zero crossing next time: no click)
                self.phase = 0.0;
                0
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tone_on_the_right_while_keyed() {
        configure(Some(1000));
        let mut s = Stereo::default();
        let mono: Vec<i16> = (0..96).map(|i| i as i16).collect();
        let mut out = vec![1i16; 192];
        s.fill(&mono, &mut out);
        assert!(out.chunks(2).enumerate().all(|(i, f)| f[0] == i as i16 && f[1] == 0), "not keyed: silence on the right");
        set_keyed(true);
        s.fill(&mono, &mut out);
        let right: Vec<i16> = out.chunks(2).map(|f| f[1]).collect();
        assert!(out.chunks(2).enumerate().all(|(i, f)| f[0] == i as i16), "the modem's audio stays on the left");
        // 1 kHz at 48 kHz: 48 samples a cycle, peaks at 12 and 36
        assert_eq!((right[0], right[24]), (0, 0));
        assert!(right[12] > 26000 && right[36] < -26000, "{right:?}");
        // the tone carries on across calls without a jump
        s.fill(&mono, &mut out);
        assert_eq!(out[1], 0);
        assert!(out[25] > 26000);
        configure(None);
        assert!(!enabled());
    }
}
