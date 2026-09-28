//! `palette-report`: per hue group OKLCH statistics of a folder of PNGs next to a reference
//! palette (e.g. `reference/ss-lit.toml`), to check palette fidelity numerically.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rayon::prelude::*;
use serde::Deserialize;

use crate::color;
use crate::png_io;
use crate::walk::{self, WalkOptions};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub name: String,
    pub groups: Vec<RefGroup>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefGroup {
    pub name: String,
    pub hue_range: [f32; 2],
    /// Median OKLCH lightness.
    pub l: f32,
    /// Median chroma.
    pub c: f32,
    /// 90th-percentile chroma.
    pub c_p90: f32,
    /// Median hue.
    pub h: f32,
}

impl Reference {
    pub fn load(path: &Path) -> Result<Self> {
        crate::config::load(path)
    }

    /// Index of the group whose hue range contains `h`.
    pub fn group_of(&self, h: f32) -> Option<usize> {
        self.groups.iter().position(|g| {
            let [from, to] = g.hue_range;
            (h - from).rem_euclid(360.0) < (to - from).rem_euclid(360.0)
        })
    }
}

/// Chroma below which a texel counts as neutral (not attributed to a hue group).
pub const NEUTRAL_C: f32 = 0.02;

fn median(v: &mut [f32]) -> f32 {
    if v.is_empty() {
        return f32::NAN;
    }
    let i = v.len() / 2;
    *v.select_nth_unstable_by(i, f32::total_cmp).1
}

fn pct(v: &mut [f32], q: f32) -> f32 {
    if v.is_empty() {
        return f32::NAN;
    }
    let i = ((v.len() - 1) as f32 * q).round() as usize;
    *v.select_nth_unstable_by(i, f32::total_cmp).1
}

/// OKLCH samples (L, C, h) of the opaque texels of every PNG under `dir`, up to about
/// `per_image` per file.
fn samples(dir: &Path, per_image: usize) -> Result<Vec<[f32; 3]>> {
    let opts = WalkOptions {
        recursive: true,
        follow_links: false,
        exclude: None,
    };
    let files: Vec<PathBuf> = walk::walk(dir, &opts)
        .with_context(|| format!("reading {}", dir.display()))?
        .entries
        .into_iter()
        .filter(|e| e.is_png)
        .map(|e| dir.join(e.rel))
        .collect();
    let per_file: Vec<Vec<[f32; 3]>> = files
        .par_iter()
        .map(|f| -> Result<Vec<[f32; 3]>> {
            let img = png_io::read(f)?;
            let step = (img.pixels.len() / per_image.max(1)).max(1);
            Ok(img
                .pixels
                .iter()
                .step_by(step)
                .filter(|p| p[3] >= 0.5)
                .map(|p| color::oklab_to_oklch(color::srgb_to_oklab([p[0], p[1], p[2]])))
                .collect())
        })
        .collect::<Result<_>>()?;
    Ok(per_file.into_iter().flatten().collect())
}

/// Alpha-weighted mean OKLab color of an image.
pub fn mean_oklab(img: &crate::image::Image) -> [f32; 3] {
    let (mut s, mut w) = ([0.0f64; 3], 0.0f64);
    for p in &img.pixels {
        let lab = color::srgb_to_oklab([p[0], p[1], p[2]]);
        for k in 0..3 {
            s[k] += lab[k] as f64 * p[3] as f64;
        }
        w += p[3] as f64;
    }
    if w <= 0.0 {
        return [0.0; 3];
    }
    s.map(|v| (v / w) as f32)
}

/// Which part of the color difference [`coarse_delta_e`] measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoarsePart {
    /// Chromatic difference (OKLab a/b distance): hue and chroma transformations.
    Color,
    /// Lightness difference.
    Lightness,
}

