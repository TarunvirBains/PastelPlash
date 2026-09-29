//! Fluid detection: water, lava and other liquids, recognized by their look alone (no names,
//! no game knowledge). Fluids are painted as soft light over depth: thin bright connected lines
//! (caustics, glowing veins, flow lines) over a smooth darker body, usually tiling. They need
//! their own material treatment (see `Category::Water`, `Lava`, `Liquid`): value grouping,
//! coarse simplification and wet-edge outlines turn caustics into cracked-stone cells.
//!
//! Everything is measured on a thumbnail (long side [`THUMB`]) so the decision does not depend on
//! the texture's resolution.

use crate::color;
use crate::image::{Image, SourceColor, SourceFormat};

/// Long side of the analysis thumbnail.
pub const THUMB: u32 = 256;

/// What kind of fluid a texture is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FluidKind {
    /// Clear, murky or engine-tinted water.
    Water,
    /// Glowing molten rock: emissive, keeps its heat colors and glow in every mood.
    Lava,
    /// Other liquids (poison, swamp, organic fluids): fluid treatment, own color.
    Liquid,
}

impl FluidKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Water => "water",
            Self::Lava => "lava",
            Self::Liquid => "liquid",
        }
    }
}

/// The measurements the detector decides on.
#[derive(Debug, Clone, Copy, Default)]
pub struct Features {
    /// Share of (mostly) opaque texels (every texel of a translucent overlay counts).
    pub opaque: f32,
    /// Share of partly transparent texels (alpha between 0.05 and 0.95).
    pub translucent: f32,
    /// Long side over short side.
    pub aspect: f32,
    pub l_mean: f32,
    pub l_std: f32,
    pub c_mean: f32,
    pub c_p90: f32,
    /// Chroma-weighted mean hue (degrees) and its coherence (0 = scattered, 1 = one hue).
    pub hue: f32,
    pub hue_coherence: f32,
    /// Skewness of the band-passed lightness: thin bright lines over a broad body > 0.
    pub skew: f32,
    /// Share of texels on bright ridges.
    pub ridge: f32,
    /// Mean ridge height above the local body (OKLab L).
    pub ridge_amp: f32,
    /// Line-likeness of the ridges (structure-tensor coherence, 0..1).
    pub ridge_line: f32,
    /// Share of ridge texels in large connected networks.
    pub ridge_net: f32,
    /// Share of the cells of an 8×8 grid that hold ridges.
    pub ridge_spread: f32,
    /// Fine grit of the body (std of the finest band, off the ridges), OKLab L.
    pub grit: f32,
    /// Share of glowing warm texels (bright, saturated, red to yellow).
    pub glow: f32,
    /// Share of warm, clearly colored texels.
    pub warm: f32,
    /// Share of dark texels.
    pub dark: f32,
    /// Seam ratios per axis (tiling when small).
    pub seam: [f32; 2],
}

/// The detector's verdict.
#[derive(Debug, Clone, Copy)]
pub struct Detection {
    pub kind: Option<FluidKind>,
    /// Pattern score (0..1): caustic/flow lines over a smooth body.
    pub pattern: f32,
    /// Water, lava and liquid scores (0..1); the kind is the best one above [`THRESHOLD`].
    pub water: f32,
    pub lava: f32,
    pub liquid: f32,
    pub features: Features,
}

impl Detection {
    /// The best score.
    pub fn score(&self) -> f32 {
        self.water.max(self.lava).max(self.liquid)
    }
}

/// A score at or above this is a fluid.
pub const THRESHOLD: f32 = 0.5;

fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    crate::palette::smoothstep(e0, e1, x)
}

