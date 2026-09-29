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

/// Warm wood planks with deep dark grooves (a pre-rendered house wall, a plank floor).
pub fn grooved_wood(size: u32, seed: u32) -> Image {
    image(size, size, |x, y| {
        let (fx, fy) = (x as f32, y as f32);
        let v = fy / size as f32 * 7.0 + 0.6 * smooth_noise(fx, fy, 5, size, seed);
        let ridge = (v * std::f32::consts::TAU).sin() * 0.5 + 0.5;
        let groove = 1.0 - ((ridge - 0.2) / 0.25).clamp(0.0, 1.0);
        let n = noise(x, y, seed + 1);
        let l = 0.58 * (1.0 - groove) + 0.14 * groove + 0.08 * (n - 0.5);
        let [r, g, b] = from_oklch(l.clamp(0.03, 0.95), 0.045 - 0.02 * groove, 68.0 + 6.0 * n);
        [r, g, b, 1.0]
    })
}

/// Engine-tinted cloth: a light gray with soft diagonal folds (Link's tunic), grayscale.
pub fn cloth_folds(size: u32, seed: u32) -> Image {
    image(size, size, |x, y| {
        let (fx, fy) = (x as f32, y as f32);
        let u = (fx + 0.6 * fy) / size as f32 * 5.0 + 0.4 * smooth_noise(fx, fy, 4, size, seed);
        let fold = (u * std::f32::consts::TAU).sin() * 0.5 + 0.5;
        let v = 0.45 + 0.35 * fold + 0.04 * (noise(x, y, seed + 1) - 0.5);
        [v, v, v, 1.0]
    })
}

/// A shaded gray ball cut out with a hard alpha edge (a bomb, a statue knob): an object, not an
/// effect, though it falls off radially.
pub fn gray_ball(size: u32, seed: u32) -> Image {
    let c = size as f32 / 2.0;
    image(size, size, |x, y| {
        let r2 = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)) / (size as f32 * 0.42).powi(2);
        let v = 0.25 + 0.4 * (1.0 - r2).max(0.0) + 0.08 * (noise(x, y, seed) - 0.5);
        [v, v, v, if r2 < 1.0 { 1.0 } else { 0.0 }]
    })
}

/// A soft gray radial glow on transparent texels (a flare, spark or puff: an effect).
pub fn glow(size: u32) -> Image {
    let c = size as f32 / 2.0;
    image(size, size, |x, y| {
        let r2 = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)) / (size as f32 * 0.18).powi(2);
        [1.0, 1.0, 1.0, (-r2).exp()]
    })
}

