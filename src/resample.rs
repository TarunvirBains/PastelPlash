//! Integer-factor resampling for output resolution floors: a Lanczos-3 upsample into the
//! pipeline and an area-average downsample out of it.
//!
//! Both are alpha-aware: color is weighted by alpha (a transparent texel's color never bleeds
//! into a visible one), and where the result is (nearly) fully transparent the unweighted color
//! is kept, so cutout borders keep the colors the game's filtering sees. The upsample clamps each
//! pass to the two nearest source texels (no ringing: a hard edge gets no halo or overshoot), and
//! addresses by wrapping on tiling axes (a seamless texture stays seamless) or by clamping.

use rayon::prelude::*;

use crate::image::Image;

/// Lanczos kernel radius (taps per side).
const A: i32 = 3;

fn lanczos(x: f32) -> f32 {
    let x = x.abs();
    if x < 1e-6 {
        return 1.0;
    }
    if x >= A as f32 {
        return 0.0;
    }
    let px = std::f32::consts::PI * x;
    A as f32 * px.sin() * (px / A as f32).sin() / (px * px)
}

/// One output phase of an integer upsample: the first tap's offset from `o / k`, the normalized
/// weights, and the offset of the nearer-left source texel (the anti-ringing pair is it and the
/// next one).
struct Phase {
    first: i32,
    weights: [f32; 2 * A as usize],
    near: i32,
}

fn phases(k: u32) -> Vec<Phase> {
    (0..k)
        .map(|p| {
            // Output texel center in source texel coordinates, relative to `o / k`.
            let s = (p as f32 + 0.5) / k as f32 - 0.5;
            let near = s.floor() as i32;
            let first = near - A + 1;
            let mut weights = [0.0f32; 2 * A as usize];
            for (t, w) in weights.iter_mut().enumerate() {
                *w = lanczos(s - (first + t as i32) as f32);
            }
            let sum: f32 = weights.iter().sum();
            for w in &mut weights {
                *w /= sum;
            }
            Phase {
                first,
                weights,
                near,
            }
        })
        .collect()
}

fn address(i: i32, n: u32, wrap: bool) -> usize {
    if wrap {
        i.rem_euclid(n as i32) as usize
    } else {
        i.clamp(0, n as i32 - 1) as usize
    }
}

/// One separable pass over `c` channels per texel: `src` is `n` texels along the resampled axis
/// at `stride` apart (in texels), `dst` receives `n·k` texels at `dst_stride`.
#[allow(clippy::too_many_arguments)]
fn pass_1d(
    src: &[f32],
    stride: usize,
    n: u32,
    dst: &mut [f32],
    dst_stride: usize,
    c: usize,
    k: u32,
    phases: &[Phase],
    wrap: bool,
) {
    for o in 0..n * k {
        let ph = &phases[(o % k) as usize];
        let base = (o / k) as i32;
        let out = &mut dst[o as usize * dst_stride * c..][..c];
        out.fill(0.0);
        for (t, &w) in ph.weights.iter().enumerate() {
            let i = address(base + ph.first + t as i32, n, wrap);
            let s = &src[i * stride * c..][..c];
            for ch in 0..c {
                out[ch] += w * s[ch];
            }
        }
        let a = &src[address(base + ph.near, n, wrap) * stride * c..][..c];
        let b = &src[address(base + ph.near + 1, n, wrap) * stride * c..][..c];
        for ch in 0..c {
            out[ch] = out[ch].clamp(a[ch].min(b[ch]), a[ch].max(b[ch]));
        }
    }
}

