//! Procedural, license-clean test images.

use pastelplash::color;
use pastelplash::image::{Image, SourceColor, SourceFormat};

pub fn image(w: u32, h: u32, f: impl Fn(u32, u32) -> [f32; 4]) -> Image {
    Image {
        width: w,
        height: h,
        pixels: (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| f(x, y))
            .collect(),
        source: SourceFormat {
            color: SourceColor::Rgba,
            bit_depth: 8,
            has_alpha: true,
        },
        source_scale: None,
        tint_safe: None,
    }
}

/// Deterministic hash noise in 0..1.
pub fn noise(x: u32, y: u32, seed: u32) -> f32 {
    let mut v =
        x.wrapping_mul(0x9E37_79B9) ^ y.wrapping_mul(0x85EB_CA6B) ^ seed.wrapping_mul(0xC2B2_AE35);
    v ^= v >> 15;
    v = v.wrapping_mul(0x2C1B_3C6D);
    v ^= v >> 12;
    v = v.wrapping_mul(0x297A_2D39);
    v ^= v >> 15;
    (v & 0xFFFF) as f32 / 65535.0
}

/// Smooth periodic value noise (period = image size / cells), 0..1.
pub fn smooth_noise(x: f32, y: f32, cells: u32, size: u32, seed: u32) -> f32 {
    let fx = x / size as f32 * cells as f32;
    let fy = y / size as f32 * cells as f32;
    let (ix, iy) = (fx.floor() as i64, fy.floor() as i64);
    let (tx, ty) = (fx - ix as f32, fy - iy as f32);
    let (tx, ty) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
    let at = |i: i64, j: i64| {
        noise(
            i.rem_euclid(cells as i64) as u32,
            j.rem_euclid(cells as i64) as u32,
            seed,
        )
    };
    let a = at(ix, iy) + (at(ix + 1, iy) - at(ix, iy)) * tx;
    let b = at(ix, iy + 1) + (at(ix + 1, iy + 1) - at(ix, iy + 1)) * tx;
    a + (b - a) * ty
}

pub fn from_oklch(l: f32, c: f32, h: f32) -> [f32; 3] {
    color::oklch_to_srgb_gamut([l, c, h])
}

/// Dark, textured foliage: leaf blobs in dark and mid greens with gaps, tiling.
pub fn dark_foliage(size: u32, seed: u32) -> Image {
    image(size, size, |x, y| {
        let (fx, fy) = (x as f32, y as f32);
        let blob = smooth_noise(fx, fy, 8, size, seed);
        let detail = noise(x, y, seed + 1);
        let l = if blob > 0.55 {
            0.35 + 0.2 * detail
        } else {
            0.12 + 0.12 * detail
        };
        let h = 120.0 + 30.0 * smooth_noise(fx, fy, 4, size, seed + 2);
        let [r, g, b] = from_oklch(l, 0.09 + 0.04 * detail, h);
        [r, g, b, 1.0]
    })
}

/// Saturated darks of many hues (reds, blues, purples, browns) with noise.
pub fn dark_hues(size: u32, seed: u32) -> Image {
    image(size, size, |x, y| {
        let h = x as f32 / size as f32 * 360.0;
        let l = 0.1 + 0.5 * y as f32 / size as f32 + 0.05 * noise(x, y, seed);
        let [r, g, b] = from_oklch(l, 0.15, h);
        [r, g, b, 1.0]
    })
}

pub fn grayscale(size: u32, seed: u32) -> Image {
    image(size, size, |x, y| {
        let v =
            0.15 + 0.6 * smooth_noise(x as f32, y as f32, 6, size, seed) + 0.1 * noise(x, y, seed);
        [v, v, v, 1.0]
    })
}

/// Near-black neutrals with slight noise (crushed shadows).
pub fn grayscale_dark(size: u32, seed: u32) -> Image {
    image(size, size, |x, y| {
        let v =
            0.02 + 0.1 * smooth_noise(x as f32, y as f32, 6, size, seed) + 0.02 * noise(x, y, seed);
        // A faint warm cast keeps it out of tint-safe detection (real crushed shadows).
        let [r, g, b] = from_oklch(color::srgb_to_oklab([v, v, v])[0], 0.05, 60.0);
        [r, g, b, 1.0]
    })
}

/// Dull dark browns: dirt and grime, the raw material of "mud".
pub fn dull_browns(size: u32, seed: u32) -> Image {
    image(size, size, |x, y| {
        let l =
            0.18 + 0.2 * smooth_noise(x as f32, y as f32, 5, size, seed) + 0.04 * noise(x, y, seed);
        let [r, g, b] = from_oklch(
            l,
            0.03 + 0.03 * noise(x, y, seed + 1),
            55.0 + 30.0 * noise(x, y, seed + 2),
        );
        [r, g, b, 1.0]
    })
}

