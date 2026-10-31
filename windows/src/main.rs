//! xara for Windows: the modem (crates/xara, through xara-ffi) with a window and a tray icon,
//! the Windows counterpart of the Mac app in ../macos.
//!
//! Tabs: Modem (settings, start/stop), Web UI, Received and Log. Closing the window leaves
//! the modem running in the tray; quit from the tray menu, which releases PTT first.

#![windows_subsystem = "windows"]

mod icon;
mod settings;
mod win;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use eframe::egui::{self, Color32, RichText};
use settings::{MODES, PTTS, Settings};

const LOG_LINES: usize = 5000;
const RECEIVED_LIMIT: usize = 500_000;

/// The modem and what it has said, shared by the window and the tray menu (which works while
/// the window is hidden and so not drawing).
struct Modem {
    handle: Option<xara_ffi::XaraModem>,
    status: String,
    log: VecDeque<String>,
    received: String,
}

impl Modem {
    fn running(&self) -> bool {
        self.handle.is_some()
    }

    fn start(&mut self, s: &Settings) {
        if self.handle.is_some() {
            return;
        }
        if !s.call_valid() {
            self.status = "Set your call sign first.".into();
            return;
        }
        if s.playback.is_empty() {
            self.status = "Choose the audio out first.".into();
            return;
        }
        if settings::port_open(s.port) {
            self.status = format!("Port {} is already in use (is VARA or another modem running?)", s.port);
            return;
        }
        let args = s.arguments();
        match xara_ffi::start(args) {
            Some(m) => {
                self.handle = Some(m);
                self.status = format!("Running {} as {}", s.mode_label(), s.call.to_uppercase());
            }
            None => self.status = "The modem is already running.".into(),
        }
    }

    /// Stops the modem and waits for it: it releases PTT and closes its ports first.
    fn stop(&mut self) {
        let Some(m) = self.handle.take() else { return };
        let code = m.stop();
        self.drain();
        self.status = if code == 0 { "Stopped".into() } else { format!("Stopped (exit {code}): {}", self.last_error()) };
    }

    fn last_error(&self) -> String {
        self.log.iter().rev().find_map(|l| l.strip_prefix("xara: ")).unwrap_or("see Log").to_string()
    }

    /// Takes in the modem's log and received text, and notices it stopping on its own.
    fn drain(&mut self) {
        while let Some(line) = xara_ffi::next_log() {
            if self.log.len() == LOG_LINES {
                self.log.pop_front();
            }
            self.log.push_back(line);
        }
        while let Some(piece) = xara_ffi::next_received() {
            self.received.push_str(&piece);
        }
        if self.received.len() > RECEIVED_LIMIT {
            let mut cut = self.received.len() - RECEIVED_LIMIT;
            while !self.received.is_char_boundary(cut) {
                cut += 1;
            }
            self.received.drain(..cut);
        }
        if self.handle.as_ref().is_some_and(|m| !m.running()) {
            let code = self.handle.take().map_or(0, |m| m.stop());
            self.status = format!("Stopped (exit {code}): {}", self.last_error());
        }
    }
}

type Shared = Arc<Mutex<Modem>>;

fn lock(m: &Shared) -> std::sync::MutexGuard<'_, Modem> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Modem,
    WebUi,
    Received,
    Log,
}

struct App {
    modem: Shared,
    settings: Arc<Mutex<Settings>>,
    tab: Tab,
    inputs: Vec<(String, String)>,
    outputs: Vec<(String, String)>,
    serial_ports: Vec<String>,
    login_item: bool,
    _tray: Option<tray_icon::TrayIcon>,
    tray_on: Option<bool>,
    hwnd_known: bool,
}

/// Set by the tray's Quit: the window lets itself close.
static QUITTING: AtomicBool = AtomicBool::new(false);

