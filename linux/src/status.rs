//! The modem's live status for the window: the same `status` messages the web UI gets on
//! its websocket (`/ws`), read here by a small client so the Linux app shows the state,
//! speed level, bitrate, S/N and levels itself (the Mac app shows the web UI in its window
//! instead; WebKit isn't something to depend on here).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// One `{"t":"status",…}` message, the parts the window shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub state: String,
    pub peer: String,
    pub bw: String,
    pub level: Option<u64>,
    pub bps: Option<u64>,
    pub tx: bool,
    pub modulation: String,
    pub sn: Option<f64>,
    pub afc: Option<f64>,
    pub attempts: u64,
    pub buffer: u64,
    pub ptt: bool,
    pub busy: bool,
    pub rx_rms: f64,
    pub tx_rms: f64,
    pub clip: bool,
    /// A tune carrier is on, for `tune_left` more seconds
    pub tuning: bool,
    pub tune_left: f64,
    /// The TX drive the modem uses now (dB)
    pub drive: Option<f64>,
}

impl Status {
    /// A status message's JSON; None for any other message.
    pub fn parse(text: &str) -> Option<Self> {
        let v: serde_json::Value = serde_json::from_str(text).ok()?;
        if v.get("t")?.as_str()? != "status" {
            return None;
        }
        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string();
        let n = |k: &str| v.get(k).and_then(serde_json::Value::as_f64);
        let b = |k: &str| v.get(k).and_then(serde_json::Value::as_bool).unwrap_or(false);
        let rate = v.get("bitrate");
        let meter = |k: &str, f: &str| v.get(k).and_then(|m| m.get(f)).and_then(serde_json::Value::as_f64);
        Some(Self {
            state: s("state"),
            peer: s("peer"),
            bw: s("bw"),
            level: v.get("level").and_then(serde_json::Value::as_u64).filter(|_| rate.is_some_and(|r| !r.is_null())),
            bps: rate.and_then(|r| r.get("bps")).and_then(serde_json::Value::as_u64),
            tx: rate.and_then(|r| r.get("tx")).and_then(serde_json::Value::as_bool).unwrap_or(false),
            modulation: rate.and_then(|r| r.get("modulation")).and_then(|m| m.as_str()).unwrap_or_default().to_string(),
            sn: n("sn"),
            afc: n("afc"),
            attempts: v.get("attempts").and_then(serde_json::Value::as_u64).unwrap_or(0),
            buffer: v.get("buffer").and_then(serde_json::Value::as_u64).unwrap_or(0),
            ptt: b("ptt"),
            busy: b("busy"),
            rx_rms: meter("rx", "rms").unwrap_or(-120.0),
            tx_rms: meter("tx", "rms").unwrap_or(-120.0),
            clip: meter("rx", "clip").is_some_and(|c| c > 0.0),
            tuning: b("tuning"),
            tune_left: n("tune_left").unwrap_or(0.0),
            drive: n("drive"),
        })
    }
}

/// The latest status and when it came; None while the web UI isn't reachable.
pub type Latest = Arc<Mutex<Option<(Instant, Status)>>>;

/// The web UI port the reader connects to (changed by the settings).
pub static PORT: AtomicU16 = AtomicU16::new(8080);

/// Keep reading the web UI's status, reconnecting while the modem starts and stops.
pub fn spawn(latest: Latest, repaint: impl Fn() + Send + 'static) {
    std::thread::Builder::new()
        .name("status".into())
        .spawn(move || {
            loop {
                let _ = follow(&latest, &repaint);
                *latest.lock().unwrap_or_else(PoisonError::into_inner) = None;
                repaint();
                std::thread::sleep(Duration::from_secs(1));
            }
        })
        .ok();
}

