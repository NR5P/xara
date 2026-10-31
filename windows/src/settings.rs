//! The app's saved settings (%APPDATA%\xara\app.json), turned into xara's options.

use std::net::{SocketAddr, TcpStream, UdpSocket};
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

pub const MODES: [(&str, &str); 5] = [
    ("hf500", "HF 500 Hz"),
    ("hf2300", "HF 2300 Hz"),
    ("hf2750", "HF 2750 Hz"),
    ("fm-narrow", "FM narrow"),
    ("fm-wide", "FM wide"),
];

/// CM108 GPIO PTT is HID by device path or USB id, which xara does not do on Windows yet.
pub const PTTS: [(&str, &str); 4] =
    [("vox", "VOX"), ("rts", "Serial RTS"), ("dtr", "Serial DTR"), ("rigctld", "rigctld (hamlib CAT)")];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub call: String,
    pub mode: String,
    /// WASAPI endpoint names (or "default" for capture).
    pub capture: String,
    pub playback: String,
    pub radio: bool,
    pub ptt: String,
    pub serial: String,
    pub rigctld: String,
    pub drive_db: f32,
    pub lead_ms: u32,
    pub tail_ms: u32,
    pub port: u16,
    pub kiss_port: u16,
    pub ui_port: u16,
    pub lan: bool,
    pub ui_control: bool,
    pub monitor: bool,
    pub monitor_calls: String,
    pub monitor_frames: bool,
    pub auto_start: bool,
    /// Registered VARA speeds, for a user with a VARA licence.
    pub registered: bool,
    /// Delay in the audio path that xara does not add (a network audio link).
    pub audio_latency_ms: u32,
    pub debug_log: bool,
    pub record_audio: bool,
    /// Kept across launches, so a bookmarked controlling web UI keeps working.
    pub ui_token: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            call: String::new(),
            mode: "hf2300".into(),
            capture: String::new(),
            playback: String::new(),
            radio: false,
            ptt: "vox".into(),
            serial: String::new(),
            rigctld: "127.0.0.1:4532".into(),
            drive_db: -5.0,
            lead_ms: 0,
            tail_ms: 100,
            port: 8300,
            kiss_port: 8100,
            ui_port: 8080,
            lan: false,
            ui_control: false,
            monitor: false,
            monitor_calls: String::new(),
            monitor_frames: false,
            auto_start: false,
            registered: false,
            audio_latency_ms: 0,
            debug_log: false,
            record_audio: false,
            ui_token: String::new(),
        }
    }
}

fn app_dir() -> PathBuf {
    std::env::var_os("APPDATA").map_or_else(|| PathBuf::from("."), PathBuf::from).join("xara")
}

/// The web UI's settings and token.
pub fn state_dir() -> PathBuf {
    app_dir().join("state")
}

/// The debug log and recordings.
pub fn logs_dir() -> PathBuf {
    let d = std::env::var_os("LOCALAPPDATA").map_or_else(app_dir, |d| PathBuf::from(d).join("xara")).join("logs");
    let _ = std::fs::create_dir_all(&d);
    d
}

impl Settings {
    fn path() -> PathBuf {
        app_dir().join("app.json")
    }

    pub fn load() -> Self {
        let mut s: Self =
            std::fs::read_to_string(Self::path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        if s.ui_token.len() < 16 {
            s.ui_token = new_token();
            let _ = s.save();
        }
        s
    }

    pub fn save(&self) -> Result<(), String> {
        std::fs::create_dir_all(app_dir()).map_err(|e| e.to_string())?;
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(Self::path(), text).map_err(|e| e.to_string())
    }

    pub fn call_valid(&self) -> bool {
        let c = self.call.to_uppercase();
        let (base, ssid) = c.split_once('-').map_or((c.as_str(), None), |(b, s)| (b, Some(s)));
        (3..=7).contains(&base.len())
            && base.bytes().all(|b| b.is_ascii_alphanumeric())
            && ssid.is_none_or(|s| (1..=2).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit()))
            && !c.starts_with("N0CALL")
    }

    pub fn is_hf(&self) -> bool {
        self.mode.starts_with("hf")
    }

