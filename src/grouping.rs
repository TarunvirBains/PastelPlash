//! Value masses for soft value grouping (notan): a texture's 2–4 dominant lightness levels.
//!
//! The image (de-lit the way the GPU de-lights it) is box-averaged in OKLab onto a small grid,
//! smoothed edge-aware (a few bilateral iterations on lightness, never a plain blur, so a pale
//! lichen patch and a dark groove stay apart), and its lightness is clustered with 1D k-means.
//! The smallest mass count that explains enough of the variance wins; masses that are too close
//! or too small merge. The GPU pass then pulls each texel toward its soft-assigned mass.

use rayon::prelude::*;

use crate::analysis::Lowres;
use crate::color;
use crate::config::Grouping;
use crate::image::Image;

/// Longest side of the analysis grid.
const GRID: usize = 192;

/// The masses of one texture, darkest first.
#[derive(Debug, Clone, PartialEq)]
pub struct Masses {
    pub count: usize,
    /// Mass lightness (OKLab L).
    pub l: [f32; 4],
    /// Mass color (mean OKLab a/b of its members).
    pub ab: [[f32; 2]; 4],
    /// Share of the opaque texels in each mass.
    pub share: [f32; 4],
    /// Share of the lightness variance the masses explain.
    pub explained: f32,
    /// Share explained by the best two masses (graphic textures score near 1).
    pub explained2: f32,
}

/// Why a texture gets no grouping.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Skip {
    /// Too few opaque texels to analyze.
    Empty,
    /// One mass only: nothing to separate.
    Uniform,
    /// Already graphic (lettering, flat art): two masses explain nearly everything.
    Graphic(f32),
}

/// De-light parameters matching the GPU pass (strength, min gain, max gain).
pub struct DelightField<'a> {
    pub field: &'a Lowres,
    pub strength: f32,
    pub min_gain: f32,
    pub max_gain: f32,
}

