//! Shared Agentbolt artwork. Pure RGBA pixels, with no OS, GUI or image-library dependency.
//! The tray adds a count badge; the Desk and executable use the same unbadged amber bolt.

pub const SIZE: usize = 32;

const FILL: [u8; 3] = [0xFF, 0xC4, 0x2E];
const OUTLINE: [u8; 3] = [0x3A, 0x2A, 0x05];
const BADGE: [u8; 3] = [0xD1, 0x34, 0x38];
const WHITE: [u8; 3] = [0xFF, 0xFF, 0xFF];

/// The bolt, clockwise from the top-left of its top bar.
const BOLT: [(f32, f32); 7] = [
    (17.5, 1.0),
    (26.5, 1.0),
    (19.5, 12.0),
    (26.5, 12.0),
    (11.0, 31.0),
    (14.0, 19.0),
    (6.0, 19.0),
];

/// RGBA pixels, row by row. `badge` = the number of open asks (0 or None = no badge; above 9 shows "+").
pub fn rgba(badge: Option<u32>) -> Vec<u8> {
    let mut out = rgba_at(SIZE);
    if let Some(n) = badge.filter(|n| *n > 0) {
        draw_badge(&mut out, n);
    }
    out
}

/// Render at the requested size rather than scaling a small bitmap on high-DPI displays.
pub fn rgba_at(size: usize) -> Vec<u8> {
    assert!((1..=256).contains(&size));
    let mut out = vec![0u8; size * size * 4];
    let scale = SIZE as f32 / size as f32;
    const SS: usize = 4;
    for y in 0..size {
        for x in 0..size {
            let (mut fill, mut outline) = (0u32, 0u32);
            for sy in 0..SS {
                for sx in 0..SS {
                    let p = (
                        (x as f32 + (sx as f32 + 0.5) / SS as f32) * scale,
                        (y as f32 + (sy as f32 + 0.5) / SS as f32) * scale,
                    );
                    if inside(p, &BOLT) {
                        if edge_distance(p, &BOLT) < 1.0 {
                            outline += 1
                        } else {
                            fill += 1
                        }
                    } else if edge_distance(p, &BOLT) < 0.6 {
                        outline += 1;
                    }
                }
            }
            let total = (SS * SS) as u32;
            let covered = fill + outline;
            if covered > 0 {
                let mix = |c: usize| {
                    ((FILL[c] as u32 * fill + OUTLINE[c] as u32 * outline) / covered) as u8
                };
                let i = (y * size + x) * 4;
                out[i..i + 4].copy_from_slice(&[
                    mix(0),
                    mix(1),
                    mix(2),
                    (255 * covered / total) as u8,
                ]);
            }
        }
    }
    out
}

fn draw_badge(out: &mut [u8], n: u32) {
    let (cx, cy, r) = (24.0f32, 24.0f32, 7.5f32);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            let color = if d <= r - 1.0 {
                BADGE
            } else if d <= r {
                WHITE
            } else {
                continue;
            };
            let i = (y * SIZE + x) * 4;
            out[i..i + 4].copy_from_slice(&[color[0], color[1], color[2], 255]);
        }
    }
    let glyph = glyph(n);
    // 3x5 glyph at 2x: 6x10 pixels, centred on the badge.
    let (left, top) = (21usize, 19usize);
    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..3 {
            if bits & (0b100 >> col) != 0 {
                for dy in 0..2 {
                    for dx in 0..2 {
                        let (x, y) = (left + col * 2 + dx, top + row * 2 + dy);
                        let i = (y * SIZE + x) * 4;
                        out[i..i + 4].copy_from_slice(&[WHITE[0], WHITE[1], WHITE[2], 255]);
                    }
                }
            }
        }
    }
}

fn glyph(n: u32) -> [u8; 5] {
    match n {
        1 => [0b010, 0b110, 0b010, 0b010, 0b111],
        2 => [0b111, 0b001, 0b111, 0b100, 0b111],
        3 => [0b111, 0b001, 0b111, 0b001, 0b111],
        4 => [0b101, 0b101, 0b111, 0b001, 0b001],
        5 => [0b111, 0b100, 0b111, 0b001, 0b111],
        6 => [0b111, 0b100, 0b111, 0b101, 0b111],
        7 => [0b111, 0b001, 0b001, 0b001, 0b001],
        8 => [0b111, 0b101, 0b111, 0b101, 0b111],
        9 => [0b111, 0b101, 0b111, 0b001, 0b111],
        _ => [0b000, 0b010, 0b111, 0b010, 0b000],
    }
}

fn inside(p: (f32, f32), poly: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < (b.0 - a.0) * (p.1 - a.1) / (b.1 - a.1) + a.0 {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn edge_distance(p: (f32, f32), poly: &[(f32, f32)]) -> f32 {
    let mut best = f32::MAX;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
        let (qx, qy) = (a.0 + t * dx, a.1 + t * dy);
        best = best.min(((p.0 - qx).powi(2) + (p.1 - qy).powi(2)).sqrt());
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha(px: &[u8], x: usize, y: usize) -> u8 {
        px[(y * SIZE + x) * 4 + 3]
    }

    #[test]
    fn the_bolt_is_drawn_with_transparent_corners() {
        let px = rgba(None);
        assert_eq!(px.len(), SIZE * SIZE * 4);
        assert_eq!(alpha(&px, 0, 0), 0);
        assert_eq!(alpha(&px, 31, 31), 0);
        assert_eq!(alpha(&px, 0, 31), 0);
        // Inside the top bar and the lower stroke.
        assert_eq!(alpha(&px, 21, 5), 255);
        assert_eq!(&px[(5 * SIZE + 21) * 4..(5 * SIZE + 21) * 4 + 3], &FILL);
        assert_eq!(alpha(&px, 15, 21), 255);
        let opaque = px.chunks(4).filter(|p| p[3] > 128).count();
        assert!(
            (150..450).contains(&opaque),
            "a bolt, not a block: {opaque} px"
        );
        if let Ok(path) = std::env::var("AGENTBOLT_ICON_DUMP") {
            std::fs::write(&path, &px).unwrap();
            std::fs::write(format!("{path}.badge"), rgba(Some(3))).unwrap();
        }
    }

    #[test]
    fn the_badge_shows_only_with_open_asks() {
        assert_eq!(rgba(Some(0)), rgba(None));
        let two = rgba(Some(2));
        assert_ne!(two, rgba(None));
        assert_ne!(two, rgba(Some(3)), "each count draws its own digit");
        assert_eq!(rgba(Some(12)), rgba(Some(40)), "above 9 it's a plus");
        assert_eq!(
            &two[(17 * SIZE + 24) * 4..(17 * SIZE + 24) * 4 + 4],
            &[0xFF, 0xFF, 0xFF, 255],
            "white rim"
        );
    }
}
