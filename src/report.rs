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

/// The report as text: one row per reference group, plus neutrals.
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
    let _ = writeln!(
        out,
        "all texels: median C {:.3}, 90th-percentile C {:.3}",
        median(&mut c_all.clone()),
        pct(&mut c_all, 0.9)
    );
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
