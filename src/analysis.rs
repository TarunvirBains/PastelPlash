//! CPU-side per-image analysis that parameterizes the GPU passes: seamless-tiling detection,
//! tint-safe (grayscale-origin) detection, and the low-resolution luminance field used for
//! de-lighting.

use rayon::prelude::*;

use crate::color;
use crate::image::Image;

/// Sampling stride so an analysis touches at most about `budget` pixels.
fn stride(image: &Image, budget: f64) -> usize {
    let n = image.width as f64 * image.height as f64;
    ((n / budget).sqrt().ceil() as usize).max(1)
}

fn px(image: &Image, x: usize, y: usize) -> [f32; 4] {
    image.pixels[y * image.width as usize + x]
}

/// Premultiplied distance between two pixels (mean absolute difference over RGBA).
fn diff(a: [f32; 4], b: [f32; 4]) -> f32 {
    let pa = [a[0] * a[3], a[1] * a[3], a[2] * a[3], a[3]];
    let pb = [b[0] * b[3], b[1] * b[3], b[2] * b[3], b[3]];
    (0..4).map(|k| (pa[k] - pb[k]).abs()).sum::<f32>() / 4.0
}

/// Seam-to-interior discontinuity ratio along one axis. `transpose` measures the vertical seam
/// (top vs. bottom row) instead of the horizontal one.
///
/// Both single texels and `k`-wide strip averages are compared, and the worse ratio wins: noisy
/// textures have large per-texel differences everywhere, so only the strip averages reveal a
/// low-frequency jump at a non-matching edge.
pub fn seam_ratio(image: &Image, transpose: bool) -> f32 {
    let (w, h) = (image.width as usize, image.height as usize);
    let (len, across) = if transpose { (h, w) } else { (w, h) };
    if len < 4 || across == 0 {
        return f32::INFINITY;
    }
    let get = |i: usize, j: usize| {
        if transpose {
            px(image, j, i)
        } else {
            px(image, i, j)
        }
    };
    let k = (len / 128).clamp(1, 16);
    let strip = |start: usize, j: usize| -> [f32; 4] {
        let mut s = [0.0; 4];
        for d in 0..k {
            let p = get((start + d) % len, j);
            for c in 0..4 {
                s[c] += p[c];
            }
        }
        s.map(|v| v / k as f32)
    };
    let step = (across / 512).max(1);
    let rows: Vec<usize> = (0..across).step_by(step).collect();
    let (seam1, inner1, seamk, innerk) = rows
        .par_iter()
        .map(|&j| {
            let seam1 = diff(get(len - 1, j), get(0, j));
            let mut inner1 = 0.0;
            for i in 0..len - 1 {
                inner1 += diff(get(i, j), get(i + 1, j));
            }
            inner1 /= (len - 1) as f32;
            let seamk = diff(strip(len - k, j), strip(0, j));
            let mut innerk = 0.0;
            let mut n = 0;
            for i in (0..len - 2 * k).step_by(k) {
                innerk += diff(strip(i, j), strip(i + k, j));
                n += 1;
            }
            innerk /= n.max(1) as f32;
            (seam1, inner1, seamk, innerk)
        })
        .reduce(
            || (0.0, 0.0, 0.0, 0.0),
            |a, b| (a.0 + b.0, a.1 + b.1, a.2 + b.2, a.3 + b.3),
        );
    let eps = 1e-3 * rows.len() as f32;
    ((seam1 + eps) / (inner1 + eps)).max((seamk + eps) / (innerk + eps))
}

/// Low-resolution alpha-weighted linear luminance, blurred, for de-lighting.
#[derive(Debug, Clone)]
pub struct Lowres {
    pub width: u32,
    pub height: u32,
    pub data: Vec<f32>,
    /// Alpha-weighted mean of the blurred field.
    pub mean: f32,
}