/// Finds the value masses of `image`.
pub fn value_masses(
    image: &Image,
    delight: Option<&DelightField>,
    wrap: [bool; 2],
    g: &Grouping,
) -> Result<Masses, Skip> {
    let (w, h) = (image.width as usize, image.height as usize);
    let cell = (w.max(h).div_ceil(GRID)).max(1);
    let (gw, gh) = (w.div_ceil(cell), h.div_ceil(cell));
    let sub = (cell / 4).max(1);
    // Box-average (alpha-weighted) OKLab per cell; cells mostly transparent are left out.
    let cells: Vec<Option<[f32; 3]>> = (0..gw * gh)
        .into_par_iter()
        .map(|i| {
            let (cx, cy) = (i % gw, i / gw);
            let (mut s, mut ws, mut n) = ([0.0f32; 3], 0.0f32, 0.0f32);
            for y in (cy * cell..((cy + 1) * cell).min(h)).step_by(sub) {
                for x in (cx * cell..((cx + 1) * cell).min(w)).step_by(sub) {
                    let p = image.pixels[y * w + x];
                    n += 1.0;
                    if p[3] < 0.5 {
                        continue;
                    }
                    let mut lin = [p[0], p[1], p[2]].map(color::srgb_to_linear);
                    if let Some(d) = delight {
                        let yb = d.field.sample(
                            x as f32 + 0.5,
                            y as f32 + 0.5,
                            image.width,
                            image.height,
                            wrap,
                        );
                        let gain = (d.field.mean / yb.max(1e-4))
                            .powf(d.strength)
                            .clamp(d.min_gain, d.max_gain);
                        lin = lin.map(|v| v * gain);
                        let m = lin[0].max(lin[1]).max(lin[2]);
                        if m > 1.0 {
                            lin = lin.map(|v| v / m);
                        }
                    }
                    let lab = color::linear_to_oklab(lin);
                    for k in 0..3 {
                        s[k] += lab[k];
                    }
                    ws += 1.0;
                }
            }
            (ws >= 0.5 * n && ws > 0.0).then(|| s.map(|v| v / ws))
        })
        .collect();
    if cells.iter().flatten().count() < 16 {
        return Err(Skip::Empty);
    }

    // Edge-aware smoothing of lightness: three bilateral iterations over a 5×5 neighborhood.
    let mut l: Vec<f32> = cells.iter().map(|c| c.map_or(f32::NAN, |c| c[0])).collect();
    let sigma = g.range.max(1e-3);
    for _ in 0..3 {
        l = (0..gw * gh)
            .into_par_iter()
            .map(|i| {
                let l0 = l[i];
                if l0.is_nan() {
                    return l0;
                }
                let (x, y) = ((i % gw) as isize, (i / gw) as isize);
                let (mut s, mut ws) = (0.0, 0.0);
                for dy in -2..=2isize {
                    for dx in -2..=2isize {
                        let fetch = |v: isize, n: usize, wrap: bool| {
                            if wrap {
                                v.rem_euclid(n as isize) as usize
                            } else {
                                v.clamp(0, n as isize - 1) as usize
                            }
                        };
                        let q = fetch(y + dy, gh, wrap[1]) * gw + fetch(x + dx, gw, wrap[0]);
                        let lq = l[q];
                        if lq.is_nan() {
                            continue;
                        }
                        let d = (lq - l0) / sigma;
                        let wq = (-((dx * dx + dy * dy) as f32) / 4.5 - d * d).exp();
                        s += wq * lq;
                        ws += wq;
                    }
                }
                s / ws
            })
            .collect();
    }
    let members: Vec<(f32, [f32; 2])> = l
        .iter()
        .zip(&cells)
        .filter_map(|(&l, c)| c.map(|c| (l, [c[1], c[2]])))
        .collect();
    let mut values: Vec<f32> = members.iter().map(|m| m.0).collect();
    values.sort_by(f32::total_cmp);
    let mean = values.iter().sum::<f32>() / values.len() as f32;
    let total = values.iter().map(|v| (v - mean).powi(2)).sum::<f32>();
    // Lightness std below 0.01: one flat mass.
    if total <= 1e-4 * values.len() as f32 {
        return Err(Skip::Uniform);
    }

    let explained = |centers: &[f32]| {
        let within: f32 = values
            .iter()
            .map(|&v| {
                centers
                    .iter()
                    .map(|c| (v - c).powi(2))
                    .fold(f32::MAX, f32::min)
            })
            .sum();
        1.0 - within / total
    };
    // Already graphic? Judged on the unsmoothed lightness (smoothing would make any noisy
    // two-tone photo look clean).
    let mut raw: Vec<f32> = cells.iter().flatten().map(|c| c[0]).collect();
    raw.sort_by(f32::total_cmp);
    let raw_mean = raw.iter().sum::<f32>() / raw.len() as f32;
    let raw_total = raw.iter().map(|v| (v - raw_mean).powi(2)).sum::<f32>();
    let raw_two = kmeans(&raw, 2);
    let explained2 = 1.0
        - raw
            .iter()
            .map(|&v| {
                raw_two
                    .iter()
                    .map(|c| (v - c).powi(2))
                    .fold(f32::MAX, f32::min)
            })
            .sum::<f32>()
            / raw_total.max(1e-12);
    if explained2 >= g.skip_explained {
        return Err(Skip::Graphic(explained2));
    }
    let two = kmeans(&values, 2);
    let max_k = (g.max_masses as usize).clamp(2, 4);
    let mut centers = two;
    for k in 3..=max_k {
        if explained(&centers) >= g.explained {
            break;
        }
        centers = kmeans(&values, k);
    }

    // Merge masses that are too close or too small into their nearest neighbor.
    loop {
        let shares = shares(&values, &centers);
        let n = centers.len();
        if n < 2 {
            break;
        }
        let gap = (0..n - 1)
            .map(|i| (centers[i + 1] - centers[i], i))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .unwrap();
        let small = (0..n)
            .map(|i| (shares[i], i))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .unwrap();
        let pair = if gap.0 < g.min_gap {
            Some((gap.1, gap.1 + 1))
        } else if small.0 < g.min_share {
            let i = small.1;
            let j = if i == 0 {
                1
            } else if i == n - 1 || centers[i] - centers[i - 1] < centers[i + 1] - centers[i] {
                i - 1
            } else {
                i + 1
            };
            Some((i.min(j), i.max(j)))
        } else {
            None
        };
        let Some((a, b)) = pair else { break };
        let (sa, sb) = (shares[a].max(1e-6), shares[b].max(1e-6));
        centers[a] = (centers[a] * sa + centers[b] * sb) / (sa + sb);
        centers.remove(b);
    }
    if centers.len() < 2 {
        return Err(Skip::Uniform);
    }

    let mut out = Masses {
        count: centers.len(),
        l: [0.0; 4],
        ab: [[0.0; 2]; 4],
        share: [0.0; 4],
        explained: explained(&centers),
        explained2,
    };
    let mut sums = [[0.0f32; 3]; 4];
    for &(v, ab) in &members {
        let k = nearest(&centers, v);
        sums[k][0] += ab[0];
        sums[k][1] += ab[1];
        sums[k][2] += 1.0;
    }
    for (k, &c) in centers.iter().enumerate() {
        out.l[k] = c;
        let n = sums[k][2].max(1.0);
        out.ab[k] = [sums[k][0] / n, sums[k][1] / n];
        out.share[k] = sums[k][2] / members.len() as f32;
    }
    Ok(out)
}