    pub fn mode_label(&self) -> &'static str {
        MODES.iter().find(|(k, _)| *k == self.mode).map_or("HF 2300 Hz", |(_, l)| l)
    }

    fn ptt_option(&self) -> String {
        match self.ptt.as_str() {
            "rts" | "dtr" => format!("{}:{}", self.ptt, self.serial),
            "rigctld" => format!("rigctld:{}", self.rigctld),
            _ => "vox".into(),
        }
    }

    pub fn arguments(&self) -> Vec<String> {
        let bind = if self.lan { "0.0.0.0" } else { "127.0.0.1" };
        let mut a: Vec<String> = vec!["--call".into(), self.call.to_uppercase()];
        a.extend(
            match self.mode.as_str() {
                "hf500" => ["--hf", "500"],
                "hf2750" => ["--hf", "2750"],
                "fm-narrow" => ["--fm", "narrow"],
                "fm-wide" => ["--fm", "wide"],
                _ => ["--hf", "2300"],
            }
            .map(String::from),
        );
        let capture = if self.capture.is_empty() { "default" } else { &self.capture };
        a.extend(["--audio", "wasapi", "--capture", capture, "--playback", &self.playback].map(String::from));
        if self.radio {
            a.extend(["--radio".into(), "--ptt".into(), self.ptt_option()]);
            a.extend(["--ptt-lead-ms".into(), self.lead_ms.to_string(), "--ptt-tail-ms".into(), self.tail_ms.to_string()]);
        }
        a.extend(["--drive-db".into(), format!("{:.1}", self.drive_db)]);
        a.extend(["--addr".into(), bind.into(), "--port".into(), self.port.to_string()]);
        a.extend(["--kiss-port".into(), self.kiss_port.to_string()]);
        a.extend(["--ui-port".into(), self.ui_port.to_string(), "--ui-addr".into(), bind.into()]);
        if self.ui_control {
            a.extend(["--ui-control".into(), "--ui-token".into(), self.ui_token.clone()]);
        }
        if self.monitor && self.is_hf() {
            a.push("--monitor".into());
            if self.monitor_frames {
                a.push("--monitor-frames".into());
            }
            for c in self.monitor_calls.to_uppercase().split([' ', ',']).filter(|c| !c.is_empty()) {
                a.extend(["--monitor-call".into(), c.into()]);
            }
        }
        if self.registered {
            a.push("--registered".into());
        }
        if self.audio_latency_ms > 0 {
            a.extend(["--audio-latency-ms".into(), self.audio_latency_ms.to_string()]);
        }
        if self.debug_log {
            a.extend(["--debug-log".into(), logs_dir().join("debug.log").to_string_lossy().into_owned()]);
        }
        if self.record_audio {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            let f = logs_dir().join(format!("xara-{stamp}.flac"));
            a.extend(["--record".into(), f.to_string_lossy().into_owned()]);
        }
        let _ = std::fs::create_dir_all(state_dir());
        a.extend(["--state-dir".into(), state_dir().to_string_lossy().into_owned()]);
        a
    }

    /// The equivalent `xara` command, for running it from a terminal instead.
    pub fn command_line(&self) -> String {
        std::iter::once("xara".to_string())
            .chain(self.arguments())
            .map(|a| if a.is_empty() || a.contains(' ') { format!("\"{a}\"") } else { a })
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn web_url(&self) -> String {
        let token = if self.ui_control { format!("?token={}", self.ui_token) } else { String::new() };
        format!("http://127.0.0.1:{}/{token}", self.ui_port)
    }
}

fn new_token() -> String {
    // Random enough for a local UI token: std's hasher keys come from the OS's random source.
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::new();
    while out.len() < 32 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_usize(out.len());
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out.truncate(32);
    out
}

/// True once something accepts connections on `port` on this PC.
pub fn port_open(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok()
}

/// This PC's address on the local network, for clients elsewhere (no packet is sent).
pub fn local_addresses() -> Vec<String> {
    UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("192.0.2.1:9")?;
            s.local_addr()
        })
        .map(|a| vec![a.ip().to_string()])
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_signs() {
        let ok = |c: &str| Settings { call: c.into(), ..Default::default() }.call_valid();
        assert!(ok("NR5P") && ok("w5tst-1") && ok("K1ABC-15"));
        assert!(!ok("") && !ok("AB") && !ok("N0CALL") && !ok("K1ABC-") && !ok("K1ABC-123") && !ok("K1 ABC"));
    }

    #[test]
    fn arguments_carry_the_settings() {
        let s = Settings {
            call: "nr5p".into(),
            capture: "CABLE Output (VB-Audio Virtual Cable)".into(),
            playback: "Hi-Fi Cable Input (VB-Audio Hi-Fi Cable)".into(),
            radio: true,
            ptt: "rts".into(),
            serial: "COM3".into(),
            ..Default::default()
        };
        let a = s.arguments();
        let has = |pair: [&str; 2]| a.windows(2).any(|w| w[0] == pair[0] && w[1] == pair[1]);
        assert!(has(["--call", "NR5P"]) && has(["--hf", "2300"]) && has(["--audio", "wasapi"]));
        assert!(has(["--ptt", "rts:COM3"]) && has(["--port", "8300"]) && has(["--addr", "127.0.0.1"]));
        assert!(s.command_line().contains("\"CABLE Output (VB-Audio Virtual Cable)\""));
    }
}