impl App {
    fn new(cc: &eframe::CreationContext<'_>, start: bool) -> Self {
        let settings = Arc::new(Mutex::new(Settings::load()));
        let modem: Shared = Arc::new(Mutex::new(Modem {
            handle: None,
            status: "Stopped".into(),
            log: VecDeque::new(),
            received: String::new(),
        }));
        let tray = win::tray(cc.egui_ctx.clone(), modem.clone(), settings.clone());
        let mut app = Self {
            modem,
            settings,
            tab: Tab::Modem,
            inputs: Vec::new(),
            outputs: Vec::new(),
            serial_ports: Vec::new(),
            login_item: win::login_item(),
            _tray: tray,
            tray_on: None,
            hwnd_known: false,
        };
        app.refresh_devices();
        app.pick_default_devices();
        let s = app.settings.lock().unwrap_or_else(PoisonError::into_inner).clone();
        if start || s.auto_start {
            lock(&app.modem).start(&s);
        }
        // Keep the log drained and the tray icon current while the window is hidden.
        let (ctx, modem) = (cc.egui_ctx.clone(), app.modem.clone());
        std::thread::Builder::new()
            .name("app-tick".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_millis(250));
                    lock(&modem).drain();
                    ctx.request_repaint();
                }
            })
            .ok();
        app
    }

    fn refresh_devices(&mut self) {
        match xara::audio::list(xara::audio::Backend::Wasapi) {
            Ok((i, o)) => {
                self.inputs = i;
                self.outputs = o;
            }
            Err(e) => lock(&self.modem).status = format!("Can't list audio devices: {e}"),
        }
        self.serial_ports = xara::audio::list(xara::audio::Backend::Trusdx)
            .map(|(p, _)| p.into_iter().map(|(n, _)| n).filter(|n| n != "app").collect())
            .unwrap_or_default();
    }

    fn virtual_out(desc: &str) -> bool {
        desc.ends_with("(virtual)")
    }

    /// Without radio mode xara only plays into a virtual device, so that is all it offers.
    fn offered_outputs(&self, radio: bool) -> Vec<(String, String)> {
        self.outputs.iter().filter(|(_, d)| radio || Self::virtual_out(d)).cloned().collect()
    }

    fn pick_default_devices(&mut self) {
        let mut s = self.settings.lock().unwrap_or_else(PoisonError::into_inner);
        let outs = self.offered_outputs(s.radio);
        if s.capture.is_empty() {
            // A virtual cable's output is the usual partner of a virtual playback device.
            s.capture = self
                .inputs
                .iter()
                .find(|(_, d)| Self::virtual_out(d))
                .or(self.inputs.first())
                .map_or("default".into(), |(n, _)| n.clone());
        }
        if s.playback.is_empty() || !outs.iter().any(|(n, _)| *n == s.playback) {
            s.playback = outs.first().map(|(n, _)| n.clone()).unwrap_or_default();
        }
    }

    fn modem_tab(&mut self, ui: &mut egui::Ui) {
        let running = lock(&self.modem).running();
        let status = lock(&self.modem).status.clone();
        let mut s = self.settings.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let before = s.clone();

        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.group(|ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let color = if running { Color32::from_rgb(60, 180, 75) } else { Color32::GRAY };
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 6.0, color);
                    ui.label(RichText::new(&status).size(15.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let label = if running { "Stop" } else { "Start" };
                        let ok = running || s.call_valid();
                        let b = egui::Button::new(RichText::new(label).size(16.0)).min_size(egui::vec2(90.0, 30.0));
                        if ui.add_enabled(ok, b).clicked() {
                            let mut m = lock(&self.modem);
                            if running { m.stop() } else { m.start(&s) }
                        }
                    });
                });
                if running {
                    ui.label(
                        RichText::new(format!(
                            "VARA command {}, data {}, KISS {}, web UI {}",
                            s.port,
                            s.port + 1,
                            s.kiss_port,
                            s.ui_port
                        ))
                        .weak(),
                    );
                }
            });

            ui.add_enabled_ui(!running, |ui| {
                section(ui, "Station", |ui| {
                    egui::Grid::new("station").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                        ui.label("Call sign");
                        if ui.add(egui::TextEdit::singleline(&mut s.call).desired_width(140.0)).changed() {
                            s.call = s.call.to_uppercase();
                        }
                        ui.end_row();
                        ui.label("Mode");
                        combo(ui, "mode", &mut s.mode, &MODES);
                        ui.end_row();
                        ui.label("TX drive");
                        ui.add(egui::Slider::new(&mut s.drive_db, -30.0..=0.0).step_by(1.0).suffix(" dB"));
                        ui.end_row();
                    });
                    ui.checkbox(&mut s.registered, "Registered (VARA licence)");
                    note(
                        ui,
                        "Only with a registered copy of VARA: higher speeds, and links at full speed with other registered stations.",
                    );
                });

                section(ui, "Audio and radio", |ui| {
                    egui::Grid::new("audio").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                        ui.label("Audio in");
                        let mut ins = vec![("default".to_string(), "System default".to_string())];
                        ins.extend(self.inputs.iter().cloned());
                        device_combo(ui, "in", &mut s.capture, &ins, "Choose…");
                        ui.end_row();
                        ui.label("Audio out");
                        let outs = self.offered_outputs(s.radio);
                        let empty = "No virtual device (install VB-CABLE)";
                        device_combo(ui, "out", &mut s.playback, &outs, if outs.is_empty() { empty } else { "Choose…" });
                        ui.end_row();
                    });
                    if ui.checkbox(&mut s.radio, "Radio mode: transmit through a real radio").changed() {
                        let outs = self.offered_outputs(s.radio);
                        if !outs.iter().any(|(n, _)| *n == s.playback) {
                            s.playback = outs.first().map(|(n, _)| n.clone()).unwrap_or_default();
                        }
                    }
                    if s.radio {
                        egui::Grid::new("ptt").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                            ui.label("PTT");
                            combo(ui, "ptt", &mut s.ptt, &PTTS);
                            ui.end_row();
                            match s.ptt.as_str() {
                                "rts" | "dtr" => {
                                    ui.label("Serial port");
                                    let ports: Vec<_> = self.serial_ports.iter().map(|p| (p.clone(), p.clone())).collect();
                                    let none = if ports.is_empty() { "No serial ports found" } else { "Choose…" };
                                    device_combo(ui, "serial", &mut s.serial, &ports, none);
                                    ui.end_row();
                                }
                                "rigctld" => {
                                    ui.label("rigctld host:port");
                                    ui.text_edit_singleline(&mut s.rigctld);
                                    ui.end_row();
                                }
                                _ => {}
                            }
                            ui.label("PTT lead");
                            ui.add(egui::DragValue::new(&mut s.lead_ms).range(0..=1000).speed(10).suffix(" ms"));
                            ui.end_row();
                            ui.label("PTT tail");
                            ui.add(egui::DragValue::new(&mut s.tail_ms).range(0..=1000).speed(10).suffix(" ms"));
                            ui.end_row();
                        });
                    }
                    ui.horizontal(|ui| {
                        ui.label("Audio delay");
                        ui.add(egui::DragValue::new(&mut s.audio_latency_ms).range(0..=1000).speed(50).suffix(" ms"));
                    });
                    note(
                        ui,
                        "Round-trip delay in the audio path that xara does not add itself, such as network audio. \
                         Leave at 0 for a radio on a USB interface.",
                    );
                    if ui.button("Refresh devices").clicked() {
                        self.refresh_devices();
                    }
                    note(
                        ui,
                        if s.radio {
                            "xara keys the radio and transmits into the chosen output. Pick your radio's USB audio interface."
                        } else {
                            "Without radio mode xara only plays into a virtual device such as VB-Audio's CABLE Input, \
                             so nothing reaches a transmitter."
                        },
                    );
                });

                section(ui, "Receive", |ui| {
                    let hf = s.is_hf();
                    ui.add_enabled(hf, egui::Checkbox::new(&mut s.monitor, "Show other stations' traffic"));
                    if s.monitor && hf {
                        ui.horizontal(|ui| {
                            ui.label("Show");
                            ui.selectable_value(&mut s.monitor_frames, false, "Text only");
                            ui.selectable_value(&mut s.monitor_frames, true, "Every packet");
                        });
                        ui.horizontal(|ui| {
                            ui.label("Calls to name (optional)");
                            ui.text_edit_singleline(&mut s.monitor_calls);
                        });
                    }
                    ui.checkbox(&mut s.record_audio, "Record received audio");
                    ui.checkbox(&mut s.debug_log, "Keep a debug log");
                    if (s.record_audio || s.debug_log) && ui.button("Show in Explorer").clicked() {
                        let _ = open::that(settings::logs_dir());
                    }
                    note(
                        ui,
                        &format!(
                            "{} Recording saves what the modem hears and sends (about 3 MB a minute); the debug log adds a \
                             timestamped line for every frame, host command and, once a second, the audio levels. Both go \
                             to {}.",
                            if hf {
                                "The Received tab always shows your own links. With this on it also decodes VARA HF traffic \
                                 between other stations, like VARA's monitor. Answering stations show as #hash unless their \
                                 call is listed, separated by spaces."
                            } else {
                                "Showing other stations' traffic is for VARA HF; FM shows your own links only."
                            },
                            settings::logs_dir().display()
                        ),
                    );
                });

                section(ui, "Clients", |ui| {
                    ui.checkbox(&mut s.lan, "Accept clients on the local network");
                    ui.checkbox(&mut s.ui_control, "Web UI can control the radio");
                    egui::Grid::new("ports").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                        ui.label("VARA command port (data is +1)");
                        ui.add(egui::DragValue::new(&mut s.port).range(1024..=65000));
                        ui.end_row();
                        ui.label("KISS port");
                        ui.add(egui::DragValue::new(&mut s.kiss_port).range(1024..=65535));
                        ui.end_row();
                        ui.label("Web UI port");
                        ui.add(egui::DragValue::new(&mut s.ui_port).range(1024..=65535));
                        ui.end_row();
                    });
                    note(ui, &clients_note(&s));
                });
            });

            section(ui, "App", |ui| {
                ui.checkbox(&mut s.auto_start, "Start the modem when xara opens");
                if ui.checkbox(&mut self.login_item, "Open xara when I sign in to Windows").changed() {
                    win::set_login_item(self.login_item);
                    self.login_item = win::login_item();
                }
                ui.horizontal(|ui| {
                    ui.label("Command line");
                    if ui.button("Copy").clicked() {
                        ui.ctx().copy_text(s.command_line());
                    }
                });
                ui.label(format!("Version: xara {}", xara_ffi_version()));
            });
        });

        if s != before {
            if let Err(e) = s.save() {
                lock(&self.modem).status = format!("Couldn't save settings: {e}");
            }
            *self.settings.lock().unwrap_or_else(PoisonError::into_inner) = s;
        }
    }

    fn web_tab(&mut self, ui: &mut egui::Ui) {
        let running = lock(&self.modem).running();
        let url = self.settings.lock().unwrap_or_else(PoisonError::into_inner).web_url();
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);
            if running {
                ui.heading("xara's web UI");
                ui.label("The waterfall, levels and status open in your browser.");
                ui.add_space(10.0);
                if ui.button(RichText::new("Open in browser").size(16.0)).clicked() {
                    let _ = open::that(&url);
                }
                ui.add_space(6.0);
                ui.hyperlink_to(&url, &url);
            } else {
                ui.heading("Modem stopped");
                ui.label("Start the modem to see its waterfall and status.");
            }
        });
    }

    fn text_tab(&mut self, ui: &mut egui::Ui, received: bool) {
        let mut m = lock(&self.modem);
        ui.horizontal(|ui| {
            if received && !m.received.is_empty() {
                ui.label(RichText::new(format!("{} characters", m.received.chars().count())).weak());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Clear").clicked() {
                    if received { m.received.clear() } else { m.log.clear() }
                }
                if ui.button("Copy").clicked() {
                    let text = if received { m.received.clone() } else { m.log.iter().cloned().collect::<Vec<_>>().join("\n") };
                    ui.ctx().copy_text(text);
                }
            });
        });
        ui.separator();
        egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink(false).show(ui, |ui| {
            if received {
                if m.received.is_empty() {
                    ui.label(RichText::new("Nothing received yet.").weak());
                } else {
                    ui.add(egui::Label::new(RichText::new(&m.received).monospace()).selectable(true));
                }
            } else {
                for line in &m.log {
                    ui.add(egui::Label::new(RichText::new(line).monospace().size(12.0)).selectable(true));
                }
            }
        });
    }
}

