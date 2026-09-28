//! 3D color lookup tables: baking from a function, tetrahedral sampling, and `.cube` I/O.
//!
//! Tables map gamma-encoded sRGB in `0..=1` to gamma-encoded sRGB. Each entry also carries an
//! optional fourth value, the **lightness floor** the palette promised for that input (OKLab L);
//! the GPU stage uses it to keep later watercolor darkening from undercutting the palette. `.cube`
//! files have no such column, so loaded tables get a floor of 0 (no guard).

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};

#[derive(Debug, Clone, PartialEq)]
pub struct Lut3d {
    /// Entries per axis (2..=256).
    pub size: usize,
    /// `size³` entries, red fastest, then green, then blue (the `.cube` order). `[r, g, b, floor]`.
    pub data: Vec<[f32; 4]>,
}

impl Lut3d {
    /// Bakes `f(rgb) -> [r, g, b, floor]` at every lattice point.
    pub fn bake(size: usize, f: impl Fn([f32; 3]) -> [f32; 4] + Sync) -> Self {
        use rayon::prelude::*;
        assert!((2..=256).contains(&size));
        let n = size as f32 - 1.0;
        let data = (0..size * size * size)
            .into_par_iter()
            .map(|i| {
                let (r, g, b) = (i % size, i / size % size, i / (size * size));
                f([r as f32 / n, g as f32 / n, b as f32 / n])
            })
            .collect();
        Self { size, data }
    }

    pub fn identity(size: usize) -> Self {
        Self::bake(size, |[r, g, b]| [r, g, b, 0.0])
    }

    fn at(&self, r: usize, g: usize, b: usize) -> [f32; 4] {
        self.data[r + self.size * (g + self.size * b)]
    }

    /// Tetrahedral lookup (same math as the shader). Unlike trilinear interpolation it keeps
    /// the gray axis exact: a neutral input only ever blends neutral lattice points.
    pub fn sample(&self, rgb: [f32; 3]) -> [f32; 4] {
        let n = self.size - 1;
        let p = rgb.map(|c| c.clamp(0.0, 1.0) * n as f32);
        let [r, g, b] = p.map(|c| (c.floor() as usize).min(n - 1));
        let [fr, fg, fb] = [p[0] - r as f32, p[1] - g as f32, p[2] - b as f32];
        let c = |dr: usize, dg: usize, db: usize| self.at(r + dr, g + dg, b + db);
        // Four corners and weights of the tetrahedron containing the point.
        let (w, k): ([f32; 4], [[f32; 4]; 4]) = if fr > fg {
            if fg > fb {
                (
                    [1.0 - fr, fr - fg, fg - fb, fb],
                    [c(0, 0, 0), c(1, 0, 0), c(1, 1, 0), c(1, 1, 1)],
                )
            } else if fr > fb {
                (
                    [1.0 - fr, fr - fb, fb - fg, fg],
                    [c(0, 0, 0), c(1, 0, 0), c(1, 0, 1), c(1, 1, 1)],
                )
            } else {
                (
                    [1.0 - fb, fb - fr, fr - fg, fg],
                    [c(0, 0, 0), c(0, 0, 1), c(1, 0, 1), c(1, 1, 1)],
                )
            }
        } else if fb > fg {
            (
                [1.0 - fb, fb - fg, fg - fr, fr],
                [c(0, 0, 0), c(0, 0, 1), c(0, 1, 1), c(1, 1, 1)],
            )
        } else if fb > fr {
            (
                [1.0 - fg, fg - fb, fb - fr, fr],
                [c(0, 0, 0), c(0, 1, 0), c(0, 1, 1), c(1, 1, 1)],
            )
        } else {
            (
                [1.0 - fg, fg - fr, fr - fb, fb],
                [c(0, 0, 0), c(0, 1, 0), c(1, 1, 0), c(1, 1, 1)],
            )
        };
        let mut out = [0.0; 4];
        for i in 0..4 {
            for ch in 0..4 {
                out[ch] += w[i] * k[i][ch];
            }
        }
        out
    }