/// Coarse-scale identity: both images are divided into a `cells`×`cells` grid; in each cell the
/// opaque texels are split at the cell's median lightness, and the mean OKLab color of the dark
/// half and of the light half are compared between source and output (cell value = the larger
/// of the two differences, in chroma or in lightness per `part`). Returns (median, 90th
/// percentile, max) over cells with enough opaque texels.
///
/// Brushwork only moves texels around within a cell, so the two half-means barely change; a hue
/// or value transformation (dark grooves turned navy, a floor turned tan) changes one of them
/// even when the cell's overall mean stays similar. Measuring chroma apart from lightness keeps
/// intended value changes (lifted darks, compression) from masking hue changes.
pub fn coarse_delta_e(
    a: &crate::image::Image,
    b: &crate::image::Image,
    cells: u32,
    part: CoarsePart,
) -> (f32, f32, f32) {
    let n = cells as usize;
    let (w, h) = (a.width as usize, a.height as usize);
    let step = ((w * h / (n * n * 400)).max(1) as f64).sqrt().ceil() as usize;
    let half_means = |img: &crate::image::Image, cy: usize, cx: usize| -> Option<[[f32; 3]; 2]> {
        let mut v: Vec<[f32; 3]> = Vec::new();
        for y in (cy * h / n..(cy + 1) * h / n).step_by(step) {
            for x in (cx * w / n..(cx + 1) * w / n).step_by(step) {
                let p = img.pixels[y * w + x];
                if p[3] >= 0.5 {
                    v.push(color::srgb_to_oklab([p[0], p[1], p[2]]));
                }
            }
        }
        let total = ((h / n) * (w / n)).div_ceil(step * step);
        if v.len() * 4 < total || v.len() < 4 {
            return None;
        }
        v.sort_by(|p, q| p[0].total_cmp(&q[0]));
        let mean = |s: &[[f32; 3]]| {
            let mut m = [0.0f32; 3];
            for p in s {
                for k in 0..3 {
                    m[k] += p[k] / s.len() as f32;
                }
            }
            m
        };
        let mid = v.len() / 2;
        Some([mean(&v[..mid]), mean(&v[mid..])])
    };
    let mut d = Vec::new();
    for cy in 0..n {
        for cx in 0..n {
            let (Some(x), Some(y)) = (half_means(a, cy, cx), half_means(b, cy, cx)) else {
                continue;
            };
            let de = |p: [f32; 3], q: [f32; 3]| {
                let (dl, da, db) = (p[0] - q[0], p[1] - q[1], p[2] - q[2]);
                match part {
                    CoarsePart::Color => (da * da + db * db).sqrt(),
                    CoarsePart::Lightness => dl.abs(),
                }
            };
            d.push(de(x[0], y[0]).max(de(x[1], y[1])));
        }
    }
    if d.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    d.sort_by(f32::total_cmp);
    let q = |f: f32| d[((d.len() - 1) as f32 * f).round() as usize];
    (q(0.5), q(0.9), d[d.len() - 1])
}

/// Per matching file under `src` and `out` (same relative paths): the OKLab ΔE between their
/// mean colors, as text lines, plus the largest value.
pub fn identity(src: &Path, out: &Path) -> Result<(String, f32)> {
    let opts = WalkOptions {
        recursive: true,
        follow_links: false,
        exclude: None,
    };
    let mut text = String::new();
    let mut worst = 0.0f32;
    for e in walk::walk(out, &opts)?
        .entries
        .into_iter()
        .filter(|e| e.is_png)
    {
        let a = src.join(&e.rel);
        if !a.exists() {
            continue;
        }
        let (ia, ib) = (png_io::read(&a)?, png_io::read(&out.join(&e.rel))?);
        let (ma, mb) = (mean_oklab(&ia), mean_oklab(&ib));
        let de = ((0..3).map(|k| (ma[k] - mb[k]).powi(2)).sum::<f32>()).sqrt();
        worst = worst.max(de);
        // Per-texel ΔE (alpha-weighted) when the images line up.
        let px = if (ia.width, ia.height) == (ib.width, ib.height) {
            let (mut s, mut w) = (0.0f64, 0.0f64);
            for (p, q) in ia.pixels.iter().zip(&ib.pixels) {
                let (x, y) = (
                    color::srgb_to_oklab([p[0], p[1], p[2]]),
                    color::srgb_to_oklab([q[0], q[1], q[2]]),
                );
                let d = (0..3).map(|k| (x[k] - y[k]).powi(2)).sum::<f32>().sqrt();
                s += (d * p[3]) as f64;
                w += p[3] as f64;
            }
            format!("{:.3}", s / w.max(1e-9))
        } else {
            "-".into()
        };
        let _ = writeln!(
            text,
            "  mean-color ΔE {de:.3}, per-texel ΔE {px} (L {:.3} -> {:.3})  {}",
            ma[0],
            mb[0],
            e.rel.display()
        );
    }
    Ok((text, worst))
}