/// Area-averaged (premultiplied) copy fitting a `side` box; the image itself when it already fits.
pub fn thumbnail(image: &Image, side: u32) -> Image {
    if image.width <= side && image.height <= side {
        return image.clone();
    }
    let (w, h) = (image.width as f32, image.height as f32);
    let s = side as f32 / w.max(h);
    let (ow, oh) = (
        ((w * s).round() as u32).max(1),
        ((h * s).round() as u32).max(1),
    );
    let (sx, sy) = (w / ow as f32, h / oh as f32);
    let mut pixels = Vec::with_capacity((ow * oh) as usize);
    for oy in 0..oh {
        let (y0, y1) = (
            (oy as f32 * sy) as u32,
            (((oy + 1) as f32 * sy).ceil() as u32).min(image.height),
        );
        for ox in 0..ow {
            let (x0, x1) = (
                (ox as f32 * sx) as u32,
                (((ox + 1) as f32 * sx).ceil() as u32).min(image.width),
            );
            let mut acc = [0.0f32; 4];
            let mut n = 0.0f32;
            for y in y0..y1.max(y0 + 1) {
                let row = (y * image.width) as usize;
                for x in x0..x1.max(x0 + 1) {
                    let p = image.pixels[row + x as usize];
                    for c in 0..3 {
                        acc[c] += p[c] * p[3];
                    }
                    acc[3] += p[3];
                    n += 1.0;
                }
            }
            pixels.push(if acc[3] > 0.0 {
                [
                    acc[0] / acc[3],
                    acc[1] / acc[3],
                    acc[2] / acc[3],
                    acc[3] / n,
                ]
            } else {
                [0.0; 4]
            });
        }
    }
    Image {
        width: ow,
        height: oh,
        pixels,
        source: SourceFormat {
            color: SourceColor::Rgba,
            bit_depth: 8,
            has_alpha: true,
        },
        source_scale: None,
        tint_safe: None,
    }
}

/// Separable box blur with wrap-around, `passes` times (≈ Gaussian).
fn blur(v: &[f32], w: usize, h: usize, r: usize, passes: usize) -> Vec<f32> {
    let mut a = v.to_vec();
    let mut b = vec![0.0f32; v.len()];
    if r == 0 {
        return a;
    }
    let n = (2 * r + 1) as f32;
    for _ in 0..passes {
        for y in 0..h {
            let row = &a[y * w..(y + 1) * w];
            let mut s: f32 = (0..=2 * r).map(|k| row[(k + w * 4 - r) % w]).sum();
            for x in 0..w {
                b[y * w + x] = s / n;
                s += row[(x + r + 1) % w] - row[(x + w * 4 - r) % w];
            }
        }
        for x in 0..w {
            let mut s: f32 = (0..=2 * r).map(|k| b[((k + h * 4 - r) % h) * w + x]).sum();
            for y in 0..h {
                a[y * w + x] = s / n;
                s += b[((y + r + 1) % h) * w + x] - b[((y + h * 4 - r) % h) * w + x];
            }
        }
    }
    a
}