/// Mid-value foliage (typical lit grass/leaves), tiling.
pub fn mid_foliage(size: u32, seed: u32) -> Image {
    image(size, size, |x, y| {
        let (fx, fy) = (x as f32, y as f32);
        let l = 0.4 + 0.25 * smooth_noise(fx, fy, 8, size, seed) + 0.06 * noise(x, y, seed);
        let [r, g, b] = from_oklch(
            l,
            0.1,
            125.0 + 15.0 * smooth_noise(fx, fy, 4, size, seed + 1),
        );
        [r, g, b, 1.0]
    })
}

/// Stone blocks with mortar and photographic grit (fine light/dark noise).
pub fn gritty_blocks(size: u32, seed: u32) -> Image {
    image(size, size, |x, y| {
        let mortar = y % 32 < 4 || (x + if (y / 32) % 2 == 1 { 24 } else { 0 }) % 48 < 4;
        let base = if mortar { 0.35 } else { 0.62 };
        let l = base + 0.18 * (noise(x, y, seed) - 0.5);
        let [r, g, b] = from_oklch(l, 0.05, 75.0);
        [r, g, b, 1.0]
    })
}

/// Tree bark: near-neutral olive-gray ridges with deep near-black vertical grooves (like OoT
/// Reloaded's Kokiri Forest trunks), tiling.
pub fn bark(size: u32, seed: u32) -> Image {
    image(size, size, |x, y| {
        let (fx, fy) = (x as f32, y as f32);
        // Wavy vertical grooves.
        let u = fx / size as f32 * 10.0 + 0.8 * smooth_noise(fx, fy, 6, size, seed);
        let ridge = (u * std::f32::consts::TAU).sin() * 0.5 + 0.5;
        let groove = 1.0 - ((ridge - 0.25) / 0.25).clamp(0.0, 1.0);
        let l = 0.62 * (1.0 - groove) + 0.08 * groove + 0.1 * (noise(x, y, seed) - 0.5);
        let [r, g, b] = from_oklch(l.clamp(0.02, 0.95), 0.025 + 0.015 * (1.0 - groove), 105.0);
        [r, g, b, 1.0]
    })
}

/// Dark brown bark (colored darks).
pub fn dark_brown_bark(size: u32, seed: u32) -> Image {
    image(size, size, |x, y| {
        let (fx, fy) = (x as f32, y as f32);
        let u = fx / size as f32 * 8.0 + 0.6 * smooth_noise(fx, fy, 5, size, seed);
        let ridge = (u * std::f32::consts::TAU).sin() * 0.5 + 0.5;
        let l = 0.12 + 0.2 * ridge + 0.05 * noise(x, y, seed);
        let [r, g, b] = from_oklch(l, 0.05 + 0.02 * ridge, 55.0);
        [r, g, b, 1.0]
    })
}

/// Pale peach skin, like an actor's hand or face texture.
pub fn pale_skin(size: u32, seed: u32) -> Image {
    image(size, size, |x, y| {
        let l = 0.84
            + 0.06 * smooth_noise(x as f32, y as f32, 4, size, seed)
            + 0.01 * noise(x, y, seed);
        let [r, g, b] = from_oklch(l, 0.07, 70.0);
        [r, g, b, 1.0]
    })
}

/// A leafy cutout: an opaque disc of mid green on fully transparent black texels.
pub fn cutout(size: u32, seed: u32) -> Image {
    let c = size as f32 / 2.0;
    image(size, size, |x, y| {
        let d = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt();
        if d < size as f32 * 0.35 {
            let [r, g, b] = from_oklch(0.6 + 0.05 * noise(x, y, seed), 0.1, 135.0);
            [r, g, b, 1.0]
        } else {
            [0.0, 0.0, 0.0, 0.0]
        }
    })
}

/// A hard vertical step between two flat, finely noisy regions.
pub fn step_edge(size: u32, seed: u32) -> Image {
    image(size, size, |x, y| {
        let n = 0.04 * (noise(x, y, seed) - 0.5);
        let l = if x < size / 2 { 0.4 } else { 0.8 } + n;
        let [r, g, b] = from_oklch(l, 0.06, 80.0);
        [r, g, b, 1.0]
    })
}

/// A seamlessly tiling colorful pattern.
pub fn tiling(size: u32, seed: u32) -> Image {
    image(size, size, |x, y| {
        let (fx, fy) = (x as f32, y as f32);
        let l = 0.3 + 0.5 * smooth_noise(fx, fy, 6, size, seed);
        let h = 360.0 * smooth_noise(fx, fy, 3, size, seed + 7);
        let [r, g, b] = from_oklch(l, 0.1, h);
        [r, g, b, 1.0]
    })
}