/// One websocket session: the upgrade, then frames until it closes.
fn follow(latest: &Latest, repaint: &impl Fn()) -> std::io::Result<()> {
    let port = PORT.load(Ordering::Relaxed);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(500))?;
    s.set_read_timeout(Some(Duration::from_secs(10)))?;
    write!(
        s,
        "GET /ws HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Key: eGFyYS1saW51eC1hcHA9PQ==\r\nSec-WebSocket-Version: 13\r\n\r\n"
    )?;
    let mut r = BufReader::new(s.try_clone()?);
    let mut line = String::new();
    r.read_line(&mut line)?;
    if !line.contains(" 101 ") {
        return Err(std::io::Error::other(format!("no websocket: {}", line.trim())));
    }
    loop {
        line.clear();
        if r.read_line(&mut line)? == 0 || line == "\r\n" {
            break;
        }
    }
    let mut message = Vec::new();
    loop {
        let (fin, op, payload) = frame(&mut r)?;
        match op {
            // text (and its continuations)
            0 | 1 => {
                message.extend_from_slice(&payload);
                if fin {
                    if let Some(st) = std::str::from_utf8(&message).ok().and_then(Status::parse) {
                        *latest.lock().unwrap_or_else(PoisonError::into_inner) = Some((Instant::now(), st));
                        repaint();
                    }
                    message.clear();
                }
            }
            8 => return Ok(()),
            // ping: a client's frames are masked (a zero mask leaves the bytes as they are)
            9 => {
                let mut pong = vec![0x8A, 0x80 | payload.len().min(125) as u8, 0, 0, 0, 0];
                pong.extend_from_slice(&payload[..payload.len().min(125)]);
                s.write_all(&pong)?;
            }
            // binary: the waterfall rows
            _ => {}
        }
    }
}

/// One frame from the server (unmasked): FIN, opcode, payload.
fn frame(r: &mut impl Read) -> std::io::Result<(bool, u8, Vec<u8>)> {
    let mut h = [0u8; 2];
    r.read_exact(&mut h)?;
    let (fin, op, masked) = (h[0] & 0x80 != 0, h[0] & 0x0F, h[1] & 0x80 != 0);
    let len = match h[1] & 0x7F {
        126 => {
            let mut b = [0u8; 2];
            r.read_exact(&mut b)?;
            u16::from_be_bytes(b) as u64
        }
        127 => {
            let mut b = [0u8; 8];
            r.read_exact(&mut b)?;
            u64::from_be_bytes(b)
        }
        n => n as u64,
    };
    if len > 16 << 20 {
        return Err(std::io::Error::other("websocket frame too long"));
    }
    let mut mask = [0u8; 4];
    if masked {
        r.read_exact(&mut mask)?;
    }
    let mut payload = vec![0u8; len as usize];
    r.read_exact(&mut payload)?;
    if masked {
        for (i, b) in payload.iter_mut().enumerate() {
            *b ^= mask[i % 4];
        }
    }
    Ok((fin, op, payload))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_web_uis_status() {
        // (the message xara-ui's own test checks)
        let m = r#"{"t":"status","ts":1.5,"rx":{"rms":-6.02,"peak":-6.02,"clip":0},"tx":{"rms":-120,"peak":-120,"clip":0},"ptt":true,"busy":false,"tuning":false,"tune_left":0,"drive":0,"state":"Receiving","level":7,"peer":"N1CALL","bw":"2300","passband":[350,2650],"attempts":0,"retries":0,"afc":null,"sn":11.25,"bitrate":{"level":7,"bps":2412,"tx":false,"modulation":"OFDM 49 carriers · QPSK"},"buffer":0,"calls":["N0CALL"],"dropped":0}"#;
        let s = Status::parse(m).unwrap();
        assert_eq!((s.state.as_str(), s.peer.as_str(), s.bw.as_str()), ("Receiving", "N1CALL", "2300"));
        assert_eq!((s.level, s.bps, s.sn, s.afc), (Some(7), Some(2412), Some(11.25), None));
        assert_eq!(s.modulation, "OFDM 49 carriers · QPSK");
        assert!(s.ptt && !s.busy && !s.clip && !s.tuning);
        assert_eq!(s.drive, Some(0.0));
        assert!(Status::parse(r#"{"t":"notice","ts":0,"kind":"timed_out"}"#).is_none());
    }

    #[test]
    fn frames() {
        // short text, then a 16-bit length
        let mut b = vec![0x81, 3];
        b.extend_from_slice(b"abc");
        let long = vec![b'x'; 300];
        b.extend_from_slice(&[0x81, 126, 1, 44]);
        b.extend_from_slice(&long);
        let mut r = &b[..];
        assert_eq!(frame(&mut r).unwrap(), (true, 1, b"abc".to_vec()));
        assert_eq!(frame(&mut r).unwrap(), (true, 1, long));
    }
}
