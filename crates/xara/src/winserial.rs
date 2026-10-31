//! Serial ports on Windows: COM ports for RTS/DTR PTT and the (tr)uSDX's audio over CAT.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::io::AsRawHandle;

use windows_sys::Win32::Devices::Communication::{
    CLRDTR, CLRRTS, COMMTIMEOUTS, DCB, EscapeCommFunction, GetCommState, NOPARITY, ONESTOPBIT, SETDTR, SETRTS,
    SetCommState, SetCommTimeouts,
};
use windows_sys::Win32::Foundation::{ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RegCloseKey, RegEnumValueW, RegOpenKeyExW};

/// A port name as the user writes it (`COM3`, `com12`) or a full `\\.\COM12` path.
pub fn is_port(name: &str) -> bool {
    let n = name.strip_prefix(r"\\.\").unwrap_or(name);
    n.len() > 3 && n[..3].eq_ignore_ascii_case("com") && n[3..].bytes().all(|b| b.is_ascii_digit())
}

pub fn open(name: &str) -> io::Result<File> {
    if !is_port(name) {
        return Err(io::Error::other(format!("{name:?} is not a serial port name such as COM3")));
    }
    let path = if name.starts_with(r"\\.\") { name.to_string() } else { format!(r"\\.\{name}") };
    let f = OpenOptions::new().read(true).write(true).open(&path)?;
    let mut dcb: DCB = unsafe { std::mem::zeroed() };
    dcb.DCBlength = size_of::<DCB>() as u32;
    // SAFETY: f is an open handle and dcb is a DCB of the right length.
    if unsafe { GetCommState(f.as_raw_handle(), &mut dcb) } == 0 {
        return Err(io::Error::other(format!("{name} is not a serial port: {}", io::Error::last_os_error())));
    }
    Ok(f)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Line {
    Rts,
    Dtr,
}

pub fn set_line(f: &File, line: Line, on: bool) -> io::Result<()> {
    let func = match (line, on) {
        (Line::Rts, true) => SETRTS,
        (Line::Rts, false) => CLRRTS,
        (Line::Dtr, true) => SETDTR,
        (Line::Dtr, false) => CLRDTR,
    };
    // SAFETY: f is an open serial port handle.
    if unsafe { EscapeCommFunction(f.as_raw_handle(), func) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Raw 8N1 at `baud`, DTR on and RTS off, with reads that return within `read_ms` when no
/// data comes (and at once when some has).
pub fn configure_raw(f: &File, baud: u32, read_ms: u32) -> io::Result<()> {
    let h = f.as_raw_handle();
    let mut dcb: DCB = unsafe { std::mem::zeroed() };
    dcb.DCBlength = size_of::<DCB>() as u32;
    dcb.BaudRate = baud;
    // fBinary, and fDtrControl = DTR_CONTROL_ENABLE; RTS_CONTROL_DISABLE and no flow control.
    dcb._bitfield = 1 | (1 << 4);
    dcb.ByteSize = 8;
    dcb.Parity = NOPARITY;
    dcb.StopBits = ONESTOPBIT;
    let t = COMMTIMEOUTS {
        ReadIntervalTimeout: u32::MAX,
        ReadTotalTimeoutMultiplier: u32::MAX,
        ReadTotalTimeoutConstant: read_ms,
        WriteTotalTimeoutMultiplier: 0,
        WriteTotalTimeoutConstant: 0,
    };
    // SAFETY: h is an open serial port handle; dcb and t are fully initialised.
    if unsafe { SetCommState(h, &dcb) } == 0 || unsafe { SetCommTimeouts(h, &t) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// The COM ports Windows knows of, from HKLM\HARDWARE\DEVICEMAP\SERIALCOMM.
pub fn ports() -> Vec<String> {
    let key: Vec<u16> = "HARDWARE\\DEVICEMAP\\SERIALCOMM\0".encode_utf16().collect();
    let mut h: HKEY = std::ptr::null_mut();
    // SAFETY: key is NUL-terminated and h receives the opened key.
    if unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, key.as_ptr(), 0, KEY_READ, &mut h) } != ERROR_SUCCESS {
        return Vec::new();
    }
    let mut out = Vec::new();
    for i in 0.. {
        let (mut name, mut data) = ([0u16; 256], [0u16; 64]);
        let (mut nlen, mut dlen) = (name.len() as u32, (data.len() * 2) as u32);
        // SAFETY: the buffers and their lengths match; h is open.
        let r = unsafe {
            RegEnumValueW(
                h,
                i,
                name.as_mut_ptr(),
                &mut nlen,
                std::ptr::null(),
                std::ptr::null_mut(),
                data.as_mut_ptr().cast(),
                &mut dlen,
            )
        };
        if r == ERROR_NO_MORE_ITEMS || r != ERROR_SUCCESS {
            break;
        }
        let d = &data[..(dlen as usize / 2)];
        let port = String::from_utf16_lossy(d).trim_end_matches('\0').to_string();
        if !port.is_empty() {
            out.push(port);
        }
    }
    // SAFETY: h was opened above.
    unsafe { RegCloseKey(h) };
    out.sort_by_key(|p| p[3..].parse::<u32>().unwrap_or(0));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_names() {
        for ok in ["COM1", "com12", r"\\.\COM12"] {
            assert!(is_port(ok), "{ok}");
        }
        for bad in ["", "COM", "COMx", "/dev/ttyUSB0", "LPT1"] {
            assert!(!is_port(bad), "{bad}");
        }
    }
}