fn xara_ffi_version() -> &'static str {
    // SAFETY: xara_version returns a static NUL-terminated string.
    unsafe { std::ffi::CStr::from_ptr(xara_ffi::xara_version()) }.to_str().unwrap_or("?")
}

fn section(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(8.0);
    ui.label(RichText::new(title).strong().size(15.0));
    ui.group(|ui| {
        ui.set_width(ui.available_width());
        body(ui);
    });
}

fn note(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).weak().size(12.0));
}

fn combo(ui: &mut egui::Ui, id: &str, value: &mut String, choices: &[(&str, &str)]) {
    let shown = choices.iter().find(|(k, _)| k == value).map_or("", |(_, l)| l);
    egui::ComboBox::from_id_salt(id).selected_text(shown).show_ui(ui, |ui| {
        for (k, l) in choices {
            ui.selectable_value(value, k.to_string(), *l);
        }
    });
}

fn device_combo(ui: &mut egui::Ui, id: &str, value: &mut String, choices: &[(String, String)], none: &str) {
    let shown = choices
        .iter()
        .find(|(k, _)| k == value)
        .map_or_else(|| if value.is_empty() { none.to_string() } else { value.clone() }, |(_, l)| l.clone());
    egui::ComboBox::from_id_salt(id).width(360.0).selected_text(shown).show_ui(ui, |ui| {
        for (k, l) in choices {
            ui.selectable_value(value, k.clone(), l);
        }
    });
}