fn luminance(p: [f32; 4]) -> f32 {
    let [r, g, b] = [p[0], p[1], p[2]].map(color::srgb_to_linear);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Box-averages linear luminance into cells of about `gm / 64` texels, fills empty
/// (transparent) cells with the mean, then Gaussian-blurs with `sigma` (in texels), wrapping each
/// axis that tiles.
pub fn lowres_luminance(image: &Image, sigma: f32, wrap: [bool; 2]) -> Lowres {
    let (w, h) = (image.width as usize, image.height as usize);
    let gm = ((w * h) as f32).sqrt();
    let cell = ((gm / 64.0).round() as usize).max(1);
    let (lw, lh) = (w.div_ceil(cell), h.div_ceil(cell));
    let sums: Vec<(f32, f32)> = (0..lw * lh)
        .into_par_iter()
        .map(|i| {
            let (cx, cy) = (i % lw, i / lw);
            let (mut s, mut ws) = (0.0, 0.0);
            let sub = (cell / 8).max(1); // subsample large cells
            for y in (cy * cell..((cy + 1) * cell).min(h)).step_by(sub) {
                for x in (cx * cell..((cx + 1) * cell).min(w)).step_by(sub) {
                    let p = px(image, x, y);
                    s += luminance(p) * p[3];
                    ws += p[3];
                }
            }
            (s, ws)
        })
        .collect();
    let total_w: f32 = sums.iter().map(|s| s.1).sum();
    let global = if total_w > 0.0 {
        sums.iter().map(|s| s.0).sum::<f32>() / total_w
    } else {
        0.5
    };
    let mut field: Vec<f32> = sums
        .iter()
        .map(|&(s, ws)| {
            // Blend sparse cells toward the global mean so a few edge texels don't dominate.
            let full = (cell * cell) as f32 / ((cell / 8).max(1).pow(2)) as f32;
            let t = (ws / (0.25 * full)).min(1.0);
            let v = if ws > 0.0 { s / ws } else { global };
            global + (v - global) * t
        })
        .collect();

    let sigma = sigma / cell as f32;
    if sigma > 0.3 {
        let radius = (sigma * 3.0).ceil() as isize;
        let kernel: Vec<f32> = (-radius..=radius)
            .map(|d| (-(d * d) as f32 / (2.0 * sigma * sigma)).exp())
            .collect();
        let blur = |src: &[f32], horizontal: bool| -> Vec<f32> {
            let (n, wrap) = if horizontal {
                (lw, wrap[0])
            } else {
                (lh, wrap[1])
            };
            (0..lw * lh)
                .map(|i| {
                    let (x, y) = (i % lw, i / lw);
                    let pos = if horizontal { x } else { y } as isize;
                    let (mut s, mut ws) = (0.0, 0.0);
                    for (ki, d) in (-radius..=radius).enumerate() {
                        let q = pos + d;
                        let q = if wrap {
                            q.rem_euclid(n as isize)
                        } else {
                            q.clamp(0, n as isize - 1)
                        } as usize;
                        let idx = if horizontal { y * lw + q } else { q * lw + x };
                        s += src[idx] * kernel[ki];
                        ws += kernel[ki];
                    }
                    s / ws
                })
                .collect()
        };
        field = blur(&blur(&field, true), false);
    }
    let mean = if total_w > 0.0 {
        field.iter().zip(&sums).map(|(v, s)| v * s.1).sum::<f32>() / total_w
    } else {
        global
    };
    Lowres {
        width: lw as u32,
        height: lh as u32,
        data: field,
        mean,
    }
}

impl Lowres {
    /// Bilinear lookup at image texel center `(x, y)` (same math as the shader).
    pub fn sample(&self, x: f32, y: f32, img_w: u32, img_h: u32, wrap: [bool; 2]) -> f32 {
        let (lw, lh) = (self.width as i64, self.height as i64);
        let u = x / img_w as f32 * lw as f32 - 0.5;
        let v = y / img_h as f32 * lh as f32 - 0.5;
        let (x0, y0) = (u.floor(), v.floor());
        let (tx, ty) = (u - x0, v - y0);
        let at = |i: i64, j: i64| {
            let i = if wrap[0] {
                i.rem_euclid(lw)
            } else {
                i.clamp(0, lw - 1)
            };
            let j = if wrap[1] {
                j.rem_euclid(lh)
            } else {
                j.clamp(0, lh - 1)
            };
            self.data[(j * lw + i) as usize]
        };
        let (x0, y0) = (x0 as i64, y0 as i64);
        let top = at(x0, y0) * (1.0 - tx) + at(x0 + 1, y0) * tx;
        let bot = at(x0, y0 + 1) * (1.0 - tx) + at(x0 + 1, y0 + 1) * tx;
        top * (1.0 - ty) + bot * ty
    }
}

/// Median OKLab lightness standard deviation over square windows of `radius` texels, centred on
/// a grid of about 600 opaque texels: the texture's value spread at that scale. Windows are
/// sampled on a sub-grid, so the cost is independent of the radius.
pub fn local_l_std(image: &Image, radius: f32, wrap: [bool; 2]) -> f32 {
    let (w, h) = (image.width as isize, image.height as isize);
    let r = radius.round().max(1.0) as isize;
    let sub = (r / 6).max(1);
    let at = |x: isize, y: isize| {
        let x = if wrap[0] {
            x.rem_euclid(w)
        } else {
            x.clamp(0, w - 1)
        };
        let y = if wrap[1] {
            y.rem_euclid(h)
        } else {
            y.clamp(0, h - 1)
        };
        image.pixels[(y * w + x) as usize]
    };
    let step = (((w * h) as f32 / 600.0).sqrt().max(1.0)) as isize;
    let centres: Vec<(isize, isize)> = (0..h)
        .step_by(step as usize)
        .flat_map(|y| (0..w).step_by(step as usize).map(move |x| (x, y)))
        .filter(|&(x, y)| at(x, y)[3] >= 0.5)
        .collect();
    let mut stds: Vec<f32> = centres
        .par_iter()
        .map(|&(cx, cy)| {
            let (mut s, mut s2, mut n) = (0.0f64, 0.0f64, 0.0f64);
            for y in (cy - r..=cy + r).step_by(sub as usize) {
                for x in (cx - r..=cx + r).step_by(sub as usize) {
                    let p = at(x, y);
                    if p[3] < 0.5 {
                        continue;
                    }
                    let l = color::srgb_to_oklab([p[0], p[1], p[2]])[0] as f64;
                    s += l;
                    s2 += l * l;
                    n += 1.0;
                }
            }
            let m = s / n.max(1.0);
            ((s2 / n.max(1.0) - m * m).max(0.0)).sqrt() as f32
        })
        .collect();
    if stds.is_empty() {
        return 0.0;
    }
    let i = stds.len() / 2;
    *stds.select_nth_unstable_by(i, f32::total_cmp).1
}

/// 99th-percentile OKLab chroma over the opaque texels (tint-safe detection).
pub fn chroma_p99(image: &Image) -> f32 {
    let (w, h) = (image.width as usize, image.height as usize);
    let s = stride(image, 1.0e6);
    let mut chroma: Vec<f32> = (0..h)
        .into_par_iter()
        .step_by(s)
        .flat_map_iter(|y| {
            (0..w).step_by(s).filter_map(move |x| {
                let p = px(image, x, y);
                (p[3] >= 0.5).then(|| {
                    let lab = color::srgb_to_oklab([p[0], p[1], p[2]]);
                    lab[1].hypot(lab[2])
                })
            })
        })
        .collect();
    if chroma.is_empty() {
        return 0.0;
    }
    let i = ((chroma.len() - 1) as f32 * 0.99).round() as usize;
    *chroma.select_nth_unstable_by(i, f32::total_cmp).1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::{SourceColor, SourceFormat};

    fn image(w: u32, h: u32, f: impl Fn(u32, u32) -> [f32; 4]) -> Image {
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

    fn noise(x: u32, y: u32) -> f32 {
        let mut v = x.wrapping_mul(0x9E37_79B9) ^ y.wrapping_mul(0x85EB_CA6B);
        v ^= v >> 15;
        v = v.wrapping_mul(0x2C1B_3C6D);
        v ^= v >> 12;
        (v & 0xFFFF) as f32 / 65535.0
    }

    #[test]
    fn periodic_image_tiles_on_both_axes() {
        let tau = std::f32::consts::TAU;
        let img = image(128, 64, |x, y| {
            let v = 0.5
                + 0.25 * (tau * x as f32 / 128.0 * 3.0).sin()
                + 0.2 * (tau * y as f32 / 64.0 * 2.0).cos();
            [v, v * 0.8, 0.3, 1.0]
        });
        assert!(seam_ratio(&img, false) < 2.0);
        assert!(seam_ratio(&img, true) < 2.0);
    }

    #[test]
    fn gradient_does_not_tile() {
        let img = image(128, 64, |x, _| {
            let v = x as f32 / 127.0;
            [v, v, v, 1.0]
        });
        assert!(seam_ratio(&img, false) > 10.0);
        // Constant along y ⇒ the vertical seam is perfect.
        assert!(seam_ratio(&img, true) < 2.0);
    }

    #[test]
    fn noisy_image_with_offset_edges_does_not_tile() {
        // Per-texel noise hides a low-frequency jump from a texel-level metric.
        let img = image(256, 256, |x, y| {
            let v = 0.3 + 0.3 * noise(x, y) + 0.3 * (x as f32 / 255.0);
            [v, v, v, 1.0]
        });
        assert!(seam_ratio(&img, false) > 2.5);
    }

    #[test]
    fn lowres_of_flat_image_is_flat() {
        let img = image(200, 100, |_, _| [0.5, 0.5, 0.5, 1.0]);
        let lr = lowres_luminance(&img, 12.0, [false, false]);
        let y = color::srgb_to_linear(0.5);
        assert!(lr.data.iter().all(|v| (v - y).abs() < 1e-4));
        assert!((lr.mean - y).abs() < 1e-4);
        assert!((lr.sample(3.0, 97.0, 200, 100, [true, false]) - y).abs() < 1e-4);
    }

    #[test]
    fn gray_image_has_low_chroma() {
        let img = image(64, 64, |x, y| {
            let v = noise(x, y);
            [v, v, v, 1.0]
        });
        assert!(chroma_p99(&img) < 1e-3);
        let green = image(8, 8, |_, _| [0.2, 0.6, 0.1, 1.0]);
        assert!(chroma_p99(&green) > 0.1);
    }
}