/// True where the synthetic sign ([`sign`]) has a letter stroke: rows of 12×16 glyphs made of
/// 3-texel strokes (a verticals / horizontals pattern per glyph, like blocky lettering).
pub fn is_letter(x: u32, y: u32) -> bool {
    let rows = [40u32, 100, 160, 220];
    let Some(&top) = rows.iter().find(|&&t| (t..t + 16).contains(&y)) else {
        return false;
    };
    if !(20..236).contains(&x) {
        return false;
    }
    let (gx, gy) = ((x - 20) % 18, y - top);
    if gx >= 12 {
        return false;
    }
    let glyph = (x - 20) / 18 + 7 * (top / 60);
    let bits = (glyph.wrapping_mul(2_654_435_761) >> 7) | 1;
    let left = gx < 3;
    let right = gx >= 9;
    let center = (5..8).contains(&gx);
    let top_bar = gy < 3;
    let mid_bar = (7..10).contains(&gy);
    let bottom_bar = gy >= 13;
    (left && bits & 1 != 0)
        || (right && bits & 2 != 0)
        || (center && bits & 4 != 0)
        || (top_bar && bits & 8 != 0)
        || (mid_bar && bits & 16 != 0)
        || (bottom_bar && bits & 32 != 0)
}

/// A wooden sign: busy, strongly grained pale planks with small dark painted lettering.
pub fn sign(size: u32, seed: u32) -> Image {
    image(size, size, |x, y| {
        let (fx, fy) = (x as f32, y as f32);
        if is_letter(x, y) {
            let [r, g, b] = from_oklch(0.2 + 0.03 * noise(x, y, seed), 0.03, 50.0);
            return [r, g, b, 1.0];
        }
        let phase = 3.0 * smooth_noise(fx, fy, 4, size, seed + 1);
        let grain = (fy / 9.0 * std::f32::consts::TAU + phase).sin();
        let l = 0.62 + 0.14 * grain + 0.12 * (noise(x, y, seed) - 0.5);
        let [r, g, b] = from_oklch(l, 0.06, 70.0);
        [r, g, b, 1.0]
    })
}

/// A pre-rendered room (not tiling): crushed near-black corners, mid-value walls with painted
/// texture, a bright window, and light falling off across the image.
pub fn room(size: u32, seed: u32) -> Image {
    image(size, size, |x, y| {
        let (fx, fy) = (x as f32 / size as f32, y as f32 / size as f32);
        let falloff = 1.0 - 0.35 * fx;
        let corner = (fx < 0.3 && fy > 0.6) || (fx > 0.8 && fy < 0.25);
        let window = (0.45..0.65).contains(&fx) && (0.15..0.45).contains(&fy);
        let tex = smooth_noise(x as f32, y as f32, 12, size, seed);
        let (l, c, h) = if window {
            (0.88 + 0.04 * tex, 0.03, 100.0)
        } else if corner {
            (0.06 + 0.06 * tex, 0.02, 60.0)
        } else {
            (
                (0.3 + 0.25 * tex) * falloff + 0.03 * noise(x, y, seed),
                0.05,
                65.0,
            )
        };
        let [r, g, b] = from_oklch(l, c, h);
        [r, g, b, 1.0]
    })
}

/// Two materials: green moss patches over brown wood streaks, both mid-dark (a Deku Tree wall).
pub fn moss_on_wood(size: u32, seed: u32) -> Image {
    image(size, size, |x, y| {
        let (fx, fy) = (x as f32, y as f32);
        let moss = smooth_noise(fx, fy, 5, size, seed) > 0.55;
        let n = noise(x, y, seed + 1);
        let [r, g, b] = if moss {
            from_oklch(0.42 + 0.1 * n, 0.07, 125.0 + 10.0 * n)
        } else {
            let streak = smooth_noise(fx * 8.0, fy, 16, size * 8, seed + 2);
            from_oklch(0.3 + 0.12 * streak + 0.04 * n, 0.055, 52.0 + 8.0 * n)
        };
        [r, g, b, 1.0]
    })
}

/// Olive moss: mottled olive-green (hues across `hue`), mid-dark, like OoT Reloaded's mossy ground
/// and Deku Tree moss.
pub fn olive_moss(size: u32, seed: u32, hue: [f32; 2]) -> Image {
    image(size, size, |x, y| {
        let (fx, fy) = (x as f32, y as f32);
        let t = smooth_noise(fx, fy, 6, size, seed);
        let n = noise(x, y, seed + 1);
        let h = hue[0] + (hue[1] - hue[0]) * smooth_noise(fx, fy, 4, size, seed + 2);
        let [r, g, b] = from_oklch(0.34 + 0.16 * t + 0.04 * n, 0.05 + 0.02 * t, h);
        [r, g, b, 1.0]
    })
}

/// Near-black and near-neutral darks with faint casts of every hue (crushed shadows, fades to
/// black).
pub fn near_neutral_darks(size: u32, seed: u32) -> Image {
    image(size, size, |x, y| {
        let l = 0.03 + 0.22 * y as f32 / size as f32 + 0.02 * noise(x, y, seed);
        let c = 0.011 * noise(x, y, seed + 1);
        let [r, g, b] = from_oklch(l, c, 360.0 * x as f32 / size as f32);
        [r, g, b, 1.0]
    })
}

pub fn lch(p: [f32; 4]) -> [f32; 3] {
    color::oklab_to_oklch(color::srgb_to_oklab([p[0], p[1], p[2]]))
}
