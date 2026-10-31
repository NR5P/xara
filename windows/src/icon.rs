// The app icon, drawn rather than shipped as a file: a blue disc with a mast and radio waves.
// Shared by the window and tray icons (main.rs) and the .exe icon (build.rs includes it).

/// RGBA pixels of the icon at `size` x `size`, dimmed (grey) when `on` is false.
pub fn icon_rgba(size: u32, on: bool) -> Vec<u8> {
    let s = size as f32;
    let mut px = vec![0u8; (size * size * 4) as usize];
    let bg = if on { [31.0, 111.0, 235.0] } else { [120.0, 120.0, 120.0] };
    let (cx, cy) = (s / 2.0, s * 0.62);
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let d = ((fx - s / 2.0).powi(2) + (fy - s / 2.0).powi(2)).sqrt();
            let disc = (s / 2.0 - d).clamp(0.0, 1.0);
            if disc == 0.0 {
                continue;
            }
            // White: the mast, its tip, and two wave arcs above it.
            let r = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
            let above = fy < cy - s * 0.02 && (fx - cx).abs() < (cy - fy) * 1.1;
            let ring = |radius: f32| (s * 0.045 - (r - radius).abs()).clamp(0.0, 1.0);
            let waves = if above { ring(s * 0.2).max(ring(s * 0.34)) } else { 0.0 };
            let tip = (s * 0.075 - r).clamp(0.0, 1.0);
            let mast = if fy > cy && fy < s * 0.86 { (s * 0.04 - (fx - cx).abs()).clamp(0.0, 1.0) } else { 0.0 };
            let w = waves.max(tip).max(mast);
            let i = ((y * size + x) * 4) as usize;
            for c in 0..3 {
                px[i + c] = (bg[c] * (1.0 - w) + 255.0 * w) as u8;
            }
            px[i + 3] = (disc * 255.0) as u8;
        }
    }
    px
}