/// Enlarges `image` by an integer factor `k` with a Lanczos-3 filter (see the module docs).
/// `wrap` gives the axes that tile (x, y).
pub fn upsample(image: &Image, k: u32, wrap: [bool; 2]) -> Image {
    assert!(k >= 1, "upsample factor must be at least 1");
    if k == 1 {
        return image.clone();
    }
    let (w, h) = (image.width, image.height);
    let (ow, oh) = (w * k, h * k);
    // Channels: straight color (3) and alpha; with transparency also color × alpha (3).
    let opaque = image.pixels.iter().all(|p| p[3] >= 1.0);
    let c = if opaque { 4 } else { 7 };
    let src: Vec<f32> = image
        .pixels
        .iter()
        .flat_map(|p| {
            let mut v = [p[0], p[1], p[2], p[3], 0.0, 0.0, 0.0];
            if !opaque {
                for ch in 0..3 {
                    v[4 + ch] = p[ch] * p[3];
                }
            }
            v.into_iter().take(c)
        })
        .collect();
    let ph = phases(k);
    // Horizontal: each source row → an output-width row.
    let mut horiz = vec![0.0f32; (ow * h) as usize * c];
    horiz
        .par_chunks_mut(ow as usize * c)
        .enumerate()
        .for_each(|(y, row)| {
            let s = &src[y * w as usize * c..][..w as usize * c];
            pass_1d(s, 1, w, row, 1, c, k, &ph, wrap[0]);
        });
    // Vertical: each output row from the horizontal rows around it.
    let mut pixels = vec![[0.0f32; 4]; (ow * oh) as usize];
    pixels
        .par_chunks_mut(ow as usize)
        .enumerate()
        .for_each(|(oy, row)| {
            let p = &ph[oy % k as usize];
            let base = (oy / k as usize) as i32;
            let taps: Vec<(usize, f32)> = p
                .weights
                .iter()
                .enumerate()
                .map(|(t, &wt)| (address(base + p.first + t as i32, h, wrap[1]), wt))
                .collect();
            let (na, nb) = (
                address(base + p.near, h, wrap[1]),
                address(base + p.near + 1, h, wrap[1]),
            );
            let mut v = [0.0f32; 7];
            for (x, out) in row.iter_mut().enumerate() {
                let at = |yy: usize| &horiz[(yy * ow as usize + x) * c..][..c];
                v[..c].fill(0.0);
                for &(yy, wt) in &taps {
                    let s = at(yy);
                    for ch in 0..c {
                        v[ch] += wt * s[ch];
                    }
                }
                let (a, b) = (at(na), at(nb));
                for ch in 0..c {
                    v[ch] = v[ch].clamp(a[ch].min(b[ch]), a[ch].max(b[ch]));
                }
                let alpha = v[3].clamp(0.0, 1.0);
                let rgb = if opaque || alpha < 1.0 / 255.0 {
                    [v[0], v[1], v[2]]
                } else {
                    [v[4] / v[3], v[5] / v[3], v[6] / v[3]]
                };
                *out = [
                    rgb[0].clamp(0.0, 1.0),
                    rgb[1].clamp(0.0, 1.0),
                    rgb[2].clamp(0.0, 1.0),
                    alpha,
                ];
            }
        });
    Image {
        width: ow,
        height: oh,
        pixels,
        ..image.clone_meta()
    }
}

/// Shrinks `image` by an integer factor `k` (both sides must be multiples of it): each output
/// texel is the mean of its `k`×`k` block, color weighted by alpha.
pub fn downsample(image: &Image, k: u32) -> Image {
    assert!(
        k >= 1 && image.width.is_multiple_of(k) && image.height.is_multiple_of(k),
        "downsample factor {k} must divide {}x{}",
        image.width,
        image.height
    );
    if k == 1 {
        return image.clone();
    }
    let (w, ow, oh) = (image.width as usize, image.width / k, image.height / k);
    let k = k as usize;
    let n = (k * k) as f32;
    let mut pixels = vec![[0.0f32; 4]; (ow * oh) as usize];
    pixels
        .par_chunks_mut(ow as usize)
        .enumerate()
        .for_each(|(oy, row)| {
            for (ox, out) in row.iter_mut().enumerate() {
                let (mut plain, mut weighted, mut sa) = ([0.0f32; 3], [0.0f32; 3], 0.0f32);
                for y in oy * k..(oy + 1) * k {
                    for x in ox * k..(ox + 1) * k {
                        let p = image.pixels[y * w + x];
                        for ch in 0..3 {
                            plain[ch] += p[ch];
                            weighted[ch] += p[ch] * p[3];
                        }
                        sa += p[3];
                    }
                }
                let a = sa / n;
                let rgb = if sa >= n || a < 1.0 / 255.0 {
                    plain.map(|v| v / n)
                } else {
                    weighted.map(|v| v / sa)
                };
                *out = [rgb[0], rgb[1], rgb[2], a];
            }
        });
    Image {
        width: ow,
        height: oh,
        pixels,
        ..image.clone_meta()
    }
}