/// Measures a (thumbnail-sized) image.
pub fn features(thumb: &Image) -> Features {
    let (w, h) = (thumb.width as usize, thumb.height as usize);
    let n = w * h;
    let mut f = Features {
        seam: [
            crate::analysis::seam_ratio(thumb, false),
            crate::analysis::seam_ratio(thumb, true),
        ],
        aspect: w.max(h) as f32 / w.min(h).max(1) as f32,
        ..Features::default()
    };
    if w < 8 || h < 8 {
        return f;
    }
    let lab: Vec<[f32; 3]> = thumb
        .pixels
        .iter()
        .map(|p| color::srgb_to_oklab([p[0], p[1], p[2]]))
        .collect();
    // Translucent overlays (water sheets drawn over the ground) carry their pattern in alpha:
    // they are measured as lightness over black. Cut-outs are measured on their opaque texels.
    f.translucent = thumb
        .pixels
        .iter()
        .filter(|p| (0.05..0.95).contains(&p[3]))
        .count() as f32
        / n as f32;
    let overlay = f.translucent >= 0.25;
    let lab: Vec<[f32; 3]> = if overlay {
        lab.iter()
            .zip(&thumb.pixels)
            .map(|(q, p)| [q[0] * p[3], q[1] * p[3], q[2] * p[3]])
            .collect()
    } else {
        lab
    };
    let opaque: Vec<bool> = thumb
        .pixels
        .iter()
        .map(|p| overlay || p[3] >= 0.5)
        .collect();
    let count = opaque.iter().filter(|&&o| o).count();
    f.opaque = count as f32 / n as f32;
    if count < 64 {
        return f;
    }
    let m = count as f32;
    // Color.
    let mut chromas = Vec::with_capacity(count);
    let (mut sl, mut sl2, mut sc, mut ha, mut hb, mut hw) = (0.0f32, 0.0, 0.0, 0.0, 0.0, 0.0);
    let (mut glow, mut warm, mut dark) = (0.0f32, 0.0, 0.0);
    for (i, p) in lab.iter().enumerate() {
        if !opaque[i] {
            continue;
        }
        let [l, c, hue] = color::oklab_to_oklch(*p);
        sl += l;
        sl2 += l * l;
        sc += c;
        chromas.push(c);
        ha += p[1];
        hb += p[2];
        hw += c;
        let warm_hue = !(95.0..350.0).contains(&hue);
        if warm_hue && c > 0.06 {
            warm += 1.0;
            if l > 0.62 && c > 0.1 {
                glow += 1.0;
            }
        }
        if l < 0.3 {
            dark += 1.0;
        }
    }
    f.l_mean = sl / m;
    f.l_std = (sl2 / m - f.l_mean * f.l_mean).max(0.0).sqrt();
    f.c_mean = sc / m;
    chromas.sort_by(f32::total_cmp);
    f.c_p90 = chromas[(chromas.len() * 9 / 10).min(chromas.len() - 1)];
    f.hue = hb.atan2(ha).to_degrees().rem_euclid(360.0);
    f.hue_coherence = if hw > 1e-6 { ha.hypot(hb) / hw } else { 0.0 };
    f.glow = glow / m;
    f.warm = warm / m;
    f.dark = dark / m;

    // Band-passed lightness: the body at the cell scale, ridges above it.
    let side = w.max(h) as f32;
    let l: Vec<f32> = lab
        .iter()
        .zip(&opaque)
        .map(|(p, &o)| if o { p[0] } else { f.l_mean })
        .collect();
    let body = blur(&l, w, h, (side / 64.0).round().max(1.0) as usize, 3);
    let fine = blur(&l, w, h, 1, 1);
    let band: Vec<f32> = fine.iter().zip(&body).map(|(a, b)| a - b).collect();
    let bm = band.iter().sum::<f32>() / n as f32;
    let bs = (band.iter().map(|v| (v - bm).powi(2)).sum::<f32>() / n as f32)
        .sqrt()
        .max(1e-5);
    f.skew = band.iter().map(|v| ((v - bm) / bs).powi(3)).sum::<f32>() / n as f32;
    let thr = (bm + bs).max(bm + 0.015);
    let ridge: Vec<bool> = band.iter().map(|&v| v > thr).collect();
    let rn = ridge.iter().filter(|&&r| r).count();
    f.ridge = rn as f32 / n as f32;
    // Spread: caustics cover the whole sheet; a frame or a relief has its lines in one place.
    let mut cells = [0usize; 64];
    for (i, &r) in ridge.iter().enumerate() {
        if r {
            cells[((i / w) * 8 / h) * 8 + (i % w) * 8 / w] += 1;
        }
    }
    let per_cell = n as f32 / 64.0;
    f.ridge_spread = cells
        .iter()
        .filter(|&&c| c as f32 >= 0.03 * per_cell)
        .count() as f32
        / 64.0;
    if rn > 0 {
        f.ridge_amp = ridge
            .iter()
            .zip(&band)
            .filter(|(r, _)| **r)
            .map(|(_, v)| v - bm)
            .sum::<f32>()
            / rn as f32;
    }
    // Grit: the finest band (texel noise), off the ridges and their neighbors.
    let near_ridge = {
        let r: Vec<f32> = ridge.iter().map(|&b| b as u8 as f32).collect();
        blur(&r, w, h, 1, 1)
    };
    let (mut gs, mut gn) = (0.0f32, 0.0f32);
    for i in 0..n {
        if near_ridge[i] == 0.0 && opaque[i] {
            let d = l[i] - fine[i];
            gs += d * d;
            gn += 1.0;
        }
    }
    f.grit = if gn > 0.0 { (gs / gn).sqrt() } else { 0.0 };
    // Line-likeness: structure-tensor coherence of the fine lightness on the ridges.
    let (mut jxx, mut jxy, mut jyy) = (vec![0.0f32; n], vec![0.0f32; n], vec![0.0f32; n]);
    for y in 0..h {
        for x in 0..w {
            let at = |dx: isize, dy: isize| {
                fine[((y as isize + dy).rem_euclid(h as isize) as usize) * w
                    + (x as isize + dx).rem_euclid(w as isize) as usize]
            };
            let gx = at(1, 0) - at(-1, 0);
            let gy = at(0, 1) - at(0, -1);
            let i = y * w + x;
            (jxx[i], jxy[i], jyy[i]) = (gx * gx, gx * gy, gy * gy);
        }
    }
    let (jxx, jxy, jyy) = (
        blur(&jxx, w, h, 2, 2),
        blur(&jxy, w, h, 2, 2),
        blur(&jyy, w, h, 2, 2),
    );
    let (mut cs, mut cn) = (0.0f32, 0.0f32);
    for i in 0..n {
        if ridge[i] {
            let tr = jxx[i] + jyy[i];
            if tr > 1e-9 {
                cs += ((jxx[i] - jyy[i]).powi(2) + 4.0 * jxy[i] * jxy[i]).sqrt() / tr;
                cn += 1.0;
            }
        }
    }
    f.ridge_line = if cn > 0.0 { cs / cn } else { 0.0 };
    // Networks: ridge texels in large 8-connected components (wrapping).
    let big = ((n as f32) * 0.004).max(24.0) as usize;
    let mut seen = vec![false; n];
    let mut in_big = 0usize;
    let mut stack = Vec::new();
    for start in 0..n {
        if !ridge[start] || seen[start] {
            continue;
        }
        seen[start] = true;
        stack.push(start);
        let mut size = 0usize;
        while let Some(i) = stack.pop() {
            size += 1;
            let (x, y) = ((i % w) as isize, (i / w) as isize);
            for dy in -1..=1isize {
                for dx in -1..=1isize {
                    let j = ((y + dy).rem_euclid(h as isize) as usize) * w
                        + (x + dx).rem_euclid(w as isize) as usize;
                    if ridge[j] && !seen[j] {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        if size >= big {
            in_big += size;
        }
    }
    f.ridge_net = if rn > 0 {
        in_big as f32 / rn as f32
    } else {
        0.0
    };
    f
}

/// Scores the features.
pub fn score(f: &Features) -> Detection {
    // A surface that repeats: a whole, roughly square, seamlessly tiling sheet.
    let surface = smooth(0.8, 0.95, f.opaque)
        * (1.0 - smooth(3.0, 4.0, f.aspect))
        * (1.0 - 0.4 * smooth(2.0, 3.5, f.seam[0].max(f.seam[1])));
    // Caustic / flow pattern: thin bright lines (positive skew, line-like, networked) over a
    // smooth body (ridge height well above the body's grit).
    let pattern = smooth(0.35, 0.65, f.skew)
        * smooth(0.66, 0.73, f.ridge_line)
        * smooth(0.3, 0.42, f.ridge_net)
        * smooth(0.7, 0.85, f.ridge_spread)
        * smooth(5.5, 7.5, f.ridge_amp / f.grit.max(1e-4))
        // Soft light over depth, not white strokes on black (webs, sparks, light beams).
        * (1.0 - smooth(0.17, 0.23, f.l_std));
    let hue_in = |from: f32, to: f32| (f.hue - from).rem_euclid(360.0) <= (to - from);
    // Water: cyan to blue hues (jade to azure), or engine-tinted near-gray.
    let gray = 1.0 - smooth(0.02, 0.035, f.c_p90);
    let water_hue = if hue_in(145.0, 275.0) { 1.0 } else { 0.0 };
    let water = pattern * surface * gray.max(water_hue);
    // Lava: saturated red-orange with strong value contrast, bright veins (positive skew) in a
    // network over a darker crust.
    let lava_hue = if hue_in(18.0, 50.0) { 1.0 } else { 0.0 };
    let lava = lava_hue
        * surface
        * smooth(0.11, 0.13, f.c_p90)
        * smooth(0.08, 0.095, f.l_std)
        * smooth(0.05, 0.15, f.skew)
        * smooth(0.15, 0.25, f.ridge_net)
        * smooth(0.06, 0.1, f.dark);
    // Other liquids: the caustic pattern in any other clear color (poison, swamp, organic).
    // Reported only: without a hue prior the pattern also matches reliefs and ornaments, so
    // other liquids are confirmed by the pack map (`[[fluids]]`), never detected alone.
    let liquid = pattern * surface * (1.0 - gray) * (1.0 - water_hue) * (1.0 - lava_hue);
    let kind = if water.max(lava) < THRESHOLD {
        None
    } else if lava > water {
        Some(FluidKind::Lava)
    } else {
        Some(FluidKind::Water)
    };
    Detection {
        kind,
        pattern,
        water,
        lava,
        liquid,
        features: *f,
    }
}

/// Detects a fluid in an image of any size.
pub fn detect(image: &Image) -> Detection {
    score(&features(&thumbnail(image, THUMB)))
}
