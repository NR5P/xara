//! Linux desktop integration: the app's entry in the applications menu (with its icon,
//! which Wayland desktops take from the entry matching the window's app id, not from the
//! window), and "open xara when I log in" through the XDG autostart folder.

use std::path::PathBuf;

const APP_ID: &str = "xara-app";

fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from)
}

fn data_home() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME").map_or_else(|| home().join(".local/share"), PathBuf::from)
}

fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").map_or_else(|| home().join(".config"), PathBuf::from)
}

/// The desktop entry for this program, started with `extra` arguments.
fn entry(extra: &str) -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    Some(format!(
        "[Desktop Entry]\nType=Application\nName=xara\nGenericName=VARA-compatible modem\n\
         Comment=The xara modem for VARA clients such as VarAC, Openslot and Winlink\n\
         Keywords=ham;radio;VARA;modem;HF;FM;Winlink;VarAC;\nExec=\"{}\"{extra}\nIcon={APP_ID}\n\
         Terminal=false\nCategories=HamRadio;Network;\nStartupWMClass={APP_ID}\n",
        exe.display()
    ))
}

/// Put the menu entry and its icons in place (again when the program has moved).
pub fn install() {
    let Some(text) = entry("") else { return };
    let apps = data_home().join("applications");
    let file = apps.join(format!("{APP_ID}.desktop"));
    if std::fs::read_to_string(&file).is_ok_and(|t| t == text) {
        return;
    }
    for size in [16u32, 32, 48, 64, 128, 256] {
        let dir = data_home().join(format!("icons/hicolor/{size}x{size}/apps"));
        if std::fs::create_dir_all(&dir).is_ok() {
            let _ = std::fs::write(dir.join(format!("{APP_ID}.png")), png(size, &crate::icon::icon_rgba(size, true)));
        }
    }
    if std::fs::create_dir_all(&apps).is_ok() {
        let _ = std::fs::write(&file, text);
    }
}

fn autostart_file() -> PathBuf {
    config_home().join("autostart").join(format!("{APP_ID}.desktop"))
}

/// Does xara open at login?
pub fn login_item() -> bool {
    autostart_file().exists()
}

/// Open xara at login (with the modem started, as the user set it up), or not.
pub fn set_login_item(on: bool) {
    let f = autostart_file();
    if on {
        if let (Some(text), Some(dir)) = (entry(""), f.parent()) {
            let _ = std::fs::create_dir_all(dir);
            let _ = std::fs::write(&f, text);
        }
    } else {
        let _ = std::fs::remove_file(&f);
    }
}

/// An RGBA image as a PNG (uncompressed deflate blocks: the icons are small).
pub fn png(size: u32, rgba: &[u8]) -> Vec<u8> {
    fn crc32(data: &[u8]) -> u32 {
        let mut c = !0u32;
        for &b in data {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc32(&body).to_be_bytes());
    }
    let row = size as usize * 4;
    let mut raw = Vec::with_capacity((row + 1) * size as usize);
    for y in 0..size as usize {
        raw.push(0); // filter: none
        raw.extend_from_slice(&rgba[y * row..(y + 1) * row]);
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65_535).collect();
    for (i, b) in blocks.iter().enumerate() {
        z.push(u8::from(i + 1 == blocks.len()));
        z.extend_from_slice(&(b.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(b.len() as u16)).to_le_bytes());
        z.extend_from_slice(b);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + x as u32) % 65_521;
        b = (b + a) % 65_521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = size.to_be_bytes().to_vec();
    ihdr.extend_from_slice(&size.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_is_well_formed() {
        let p = png(16, &crate::icon::icon_rgba(16, true));
        assert_eq!(&p[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&p[12..16], b"IHDR");
        assert_eq!(u32::from_be_bytes(p[16..20].try_into().unwrap()), 16);
        assert!(p.ends_with(&[0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82]));
    }
}