impl Image {
    /// An empty image with this one's metadata (source format, scale, tint safety).
    fn clone_meta(&self) -> Image {
        Image {
            width: 0,
            height: 0,
            pixels: Vec::new(),
            source: self.source,
            source_scale: self.source_scale,
            tint_safe: self.tint_safe,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::{SourceColor, SourceFormat};

    fn img(w: u32, h: u32, f: impl Fn(u32, u32) -> [f32; 4]) -> Image {
        Image {
            width: w,
            height: h,
            pixels: (0..w * h).map(|i| f(i % w, i / w)).collect(),
            source: SourceFormat {
                color: SourceColor::Rgba,
                bit_depth: 8,
                has_alpha: true,
            },
            source_scale: None,
            tint_safe: None,
        }
    }

    #[test]
    fn factor_one_is_identity() {
        let a = img(5, 3, |x, y| [x as f32 / 5.0, y as f32 / 3.0, 0.5, 1.0]);
        assert_eq!(upsample(&a, 1, [false; 2]), a);
        assert_eq!(downsample(&a, 1), a);
    }

    #[test]
    fn flat_stays_flat_and_sizes_multiply() {
        let a = img(6, 4, |_, _| [0.3, 0.5, 0.7, 1.0]);
        let u = upsample(&a, 4, [false; 2]);
        assert_eq!((u.width, u.height), (24, 16));
        for p in &u.pixels {
            for c in 0..4 {
                assert!((p[c] - a.pixels[0][c]).abs() < 1e-5, "{p:?}");
            }
        }
        let d = downsample(&u, 4);
        assert_eq!((d.width, d.height), (6, 4));
    }

    #[test]
    fn hard_edges_do_not_ring() {
        // A step from dark to light: Lanczos alone overshoots on both sides of it.
        let a = img(16, 4, |x, _| {
            let v = if x < 8 { 0.1 } else { 0.9 };
            [v, v, v, 1.0]
        });
        let u = upsample(&a, 4, [false; 2]);
        for p in &u.pixels {
            assert!((0.1 - 1e-6..=0.9 + 1e-6).contains(&p[0]), "overshoot {p:?}");
        }
        // ... and the step is smooth: intermediate values between the two plateaus.
        assert!(u.pixels.iter().any(|p| p[0] > 0.2 && p[0] < 0.8));
    }

    #[test]
    fn transparent_color_does_not_bleed() {
        // Red opaque half next to a transparent half whose (unseen) color is green.
        let a = img(8, 2, |x, _| {
            if x < 4 {
                [1.0, 0.0, 0.0, 1.0]
            } else {
                [0.0, 1.0, 0.0, 0.0]
            }
        });
        let u = upsample(&a, 4, [false; 2]);
        for p in u.pixels.iter().filter(|p| p[3] >= 1.0 / 255.0) {
            assert!(p[1] < 1e-4 && p[0] > 0.999, "green bled into {p:?}");
        }
        // Fully transparent texels keep the unweighted color.
        assert!(u.pixels.iter().any(|p| p[3] == 0.0 && p[1] > 0.99));
    }

    #[test]
    fn wrapping_axes_stay_seamless() {
        // A horizontal ramp that tiles: the first and last output columns continue each other.
        let a = img(8, 2, |x, _| {
            let v = 0.5 + 0.4 * (x as f32 / 8.0 * std::f32::consts::TAU).sin();
            [v, v, v, 1.0]
        });
        let u = upsample(&a, 4, [true, false]);
        let (first, last) = (u.pixels[0][0], u.pixels[u.width as usize - 1][0]);
        let step = (u.pixels[1][0] - first).abs();
        assert!(
            (first - last).abs() <= 2.0 * step + 1e-4,
            "seam {first} vs {last}"
        );
    }

    #[test]
    fn downsample_averages_blocks_weighted_by_alpha() {
        let a = img(2, 2, |x, y| match (x, y) {
            (0, 0) => [1.0, 0.0, 0.0, 1.0],
            (1, 0) => [0.0, 0.0, 1.0, 1.0],
            _ => [0.0, 1.0, 0.0, 0.0],
        });
        let d = downsample(&a, 2);
        let p = d.pixels[0];
        assert!((p[0] - 0.5).abs() < 1e-6 && p[1] < 1e-6 && (p[2] - 0.5).abs() < 1e-6);
        assert!((p[3] - 0.5).abs() < 1e-6);
    }
}