fn clients_note(s: &Settings) -> String {
    if !s.lan {
        return format!(
            "VarAC, Openslot, Winlink Express and other VARA clients on this PC connect to 127.0.0.1:{} (data {}). \
             Other programs can start the modem with: xara-app.exe --start",
            s.port,
            s.port + 1
        );
    }
    let hosts = settings::local_addresses();
    format!(
        "Clients elsewhere on the network connect to {}. Anyone on this network can use the modem. \
         Windows Firewall may ask to allow xara the first time.",
        if hosts.is_empty() { "this PC".to_string() } else { hosts.iter().map(|h| format!("{h}:{}", s.port)).collect::<Vec<_>>().join(", ") }
    )
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if !self.hwnd_known {
            self.hwnd_known = win::remember_window(frame);
        }
        lock(&self.modem).drain();
        let running = lock(&self.modem).running();
        if self.tray_on != Some(running) {
            self.tray_on = Some(running);
            if let Some(t) = &self._tray {
                win::set_tray_state(t, running, &lock(&self.modem).status);
            }
        }

        // Closing the window keeps the modem running in the tray, as the Mac app's menu bar does.
        if ctx.input(|i| i.viewport().close_requested()) && !QUITTING.load(Ordering::SeqCst) && self._tray.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            win::hide_window();
        }

        egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                for (tab, label) in [
                    (Tab::Modem, "📡 Modem"),
                    (Tab::WebUi, "〰 Web UI"),
                    (Tab::Received, "💬 Received"),
                    (Tab::Log, "☰ Log"),
                ] {
                    ui.selectable_value(&mut self.tab, tab, RichText::new(label).size(15.0));
                }
            });
            ui.add_space(2.0);
        });
        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::Modem => self.modem_tab(ui),
            Tab::WebUi => self.web_tab(ui),
            Tab::Received => self.text_tab(ui, true),
            Tab::Log => self.text_tab(ui, false),
        });
        ctx.request_repaint_after(Duration::from_millis(250));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Release PTT and close the ports before quitting.
        lock(&self.modem).stop();
    }
}

fn main() -> eframe::Result<()> {
    let start = std::env::args().any(|a| a == "--start" || a == "xara://start" || a == "xara://start/");
    let size = 64;
    let icon = egui::IconData { rgba: icon::icon_rgba(size, true), width: size, height: size };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("xara")
            .with_inner_size([760.0, 700.0])
            .with_min_inner_size([620.0, 560.0])
            .with_icon(Arc::new(icon)),
        ..Default::default()
    };
    eframe::run_native("xara", options, Box::new(move |cc| Ok(Box::new(App::new(cc, start)))))
}
