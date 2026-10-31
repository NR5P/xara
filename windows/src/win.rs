//! What only Windows needs: the tray icon and its menu, showing and hiding the window from
//! it (the window does not draw while hidden, so this goes through Win32 directly), and
//! opening at sign-in (the HKCU Run key).

use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use eframe::egui;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegDeleteValueW, RegOpenKeyExW,
    RegQueryValueExW, RegSetValueExW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_RESTORE, SW_SHOW, SetForegroundWindow, ShowWindow};

use crate::settings::Settings;
use crate::{Modem, QUITTING, lock};

static HWND: AtomicIsize = AtomicIsize::new(0);

pub fn remember_window(frame: &eframe::Frame) -> bool {
    if let Ok(h) = frame.window_handle()
        && let RawWindowHandle::Win32(w) = h.as_raw()
    {
        HWND.store(w.hwnd.get(), Ordering::SeqCst);
        return true;
    }
    false
}

pub fn hide_window() {
    let h = HWND.load(Ordering::SeqCst);
    if h != 0 {
        // SAFETY: h is our own top-level window.
        unsafe { ShowWindow(h as _, SW_HIDE) };
    }
}

pub fn show_window(ctx: &egui::Context) {
    let h = HWND.load(Ordering::SeqCst);
    if h != 0 {
        // SAFETY: h is our own top-level window.
        unsafe {
            ShowWindow(h as _, SW_SHOW);
            ShowWindow(h as _, SW_RESTORE);
            SetForegroundWindow(h as _);
        }
    }
    ctx.request_repaint();
}

fn icon(on: bool) -> Option<Icon> {
    Icon::from_rgba(crate::icon::icon_rgba(32, on), 32, 32).ok()
}

pub fn set_tray_state(t: &TrayIcon, running: bool, status: &str) {
    let _ = t.set_icon(icon(running));
    let _ = t.set_tooltip(Some(format!("xara: {status}")));
}

/// The tray icon: a click shows the window; its menu starts and stops the modem, opens the
/// web UI and quits.
pub fn tray(ctx: egui::Context, modem: Arc<Mutex<Modem>>, settings: Arc<Mutex<Settings>>) -> Option<TrayIcon> {
    let show = MenuItem::new("Show xara", true, None);
    let toggle = MenuItem::new("Start / stop modem", true, None);
    let web = MenuItem::new("Open web UI in browser", true, None);
    let quit = MenuItem::new("Quit xara", true, None);
    let menu = Menu::new();
    menu.append_items(&[&show, &toggle, &web, &PredefinedMenuItem::separator(), &quit]).ok()?;
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip("xara: Stopped")
        .with_icon(icon(false)?)
        .build()
        .ok()?;

    let c = ctx.clone();
    TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
        if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
            show_window(&c);
        }
    }));
    let (show, toggle, web, quit) = (show.id().clone(), toggle.id().clone(), web.id().clone(), quit.id().clone());
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        let s = settings.lock().unwrap_or_else(PoisonError::into_inner).clone();
        if e.id == show {
            show_window(&ctx);
        } else if e.id == toggle {
            let mut m = lock(&modem);
            if m.running() { m.stop() } else { m.start(&s) }
            ctx.request_repaint();
        } else if e.id == web {
            let _ = open::that(s.web_url());
        } else if e.id == quit {
            QUITTING.store(true, Ordering::SeqCst);
            lock(&modem).stop();
            std::process::exit(0);
        }
    }));
    Some(tray)
}

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN_VALUE: &str = "xara";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn run_key(access: u32) -> Option<HKEY> {
    let mut h: HKEY = std::ptr::null_mut();
    // SAFETY: the key name is NUL-terminated and h receives the opened key.
    let r = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), 0, access, &mut h) };
    (r == 0).then_some(h)
}

pub fn login_item() -> bool {
    let Some(h) = run_key(KEY_READ) else { return false };
    let mut size = 0u32;
    // SAFETY: h is open; a null buffer asks only for the size.
    let r = unsafe {
        RegQueryValueExW(h, wide(RUN_VALUE).as_ptr(), std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut(), &mut size)
    };
    // SAFETY: h was opened above.
    unsafe { RegCloseKey(h) };
    r == 0
}

pub fn set_login_item(on: bool) {
    let Some(h) = run_key(KEY_SET_VALUE) else { return };
    let name = wide(RUN_VALUE);
    if on {
        if let Ok(exe) = std::env::current_exe() {
            let cmd = wide(&format!("\"{}\"", exe.display()));
            // SAFETY: h is open and cmd is a NUL-terminated wide string of the given byte length.
            unsafe { RegSetValueExW(h, name.as_ptr(), 0, REG_SZ, cmd.as_ptr().cast(), (cmd.len() * 2) as u32) };
        }
    } else {
        // SAFETY: h is open.
        unsafe { RegDeleteValueW(h, name.as_ptr()) };
    }
    // SAFETY: h was opened above.
    unsafe { RegCloseKey(h) };
}