/// Per-texture metrics for matching PNGs under `src` and `out`: local L std (7×7, 25×25)
/// before → after; the mean absolute hue shift of dark colored source texels (L < 0.35,
/// C ≥ 0.03, output C ≥ 0.02) and their share that ended up in the blue–violet range; and, if
/// `baseline` is given, the per-texel ΔE of `out` against the same file there.
pub fn metrics(src: &Path, out: &Path, baseline: Option<&Path>) -> Result<String> {
    let opts = WalkOptions {
        recursive: true,
        follow_links: false,
        exclude: None,
    };
    let mut text = String::new();
    for e in walk::walk(out, &opts)?
        .entries
        .into_iter()
        .filter(|e| e.is_png)
    {
        let a = src.join(&e.rel);
        if !a.exists() {
            continue;
        }
        let one =
            |dir: &Path| -> Result<Vec<f32>> { local_contrast_file(&dir.join(&e.rel), &[3, 12]) };
        let (lb, la) = (one(src)?, one(out)?);
        let (ia, ib) = (png_io::read(&a)?, png_io::read(&out.join(&e.rel))?);
        let (mut dh, mut n, mut blue, mut n_dark) = (0.0f32, 0usize, 0usize, 0usize);
        for (p, q) in ia.pixels.iter().zip(&ib.pixels) {
            if p[3] < 0.5 {
                continue;
            }
            let s = color::oklab_to_oklch(color::srgb_to_oklab([p[0], p[1], p[2]]));
            let t = color::oklab_to_oklch(color::srgb_to_oklab([q[0], q[1], q[2]]));
            if s[0] < 0.35 && s[1] >= 0.03 && t[1] >= 0.02 {
                dh += color::hue_diff(s[2], t[2]).abs();
                n += 1;
            }
            // Any dark source texel (neutral or not) that came out visibly blue-violet.
            if s[0] < 0.35
                && t[1] >= 0.02
                && (225.0..300.0).contains(&t[2])
                && !(s[1] >= 0.02 && (225.0..300.0).contains(&s[2]))
            {
                blue += 1;
            }
            if s[0] < 0.35 {
                n_dark += 1;
            }
        }
        let dark = format!(
            "dark colored Δh {} | darks turned blue {:.1}% of {n_dark}",
            if n > 0 {
                format!("{:.1}°", dh / n as f32)
            } else {
                "-".into()
            },
            100.0 * blue as f32 / n_dark.max(1) as f32
        );
        let base = match baseline {
            Some(b) if b.join(&e.rel).exists() => {
                let ic = png_io::read(&b.join(&e.rel))?;
                let (mut s, mut w) = (0.0f64, 0.0f64);
                for (p, q) in ib.pixels.iter().zip(&ic.pixels) {
                    let (x, y) = (
                        color::srgb_to_oklab([p[0], p[1], p[2]]),
                        color::srgb_to_oklab([q[0], q[1], q[2]]),
                    );
                    s += ((0..3).map(|k| (x[k] - y[k]).powi(2)).sum::<f32>().sqrt() * q[3]) as f64;
                    w += q[3] as f64;
                }
                format!(" | ΔE vs baseline {:.4}", s / w.max(1e-9))
            }
            _ => String::new(),
        };
        let same = (ia.width, ia.height) == (ib.width, ib.height);
        let coarse = |part| {
            if same {
                coarse_delta_e(&ia, &ib, 16, part)
            } else {
                (f32::NAN, f32::NAN, f32::NAN)
            }
        };
        let ((_, c90, cmax), (_, l90, _)) =
            (coarse(CoarsePart::Color), coarse(CoarsePart::Lightness));
        let _ = writeln!(
            text,
            "{:<44} L std 7x7 {:.4}->{:.4}, 25x25 {:.4}->{:.4} | coarse Δab p90 {c90:.3} max \
             {cmax:.3}, ΔL p90 {l90:.3} | {dark}{base}",
            e.rel.file_stem().unwrap_or_default().to_string_lossy(),
            lb[0],
            la[0],
            lb[1],
            la[1]
        );
    }
    Ok(text)
}