fn nearest(centers: &[f32], v: f32) -> usize {
    (0..centers.len())
        .min_by(|&a, &b| (v - centers[a]).abs().total_cmp(&(v - centers[b]).abs()))
        .unwrap_or(0)
}

fn shares(values: &[f32], centers: &[f32]) -> Vec<f32> {
    let mut n = vec![0.0f32; centers.len()];
    for &v in values {
        n[nearest(centers, v)] += 1.0;
    }
    n.iter().map(|c| c / values.len() as f32).collect()
}

/// 1D k-means on sorted `values`, initialized at quantiles; centers ascending.
fn kmeans(values: &[f32], k: usize) -> Vec<f32> {
    let n = values.len();
    let mut c: Vec<f32> = (0..k)
        .map(|i| values[(((i as f32 + 0.5) / k as f32) * n as f32) as usize % n])
        .collect();
    for _ in 0..30 {
        // Sorted values: cluster boundaries are midpoints between adjacent centers.
        let mut sum = vec![0.0f64; k];
        let mut cnt = vec![0usize; k];
        let mut j = 0;
        for &v in values {
            while j + 1 < k && v > 0.5 * (c[j] + c[j + 1]) {
                j += 1;
            }
            sum[j] += v as f64;
            cnt[j] += 1;
        }
        let mut moved = 0.0f32;
        for i in 0..k {
            if cnt[i] > 0 {
                let m = (sum[i] / cnt[i] as f64) as f32;
                moved = moved.max((m - c[i]).abs());
                c[i] = m;
            }
        }
        c.sort_by(f32::total_cmp);
        if moved < 1e-5 {
            break;
        }
    }
    c.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_images::{image, noise};

    #[test]
    fn noisy_two_level_texture_has_two_masses() {
        let img = image(256, 256, |x, y| {
            let base = if (x / 64 + y / 64) % 2 == 0 {
                0.25
            } else {
                0.7
            };
            let v = base + 0.6 * (noise(x, y) - 0.5);
            [v, v * 0.95, v * 0.85, 1.0]
        });
        let m = value_masses(&img, None, [false; 2], &Grouping::default()).unwrap();
        assert_eq!(m.count, 2, "{m:?}");
        assert!(m.l[1] - m.l[0] > 0.2, "{m:?}");
    }

    #[test]
    fn clean_two_tone_art_is_graphic() {
        let img = image(128, 128, |x, _| {
            let v = if (x / 16) % 2 == 0 { 0.2 } else { 0.8 };
            [v, v, v, 1.0]
        });
        assert!(matches!(
            value_masses(&img, None, [false; 2], &Grouping::default()),
            Err(Skip::Graphic(_))
        ));
    }

    #[test]
    fn flat_texture_is_uniform() {
        let img = image(64, 64, |_, _| [0.5, 0.4, 0.3, 1.0]);
        assert_eq!(
            value_masses(&img, None, [false; 2], &Grouping::default()),
            Err(Skip::Uniform)
        );
    }
}