    /// Serializes as an Adobe/Resolve `.cube` 3D LUT (the floor column is not stored).
    pub fn to_cube(&self, title: &str) -> String {
        let mut s = String::new();
        let _ = writeln!(s, "TITLE \"{}\"", title.replace('"', "'"));
        let _ = writeln!(s, "# Written by pastelplash");
        let _ = writeln!(s, "LUT_3D_SIZE {}", self.size);
        let _ = writeln!(s, "DOMAIN_MIN 0.0 0.0 0.0");
        let _ = writeln!(s, "DOMAIN_MAX 1.0 1.0 1.0");
        for [r, g, b, _] in &self.data {
            let _ = writeln!(s, "{r:.6} {g:.6} {b:.6}");
        }
        s
    }

    pub fn parse_cube(text: &str) -> Result<Self> {
        let mut size = None;
        let mut data = Vec::new();
        for (no, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut words = line.split_whitespace();
            let first = words.next().unwrap_or_default();
            if first
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic())
            {
                match first {
                    "LUT_3D_SIZE" => {
                        let n: usize = words
                            .next()
                            .context("LUT_3D_SIZE without a value")?
                            .parse()
                            .context("bad LUT_3D_SIZE")?;
                        ensure!((2..=256).contains(&n), "LUT_3D_SIZE {n} out of range");
                        size = Some(n);
                    }
                    "LUT_1D_SIZE" => bail!("1D LUTs are not supported"),
                    "DOMAIN_MIN" | "DOMAIN_MAX" => {
                        let v: Vec<f32> = words.map(str::parse).collect::<Result<_, _>>()?;
                        let want = if first == "DOMAIN_MIN" { 0.0 } else { 1.0 };
                        ensure!(
                            v.iter().all(|&x| x == want),
                            "only a 0..1 domain is supported"
                        );
                    }
                    _ => {} // TITLE, LUT_3D_INPUT_RANGE, vendor keywords
                }
                continue;
            }
            let v: Vec<f32> = line
                .split_whitespace()
                .map(str::parse)
                .collect::<Result<_, _>>()
                .with_context(|| format!("line {}: bad number", no + 1))?;
            ensure!(v.len() == 3, "line {}: expected 3 values", no + 1);
            data.push([v[0], v[1], v[2], 0.0]);
        }
        let size = size.context("missing LUT_3D_SIZE")?;
        ensure!(
            data.len() == size * size * size,
            "expected {} entries, found {}",
            size * size * size,
            data.len()
        );
        Ok(Self { size, data })
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text =
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Self::parse_cube(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn save(&self, path: &Path, title: &str) -> Result<()> {
        fs::write(path, self.to_cube(title)).with_context(|| format!("writing {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_samples_back_its_input() {
        let lut = Lut3d::identity(17);
        for rgb in [
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
            [0.3, 0.71, 0.05],
            [0.5, 0.5, 0.99],
        ] {
            let out = lut.sample(rgb);
            for k in 0..3 {
                assert!((out[k] - rgb[k]).abs() < 1e-6, "{rgb:?} -> {out:?}");
            }
        }
    }

    #[test]
    fn grays_only_blend_grays() {
        // Neutral lattice points map to 0; everything else to 1.
        let lut = Lut3d::bake(9, |[r, g, b]| {
            let v = if r == g && g == b { 0.0 } else { 1.0 };
            [v, v, v, 0.0]
        });
        for i in 0..=100 {
            let v = i as f32 / 100.0;
            assert_eq!(lut.sample([v, v, v])[0], 0.0, "{v}");
        }
    }

    #[test]
    fn lattice_order_is_red_fastest() {
        let lut = Lut3d::identity(3);
        assert_eq!(lut.data[1], [0.5, 0.0, 0.0, 0.0]);
        assert_eq!(lut.data[3], [0.0, 0.5, 0.0, 0.0]);
        assert_eq!(lut.data[9], [0.0, 0.0, 0.5, 0.0]);
    }

    #[test]
    fn cube_round_trips() {
        let lut = Lut3d::bake(5, |[r, g, b]| [g, b * 0.5, r * r, 0.0]);
        let back = Lut3d::parse_cube(&lut.to_cube("test")).unwrap();
        assert_eq!(back.size, 5);
        for (a, b) in lut.data.iter().zip(&back.data) {
            for k in 0..3 {
                assert!((a[k] - b[k]).abs() < 1e-5);
            }
        }
    }

    #[test]
    fn cube_errors_are_reported() {
        assert!(Lut3d::parse_cube("LUT_3D_SIZE 2\n0 0 0\n").is_err());
        assert!(Lut3d::parse_cube("0 0 0\n").is_err());
        assert!(Lut3d::parse_cube("LUT_1D_SIZE 4\n").is_err());
    }
}
