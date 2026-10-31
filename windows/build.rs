//! Writes the app icon as an .ico and embeds it, with the version info, in xara-app.exe.

use std::io::Write;
use std::path::Path;

mod icon {
    include!("src/icon.rs");
}

/// An .ico holding 32-bit BMP images at each size.
fn ico(sizes: &[u32]) -> Vec<u8> {
    let images: Vec<Vec<u8>> = sizes
        .iter()
        .map(|&n| {
            let rgba = icon::icon_rgba(n, true);
            let mut b = Vec::new();
            // BITMAPINFOHEADER: the height counts the colour rows and the AND mask rows.
            for v in [40u32, n, n * 2] {
                b.extend(v.to_le_bytes());
            }
            b.extend(1u16.to_le_bytes());
            b.extend(32u16.to_le_bytes());
            b.extend([0u8; 24]);
            for y in (0..n).rev() {
                for x in 0..n {
                    let i = ((y * n + x) * 4) as usize;
                    b.extend([rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
                }
            }
            let mask_row = n.div_ceil(32) * 4;
            b.extend(vec![0u8; (mask_row * n) as usize]);
            b
        })
        .collect();
    let mut out = Vec::new();
    out.extend([0u8, 0, 1, 0]);
    out.extend((sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (&n, img) in sizes.iter().zip(&images) {
        let dim = if n >= 256 { 0 } else { n as u8 };
        out.extend([dim, dim, 0, 0]);
        out.extend(1u16.to_le_bytes());
        out.extend(32u16.to_le_bytes());
        out.extend((img.len() as u32).to_le_bytes());
        out.extend(offset.to_le_bytes());
        offset += img.len() as u32;
    }
    for img in images {
        out.extend(img);
    }
    out
}

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=src/icon.rs");
    println!("cargo::rerun-if-changed=xara.rc");
    let out = std::env::var("OUT_DIR").unwrap();
    let path = Path::new(&out).join("xara.ico");
    std::fs::File::create(&path).unwrap().write_all(&ico(&[16, 24, 32, 48, 64, 256])).unwrap();
    let rc = Path::new(&out).join("app.rc");
    let template = std::fs::read_to_string("xara.rc").unwrap();
    let ico_path = path.to_string_lossy().replace('\\', "\\\\");
    std::fs::write(&rc, template.replace("@ICON@", &ico_path).replace("@VERSION@", env!("CARGO_PKG_VERSION")))
        .unwrap();
    embed_resource::compile(&rc, embed_resource::NONE).manifest_required().unwrap();
}
