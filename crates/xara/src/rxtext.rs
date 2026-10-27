//! A readable copy of what the modem receives, for an app to show as it arrives. The data
//! still goes to the host's clients unchanged; this only renders it: printable text as is,
//! binary as a summary, and Winlink's compressed blocks as such rather than as noise.

use std::io;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};

use crate::monitor::record::{Body, Record};
use crate::monitor::{Line, Monitor, MonitorOpts};
use xara_phy::hf::level::Bandwidth;

type Sink = Arc<dyn Fn(&str) + Send + Sync>;

static SINK: Mutex<Option<Sink>> = Mutex::new(None);
/// Set while this station has a link, so the passive decoder doesn't repeat its data.
static OWN_LINK: AtomicBool = AtomicBool::new(false);
/// Data bursts the passive decoders heard and frames they decoded, across all bandwidths.
static BURSTS: AtomicUsize = AtomicUsize::new(0);
static DECODED: AtomicUsize = AtomicUsize::new(0);
/// How often to say so when VARA data is heard but none of it decodes.
const UNDECODED_REPORT: std::time::Duration = std::time::Duration::from_secs(120);

/// Sends each rendered piece of received data to `f`. Without a sink nothing is rendered.
pub fn set_sink(f: impl Fn(&str) + Send + Sync + 'static) {
    *SINK.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(f));
}

pub fn has_sink() -> bool {
    sink().is_some()
}