fn local_contrast_file(path: &Path, radii: &[usize]) -> Result<Vec<f32>> {
    let dir = path.parent().context("no parent")?;
    let name = path.file_name().context("no file name")?;
    let tmp = tempdir_with(dir, name)?;
    let r = local_contrast(tmp.path(), radii);
    drop(tmp);
    r
}

/// A temporary folder containing only a link/copy of one file (local_contrast works on
/// folders).
fn tempdir_with(dir: &Path, name: &std::ffi::OsStr) -> Result<TempFolder> {
    let t = std::env::temp_dir().join(format!(
        "pastelplash-metrics-{}-{}",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&t)?;
    std::fs::copy(dir.join(name), t.join(name))?;
    Ok(TempFolder(t))
}

static TEMP_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

struct TempFolder(PathBuf);

impl TempFolder {
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Median of the lightness standard deviation in `(2r+1)²` windows centred on a grid of opaque
/// texels, for each radius, over every PNG under `dir`: how much light/dark variation a surface
/// has at that scale.
pub fn local_contrast(dir: &Path, radii: &[usize]) -> Result<Vec<f32>> {
    let opts = WalkOptions {
        recursive: true,
        follow_links: false,
        exclude: None,
    };
    let files: Vec<PathBuf> = walk::walk(dir, &opts)?
        .entries
        .into_iter()
        .filter(|e| e.is_png)
        .map(|e| dir.join(e.rel))
        .collect();
    let per_file: Vec<Vec<Vec<f32>>> = files
        .par_iter()
        .map(|f| -> Result<Vec<Vec<f32>>> {
            let img = png_io::read(f)?;
            let (w, h) = (img.width as usize, img.height as usize);
            let l: Vec<f32> = img
                .pixels
                .iter()
                .map(|p| color::srgb_to_oklab([p[0], p[1], p[2]])[0])
                .collect();
            let opaque = |x: usize, y: usize| img.pixels[y * w + x][3] >= 0.5;
            let step = ((w * h / 1500) as f32).sqrt().max(4.0) as usize;
            Ok(radii
                .iter()
                .map(|&r| {
                    let mut out = Vec::new();
                    for y in (r..h.saturating_sub(r)).step_by(step) {
                        for x in (r..w.saturating_sub(r)).step_by(step) {
                            if !opaque(x, y) {
                                continue;
                            }
                            let (mut s, mut s2, mut n) = (0.0f64, 0.0f64, 0.0f64);
                            for yy in y - r..=y + r {
                                for xx in x - r..=x + r {
                                    if opaque(xx, yy) {
                                        let v = l[yy * w + xx] as f64;
                                        s += v;
                                        s2 += v * v;
                                        n += 1.0;
                                    }
                                }
                            }
                            let m = s / n;
                            out.push(((s2 / n - m * m).max(0.0)).sqrt() as f32);
                        }
                    }
                    out
                })
                .collect())
        })
        .collect::<Result<_>>()?;
    Ok((0..radii.len())
        .map(|i| {
            let mut v: Vec<f32> = per_file.iter().flat_map(|f| f[i].iter().copied()).collect();
            median(&mut v)
        })
        .collect())
}

/// The report as text: one row per reference group, plus neutrals and local value contrast.
pub fn report(dir: &Path, reference: &Reference) -> Result<String> {
    let all = samples(dir, 40_000)?;
    anyhow::ensure!(!all.is_empty(), "no opaque texels under {}", dir.display());
    let n = all.len() as f32;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{:<20} {:>6} | {:>6} {:>6} {:>6} {:>6} | vs {} L {:>6} C {:>6} C90 {:>6} h",
        "group", "share", "L", "C", "C90", "h", reference.name, "", "", ""
    );
    let mut groups: Vec<Vec<[f32; 3]>> = vec![Vec::new(); reference.groups.len()];
    let mut neutral = Vec::new();
    for s in &all {
        match (s[1] >= NEUTRAL_C)
            .then(|| reference.group_of(s[2]))
            .flatten()
        {
            Some(i) => groups[i].push(*s),
            None => neutral.push(*s),
        }
    }
    for (g, v) in reference.groups.iter().zip(&groups) {
        let mut l: Vec<f32> = v.iter().map(|s| s[0]).collect();
        let mut c: Vec<f32> = v.iter().map(|s| s[1]).collect();
        // Hue median as an offset from the reference hue (robust to wrap-around).
        let mut dh: Vec<f32> = v.iter().map(|s| color::hue_diff(g.h, s[2])).collect();
        let h = (g.h + median(&mut dh)).rem_euclid(360.0);
        let _ = writeln!(
            out,
            "{:<20} {:>5.1}% | {:>6.3} {:>6.3} {:>6.3} {:>6.1} |    {:>6.3} {:>6.3} {:>6.3} {:>6.1}",
            g.name,
            100.0 * v.len() as f32 / n,
            median(&mut l),
            median(&mut c.clone()),
            pct(&mut c, 0.9),
            h,
            g.l,
            g.c,
            g.c_p90,
            g.h
        );
    }
    let mut l: Vec<f32> = neutral.iter().map(|s| s[0]).collect();
    let mut c: Vec<f32> = neutral.iter().map(|s| s[1]).collect();
    let _ = writeln!(
        out,
        "{:<20} {:>5.1}% | {:>6.3} {:>6.3}",
        format!("neutral (C<{NEUTRAL_C})"),
        100.0 * neutral.len() as f32 / n,
        median(&mut l),
        median(&mut c)
    );
    let mut c_all: Vec<f32> = all.iter().map(|s| s[1]).collect();
    let mut l_all: Vec<f32> = all.iter().map(|s| s[0]).collect();
    let _ = writeln!(
        out,
        "all texels: median C {:.3}, 90th-percentile C {:.3}, L p5..p95 {:.3}..{:.3}",
        median(&mut c_all.clone()),
        pct(&mut c_all, 0.9),
        pct(&mut l_all.clone(), 0.05),
        pct(&mut l_all, 0.95)
    );
    let lc = local_contrast(dir, &[3, 12])?;
    let _ = writeln!(out, "local L std: 7x7 {:.4}, 25x25 {:.4}", lc[0], lc[1]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_file_loads_and_groups_cover_the_circle() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("reference/ss-lit.toml");
        let r = Reference::load(&path).unwrap();
        for h in 0..360 {
            assert!(r.group_of(h as f32).is_some(), "hue {h} not covered");
        }
        assert_eq!(r.groups[r.group_of(135.0).unwrap()].name, "green_foliage");
    }
}