/// Bright highlights: a mid-gray textured surface with a large blown-white patch, compact blown
/// glints, and near-white (unclipped) areas that brushwork could push over.
pub fn highlights(size: u32, seed: u32) -> Image {
    image(size, size, |x, y| {
        let (fx, fy) = (x as f32, y as f32);
        if x > size * 3 / 4 && y < size / 3 {
            return [1.0, 1.0, 1.0, 1.0];
        }
        if (5..12).contains(&(x % 37)) && (9..16).contains(&(y % 41)) {
            return [1.0, 1.0, 1.0, 1.0];
        }
        let t = smooth_noise(fx, fy, 6, size, seed);
        let l = if y > size * 2 / 3 {
            0.93 + 0.04 * t
        } else {
            0.35 + 0.3 * t
        };
        let [r, g, b] = from_oklch(l + 0.02 * (noise(x, y, seed) - 0.5), 0.03, 80.0);
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

/// Weight (0..1) of a tiling network of thin lines: the borders of a periodic Voronoi diagram
/// (caustics, lava veins), `width` texels wide, about `cells` cells across.
pub fn line_network(size: u32, seed: u32, cells: u32, width: f32) -> Vec<f32> {
    let s = size as f32;
    let points: Vec<[f32; 2]> = (0..cells * cells)
        .map(|i| {
            let (cx, cy) = ((i % cells) as f32, (i / cells) as f32);
            let step = s / cells as f32;
            [
                (cx + 0.15 + 0.7 * noise(i, 1, seed)) * step,
                (cy + 0.15 + 0.7 * noise(i, 2, seed)) * step,
            ]
        })
        .collect();
    (0..size * size)
        .map(|i| {
            let (x, y) = ((i % size) as f32 + 0.5, (i / size) as f32 + 0.5);
            let (mut d1, mut d2) = (f32::MAX, f32::MAX);
            for p in &points {
                let dx = (x - p[0]).abs().min(s - (x - p[0]).abs());
                let dy = (y - p[1]).abs().min(s - (y - p[1]).abs());
                let d = (dx * dx + dy * dy).sqrt();
                if d < d1 {
                    d2 = d1;
                    d1 = d;
                } else if d < d2 {
                    d2 = d;
                }
            }
            (-((d2 - d1) / width).powi(2)).exp()
        })
        .collect()
}

/// Caustic water: soft glowing lines over smooth, darker translucent depth, tiling. `chroma`
/// 0 gives engine-tinted gray water. Returns the image and its line weights.
pub fn caustic_water(size: u32, seed: u32, chroma: f32) -> (Image, Vec<f32>) {
    let lines = line_network(size, seed, 6, 1.6);
    let img = image(size, size, |x, y| {
        let t = lines[(y * size + x) as usize];
        let depth = smooth_noise(x as f32, y as f32, 4, size, seed + 3);
        // Mottled depth, as painted packs have it (busy enough for value grouping).
        let mottle = smooth_noise(x as f32, y as f32, 24, size, seed + 7);
        let l = 0.34 + 0.08 * depth + 0.16 * mottle + 0.02 * noise(x, y, seed + 4) + 0.4 * t;
        let [r, g, b] = from_oklch(l, chroma * (1.0 - 0.5 * t), 175.0 + 10.0 * depth);
        [r, g, b, 1.0]
    });
    (img, lines)
}

/// Lava: glowing orange-yellow veins over a dark red-brown crust, tiling. Returns the image and
/// its vein weights.
pub fn lava(size: u32, seed: u32) -> (Image, Vec<f32>) {
    let veins = line_network(size, seed, 5, 3.0);
    let img = image(size, size, |x, y| {
        let t = veins[(y * size + x) as usize];
        let crust = smooth_noise(x as f32, y as f32, 8, size, seed + 5);
        let l = 0.22 + 0.06 * crust + 0.02 * noise(x, y, seed + 6) + 0.55 * t;
        let [r, g, b] = from_oklch(l, 0.08 + 0.12 * t, 30.0 + 30.0 * t);
        [r, g, b, 1.0]
    });
    (img, veins)
}

pub fn lch(p: [f32; 4]) -> [f32; 3] {
    color::oklab_to_oklch(color::srgb_to_oklab([p[0], p[1], p[2]]))
}

/// Vertex-colored cracked dirt (Kokiri Forest's path): a light, grainy gray with thin, branching
/// mid-gray cracks; grayscale, so the engine's tint colors it. `is_crack` tells the crack texels.
pub fn cracked_ground(size: u32, seed: u32) -> (Image, impl Fn(u32, u32) -> bool) {
    let s = size as f32;
    let is_crack = move |x: u32, y: u32| {
        let (fx, fy) = (x as f32, y as f32);
        let a = (fy - (0.35 * s + 0.08 * s * (fx / s * 9.0).sin() + 0.4 * fx)).abs() < 1.2;
        let b = (fx - (0.6 * s + 0.06 * s * (fy / s * 11.0).sin())).abs() < 1.0 && fy > 0.3 * s;
        let c = (fy - (0.8 * s - 0.5 * fx + 0.05 * s * (fx / s * 13.0).cos())).abs() < 1.0
            && fx > 0.2 * s
            && fx < 0.7 * s;
        (a && fx < 0.9 * s) || b || c
    };
    let img = image(size, size, |x, y| {
        let t = smooth_noise(x as f32, y as f32, 8, size, seed);
        let v = if is_crack(x, y) {
            0.5 + 0.05 * t
        } else {
            0.78 + 0.1 * t + 0.05 * (noise(x, y, seed + 1) - 0.5)
        };
        [v, v, v, 1.0]
    });
    (img, is_crack)
}

/// A gold plate with shiny studs (the gold chest): saturated yellow metal with mottling, round
/// studs with a dark rim and a near-white highlight.
pub fn gold_studs(size: u32, seed: u32) -> Image {
    image(size, size, |x, y| {
        let (fx, fy) = (x as f32, y as f32);
        let t = smooth_noise(fx, fy, 6, size, seed);
        let (cx, cy) = (
            (fx / 32.0).floor() * 32.0 + 16.0,
            (fy / 32.0).floor() * 32.0 + 16.0,
        );
        let r = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
        let [l, c] = if r < 6.0 {
            let hl =
                (1.0 - ((fx - cx + 2.0).powi(2) + (fy - cy + 2.0).powi(2)).sqrt() / 4.0).max(0.0);
            [0.75 + 0.22 * hl, 0.14 * (1.0 - hl) + 0.02]
        } else if r < 8.0 {
            [0.35, 0.07]
        } else {
            [0.62 + 0.1 * t, 0.12 + 0.03 * t]
        };
        let [r, g, b] = from_oklch(l, c, 95.0);
        [r, g, b, 1.0]
    })
}

/// Thin shafts on a busy wall (tool handles, poles, rails in a painted room): a gritty bark-like
/// wall with 3-texel shafts slightly lighter and darker than the wall (low contrast, like the
/// pitchfork handles in Link's house), vertical and diagonal. `shaft` tells a shaft texel and
/// its kind (+1 lighter, -1 darker, 2 a reddish shaft as light as the wall).
pub fn wall_with_shafts(size: u32, seed: u32) -> (Image, impl Fn(u32, u32) -> i32) {
    let wall = bark(size, seed);
    let s = size as f32;
    let shaft = move |x: u32, y: u32| -> i32 {
        let (fx, fy) = (x as f32, y as f32);
        if fy < 0.1 * s || fy > 0.9 * s {
            return 0;
        }
        let d = |x0: f32, slope: f32| (fx - (x0 + slope * (fy - 0.1 * s))).abs();
        if d(0.2 * s, 0.0) < 1.5 || d(0.62 * s, 0.25) < 1.5 {
            1
        } else if d(0.42 * s, 0.0) < 1.5 || d(0.8 * s, -0.2) < 1.5 {
            -1
        } else if d(0.32 * s, 0.0) < 1.5 || d(0.92 * s, 0.0) < 1.5 {
            2
        } else {
            0
        }
    };
    let img = image(size, size, |x, y| {
        let p = wall.pixels[(y * size + x) as usize];
        let [l, c, h] = crate::common::lch(p);
        let sgn = shaft(x, y) as f32;
        if sgn == 0.0 {
            return p;
        }
        // Local wall lightness, blurred, plus a small offset.
        let mut m = 0.0;
        let mut n = 0.0;
        for dy in -6i32..=6 {
            for dx in -6i32..=6 {
                let (xx, yy) = (
                    (x as i32 + dx).clamp(0, size as i32 - 1),
                    (y as i32 + dy).clamp(0, size as i32 - 1),
                );
                m += crate::common::lch(wall.pixels[(yy as u32 * size + xx as u32) as usize])[0];
                n += 1.0;
            }
        }
        let _ = l;
        // Kind 2: a reddish handle as light as the wall around it (only its color differs).
        let [r, g, b] = if sgn == 2.0 {
            from_oklch(m / n, 0.09, 45.0)
        } else {
            from_oklch(m / n + 0.12 * sgn, c.max(0.03), h)
        };
        [r, g, b, 1.0]
    });
    (img, shaft)
}

/// Light, warm bark with dark grooves and small dark pits scattered over the light ridges (OoT
/// Reloaded's Kokiri treehouse bark): 2-4 texel pits, some alone, some in pairs.
pub fn pitted_bark(size: u32, seed: u32) -> Image {
    image(size, size, |x, y| {
        let (fx, fy) = (x as f32, y as f32);
        let u = fx / size as f32 * 6.0 + 0.7 * smooth_noise(fx, fy, 5, size, seed);
        let ridge = (u * std::f32::consts::TAU).sin() * 0.5 + 0.5;
        let groove = 1.0 - ((ridge - 0.15) / 0.2).clamp(0.0, 1.0);
        let n = noise(x, y, seed + 1);
        let t = smooth_noise(fx, fy, 8, size, seed + 2);
        let (bx, by) = (x / 5, y / 5);
        let side = 2 + (noise(bx, by, seed + 4) * 3.0) as u32;
        let pit = groove < 0.1 && noise(bx, by, seed + 3) > 0.9 && x % 5 < side && y % 5 < side;
        let [l, c, h] = if pit {
            [0.2 + 0.04 * n, 0.03, 62.0]
        } else {
            let l = 0.66 * (1.0 - groove) + 0.3 * groove + 0.05 * t + 0.04 * (n - 0.5);
            [l, 0.05 - 0.008 * groove, 78.0 + 6.0 * n]
        };
        let [r, g, b] = from_oklch(l, c, h);
        [r, g, b, 1.0]
    })
}

/// Near-black colors of every hue: lightness 0.02-0.15 top to bottom, chroma 0.015-0.065 (dark
/// cobbles going down into a pit, deep shadow in a colored wall).
pub fn near_black_colors(size: u32, seed: u32) -> Image {
    image(size, size, |x, y| {
        let l = 0.02 + 0.13 * y as f32 / size as f32 + 0.01 * noise(x, y, seed);
        let c = 0.015 + 0.05 * noise(x, y, seed + 1);
        let [r, g, b] = from_oklch(l, c, 360.0 * x as f32 / size as f32);
        [r, g, b, 1.0]
    })
}

/// Near-neutral darks with a faint cast of `hue` (steel and iron plates, slate carvings, a
/// green-gray door): mottled lightness 0.1-0.4 with darker grooves every 24 texels, chroma about
/// `chroma` (lightness-relative chroma stays below 0.03: "neutral" to the palette), with small
/// enamel insets of the same hue (3% of the texels), so the texture is not engine-tinted gray.
pub fn faint_cast_darks(size: u32, seed: u32, hue: f32, chroma: f32) -> Image {
    image(size, size, |x, y| {
        if inset(x, y) {
            let [r, g, b] = from_oklch(0.45, 0.1, hue);
            return [r, g, b, 1.0];
        }
        let (fx, fy) = (x as f32, y as f32);
        let groove = if x % 24 < 3 || y % 24 < 3 { 0.55 } else { 1.0 };
        let l =
            (0.12 + 0.26 * smooth_noise(fx, fy, 6, size, seed) + 0.03 * noise(x, y, seed)) * groove;
        let c = chroma * (0.6 + 0.4 * noise(x, y, seed + 1));
        let [r, g, b] = from_oklch(l.max(0.06), c, hue + 10.0 * (noise(x, y, seed + 2) - 0.5));
        [r, g, b, 1.0]
    })
}

/// A near-black neutral wall (lightness 0.02-0.12, no cast), with soft mottling and small cool
/// blue-gray insets (3% of the texels), so the texture is not engine-tinted gray.
pub fn black_wall(size: u32, seed: u32) -> Image {
    image(size, size, |x, y| {
        if inset(x, y) {
            let [r, g, b] = from_oklch(0.35, 0.08, 250.0);
            return [r, g, b, 1.0];
        }
        let l = 0.02 + 0.1 * smooth_noise(x as f32, y as f32, 7, size, seed);
        let v = color::oklab_to_srgb([l, 0.0, 0.0]).map(|v| v.clamp(0.0, 1.0));
        [v[0], v[1], v[2], 1.0]
    })
}

/// Small square insets, 8 of every 48 texels in each direction (about 3% of the texels).
fn inset(x: u32, y: u32) -> bool {
    (30..38).contains(&(x % 48)) && (30..38).contains(&(y % 48))
}