fn sink() -> Option<Sink> {
    SINK.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// Share of a chunk that must be printable for it to be shown as text.
const TEXT_SHARE: f64 = 0.85;

/// What one link has received so far, to tell a Winlink exchange from a chat.
#[derive(Default)]
pub struct RxText {
    winlink: bool,
}

impl RxText {
    pub fn connected(&mut self, caller: &str, answerer: &str) {
        self.winlink = false;
        OWN_LINK.store(true, Ordering::SeqCst);
        emit(|| format!("— connected {caller} → {answerer} —\n"));
    }

    pub fn disconnected(&mut self) {
        self.winlink = false;
        OWN_LINK.store(false, Ordering::SeqCst);
        emit(|| "— disconnected —\n".into());
    }

    pub fn data(&mut self, d: &[u8]) {
        let Some(s) = sink() else { return };
        // B2F opens with an [SID] such as [WL2K-5.0-B2FWIHJM$] and offers messages with FC lines.
        let head = String::from_utf8_lossy(&d[..d.len().min(256)]);
        if head.contains("[WL2K") || head.contains("B2F") || head.lines().any(|l| l.starts_with("FC ")) {
            self.winlink = true;
        }
        s(&render(d, self.winlink));
    }

    pub fn kiss(&mut self, frame: &[u8]) {
        if let Some(s) = sink() {
            s(&format!("[KISS {} bytes] {}\n", frame.len(), printable(frame)));
        }
    }
}

/// Blocks of received audio queued for each passive decoder; past this it skips audio rather
/// than hold up the modem.
const PASSIVE_QUEUE: usize = 200;

/// Bandwidths the passive decoder always listens at; the modem's own is added. 2750 Hz needs
/// registered VARA and costs the most to decode, so it is only searched when the modem uses it.
const PASSIVE_BWS: [Bandwidth; 2] = [Bandwidth::Hz500, Bandwidth::Hz2300];

/// One decoder thread for one bandwidth.
struct Decoder {
    bw: Bandwidth,
    tx: Option<SyncSender<Vec<f64>>>,
    worker: Option<JoinHandle<()>>,
    dropped: AtomicUsize,
}

/// Decodes everyone's VARA HF traffic on the channel, as xara-monitor does, at each common
/// bandwidth at once: a link is only readable by a decoder at its own bandwidth.
pub struct Passive {
    decoders: Vec<Decoder>,
}

impl Passive {
    /// `calls` are stations to name when heard: VARA sends the answerer's call only as a hash,
    /// which shows as #XXXXXX unless the call is known.
    /// With `frames`, every frame the decoders hear is shown, not just links, text and CQs.
    pub fn spawn(bw: Bandwidth, calls: &[String], frames: bool) -> io::Result<Self> {
        let mut bws: Vec<Bandwidth> = PASSIVE_BWS.to_vec();
        if !bws.contains(&bw) {
            bws.push(bw);
        }
        let opts = MonitorOpts { calls: calls.to_vec(), ..MonitorOpts::default() };
        let mut decoders = Vec::new();
        let bws_n = bws.len().max(1);
        for (i, &b) in bws.iter().enumerate() {
            let (tx, rx) = sync_channel::<Vec<f64>>(PASSIVE_QUEUE);
            let opts = opts.clone();
            // CQs decode at any bandwidth, so only the first decoder shows them.
            let mut heard = Heard { bw: b, first: i == 0, frames, winlink: false, pending: Vec::new() };
            let worker = thread::Builder::new().name(format!("passive-{}", b.hz())).spawn(move || {
                let mut m = Monitor::new(b, &opts);
                let mut drain = |m: &mut Monitor| {
                    while let Some(l) = m.pop() {
                        if let Some(s) = heard.line(&l) {
                            emit(|| s);
                        }
                    }
                    heard.flush(false);
                };
                let mut report = std::time::Instant::now() + UNDECODED_REPORT;
                let mut last = (BURSTS.load(Ordering::SeqCst), DECODED.load(Ordering::SeqCst));
                for x in rx {
                    m.feed(&x);
                    drain(&mut m);
                    // The first decoder speaks for all of them, so a burst at another bandwidth
                    // that one of them decoded does not count as undecoded.
                    if i == 0 && std::time::Instant::now() >= report {
                        report += UNDECODED_REPORT;
                        let (b, ok) = (BURSTS.load(Ordering::SeqCst), DECODED.load(Ordering::SeqCst));
                        let (b, ok) = (b - last.0, ok - last.1);
                        last = (last.0 + b, last.1 + ok);
                        if b > 0 && ok == 0 {
                            emit(|| {
                                format!(
                                    "· heard {} VARA data bursts in {} min but decoded none: too weak or distorted, \
                                     or at a bandwidth not monitored\n",
                                    b / bws_n,
                                    UNDECODED_REPORT.as_secs() / 60
                                )
                            });
                        }
                    }
                }
                m.finish();
                drain(&mut m);
                heard.flush(true);
            })?;
            decoders.push(Decoder { bw: b, tx: Some(tx), worker: Some(worker), dropped: AtomicUsize::new(0) });
        }
        let hz: Vec<String> = bws.iter().map(|b| b.hz().to_string()).collect();
        emit(|| format!("— showing other stations' traffic ({} Hz) —\n", hz.join(", ")));
        Ok(Self { decoders })
    }

    /// Queues one block of live audio; a decoder that has fallen behind skips it.
    pub fn feed(&self, x: &[f64]) {
        for d in &self.decoders {
            let Some(tx) = &d.tx else { continue };
            if tx.try_send(x.to_vec()).is_err() {
                let n = d.dropped.fetch_add(1, Ordering::Relaxed);
                // Once, then about every 10 s of skipped audio.
                if n % 1000 == 0 {
                    crate::say!("passive decoder at {} Hz can't keep up; skipping audio", d.bw.hz());
                }
            }
        }
    }

    /// Queues one block, waiting for room: for input faster than real time, such as a file.
    pub fn feed_wait(&self, x: &[f64]) {
        for d in &self.decoders {
            if let Some(tx) = &d.tx {
                let _ = tx.send(x.to_vec());
            }
        }
    }
}

impl Drop for Passive {
    fn drop(&mut self) {
        for d in &mut self.decoders {
            d.tx.take();
        }
        for d in &mut self.decoders {
            if let Some(w) = d.worker.take() {
                let _ = w.join();
            }
        }
    }
}

/// How long an undecoded burst waits for a decoder at another bandwidth to decode it, before
/// it is shown as undecoded.
const UNDECODED_WAIT: std::time::Duration = std::time::Duration::from_secs(3);

/// Turns one passive decoder's records into readout lines: every frame it hears, as
/// xara-monitor prints them and tagged with its bandwidth, plus the text of data frames.
struct Heard {
    bw: Bandwidth,
    /// The first decoder also shows CQs (they decode at any bandwidth) and undecoded bursts.
    first: bool,
    /// Every frame, or only links, their text and CQs.
    frames: bool,
    winlink: bool,
    /// Undecoded bursts, with the decoded-frame count when heard: shown once no decoder has
    /// decoded anything since, so a burst one decoder read is not also shown as unread.
    pending: Vec<(std::time::Instant, usize, String)>,
}

impl Heard {
    fn line(&mut self, l: &Line) -> Option<String> {
        let own = OWN_LINK.load(Ordering::SeqCst);
        let (all, hz) = (self.frames, self.bw.hz());
        let tag = |t: &str| if all { format!("[{hz}] {}\n", t.trim_start()) } else { String::new() };
        match &l.rec {
            Record::Header { call, answerer, .. } if !own => {
                self.winlink = false;
                Some(format!("— heard {call} → {answerer} ({} Hz) —\n{}", self.bw.hz(), tag(&l.text)))
            }
            Record::Payload { body: Body::Data { data, .. }, .. } if !own => {
                let head = String::from_utf8_lossy(&data[..data.len().min(256)]);
                if head.contains("[WL2K") || head.contains("B2F") || head.lines().any(|l| l.starts_with("FC ")) {
                    self.winlink = true;
                }
                Some(tag(&l.text) + &render(data, self.winlink))
            }
            Record::LinkEnd { caller, answerer, why, .. } if !own => {
                Some(format!("— {caller} → {answerer} ended ({why}) —\n"))
            }
            Record::Cq { line, .. } if self.first => Some(format!("{line}\n")),
            Record::Cq { .. } => None,
            Record::Data { frames, .. } => {
                // Every decoder sees every burst; only the one at its bandwidth decodes frames ('+').
                let ok = frames.chars().filter(|&c| c == '+').count();
                BURSTS.fetch_add(1, Ordering::SeqCst);
                DECODED.fetch_add(ok, Ordering::SeqCst);
                match (ok, own, self.first) {
                    _ if !all => None,
                    (0, false, true) => {
                        self.pending.push((std::time::Instant::now(), DECODED.load(Ordering::SeqCst), l.text.clone()));
                        None
                    }
                    (0, _, _) | (_, true, _) => None,
                    _ => Some(tag(&l.text)),
                }
            }
            Record::Summary(_) => None,
            _ if own || !all => None,
            _ => Some(tag(&l.text)),
        }
    }

    /// Shows the undecoded bursts that no decoder has read since; all of them at the end.
    fn flush(&mut self, all: bool) {
        let now = std::time::Instant::now();
        let decoded = DECODED.load(Ordering::SeqCst);
        let (due, wait): (Vec<_>, Vec<_>) =
            self.pending.drain(..).partition(|(t, _, _)| all || now.duration_since(*t) >= UNDECODED_WAIT);
        self.pending = wait;
        for (_, seen, text) in due {
            if decoded == seen {
                emit(|| format!("[?] {}  (not decoded)\n", text.trim_start()));
            }
        }
    }
}

fn emit(f: impl FnOnce() -> String) {
    if let Some(s) = sink() {
        s(&f());
    }
}

/// Share of characters that read as text once decoded: invalid UTF-8 and control codes don't.
fn text_share(d: &[u8]) -> f64 {
    let text = String::from_utf8_lossy(d);
    let (mut total, mut ok) = (0usize, 0usize);
    for c in text.chars() {
        total += 1;
        if c != '\u{fffd}' && (!c.is_control() || matches!(c, '\r' | '\n' | '\t')) {
            ok += 1;
        }
    }
    ok as f64 / total.max(1) as f64
}

/// Renders one chunk of session data.
pub fn render(d: &[u8], winlink: bool) -> String {
    if d.is_empty() {
        return String::new();
    }
    if text_share(d) < TEXT_SHARE {
        return if winlink {
            format!("[Winlink: {} bytes of compressed message data]\n", d.len())
        } else {
            format!("[{} bytes of binary data]\n", d.len())
        };
    }
    let mut s = printable(d);
    if s.ends_with('\r') {
        s.pop();
        s.push('\n');
    }
    s
}

/// The text in `d`, with CR as a line break and anything unprintable as a middle dot.
fn printable(d: &[u8]) -> String {
    String::from_utf8_lossy(d)
        .replace("\r\n", "\n")
        .chars()
        .map(|c| match c {
            '\r' => '\n',
            '\n' | '\t' => c,
            c if c.is_control() || c == '\u{fffd}' => '·',
            c => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_text_reads_as_text() {
        assert_eq!(render(b"hello from N1ABC\r", false), "hello from N1ABC\n");
        assert_eq!(render(b"line 1\r\nline 2\r", false), "line 1\nline 2\n");
        assert_eq!(render("73 de Zoë\r".as_bytes(), false), "73 de Zoë\n");
        assert_eq!(render(b"ring the bell\x07 now", false), "ring the bell· now");
    }

    #[test]
    fn binary_is_summarized_and_winlink_named() {
        let blob: Vec<u8> = (0..200u8).map(|i| i.wrapping_mul(37)).collect();
        assert_eq!(render(&blob, false), "[200 bytes of binary data]\n");
        assert_eq!(render(&blob, true), "[Winlink: 200 bytes of compressed message data]\n");
    }

    #[test]
    fn a_winlink_exchange_is_recognized_from_its_sid() {
        let got = Arc::new(Mutex::new(Vec::<String>::new()));
        let g = got.clone();
        set_sink(move |s| g.lock().unwrap().push(s.to_string()));
        let mut rx = RxText::default();
        rx.connected("N1ABC", "W1AW-10");
        rx.data(b"[WL2K-5.0-B2FWIHJM$]\rFC EM 12345 100 80 0\rF> 3A\r");
        rx.data(&[0x02, 0xc3, 0x00, 0x91, 0xfe, 0x10, 0x00, 0x7f, 0x88]);
        rx.disconnected();
        let got = got.lock().unwrap();
        assert_eq!(got[0], "— connected N1ABC → W1AW-10 —\n");
        assert!(got[1].starts_with("[WL2K-5.0-B2FWIHJM$]\nFC EM"), "{}", got[1]);
        assert_eq!(got[2], "[Winlink: 9 bytes of compressed message data]\n");
        assert_eq!(got[3], "— disconnected —\n");
    }
}
