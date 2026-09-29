//! Developer helpers for the fluid detection audit (`dev-fluid-scan`, `dev-fluid-sheet`): score
//! every texture of a pack (read-only) and lay out labeled contact sheets to check the detector
//! by eye.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use rayon::prelude::*;

use crate::config::Pack;
use crate::fluid;
use crate::image::{Image, SourceColor, SourceFormat};
use crate::png_io;

/// The TSV header of a scan.
pub const HEADER: &str = "path\tcategory\tw\th\tkind\tscore\twater\tlava\tliquid\tpattern\topaque\t\
     l_mean\tl_std\tc_mean\tc_p90\thue\thue_coh\tskew\tridge\tridge_amp\tridge_line\tridge_net\t\
     grit\tglow\twarm\tdark\tseam_u\tseam_v\ttranslucent\taspect\tridge_spread";

fn line(rel: &str, category: &str, w: u32, h: u32, d: &fluid::Detection) -> String {
    let f = &d.features;
    let mut s = format!(
        "{rel}\t{category}\t{w}\t{h}\t{}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}",
        d.kind.map_or("-", fluid::FluidKind::name),
        d.score(),
        d.water,
        d.lava,
        d.liquid,
        d.pattern
    );
    for v in [
        f.opaque,
        f.l_mean,
        f.l_std,
        f.c_mean,
        f.c_p90,
        f.hue,
        f.hue_coherence,
        f.skew,
        f.ridge,
        f.ridge_amp,
        f.ridge_line,
        f.ridge_net,
        f.grit,
        f.glow,
        f.warm,
        f.dark,
        f.seam[0].min(99.0),
        f.seam[1].min(99.0),
        f.translucent,
        f.aspect,
        f.ridge_spread,
    ] {
        let _ = write!(s, "\t{v:.4}");
    }
    s
}

/// Scores every texture of a `.o2r` pack (or every PNG under a folder, e.g. a thumbnail cache)
/// and writes one TSV line per texture. With `thumbs`, the analysis thumbnails of a pack are
/// saved there (archive paths + `.png`) for fast re-scans and contact sheets.
pub fn scan(
    input: &Path,
    out: &Path,
    thumbs: Option<&Path>,
    include: &[String],
    pack: Option<&Pack>,
    jobs: usize,
) -> Result<usize> {
    let lines = Mutex::new(Vec::new());
    let category = |rel: &str| {
        pack.map_or_else(
            || "-".to_string(),
            |p| format!("{:?}", p.classify(Path::new(rel))).to_lowercase(),
        )
    };
    let handle = |rel: &str, image: &Image| -> Result<()> {
        let thumb = fluid::thumbnail(image, fluid::THUMB);
        if let Some(dir) = thumbs {
            let dst = dir.join(format!("{rel}.png"));
            std::fs::create_dir_all(dst.parent().unwrap())?;
            png_io::write(&thumb, &dst)?;
        }
        let d = fluid::score(&fluid::features(&thumb));
        lines
            .lock()
            .unwrap()
            .push(line(rel, &category(rel), image.width, image.height, &d));
        Ok(())
    };
    let pool = rayon::ThreadPoolBuilder::new().num_threads(jobs).build()?;
    pool.install(|| -> Result<()> {
        if input.is_dir() {
            let walk_opts = crate::walk::WalkOptions {
                recursive: true,
                follow_links: false,
                exclude: None,
            };
            let rels: Vec<PathBuf> = crate::walk::walk(input, &walk_opts)?
                .entries
                .into_iter()
                .filter(|e| e.is_png)
                .map(|e| e.rel)
                .collect();
            rels.par_iter().try_for_each(|rel| {
                let image = png_io::read(&input.join(rel))?;
                let p = rel.to_string_lossy().replace('\\', "/");
                let p = p.strip_suffix(".png").unwrap_or(&p).to_string();
                if include.is_empty() || include.iter().any(|g| crate::config::glob_match(g, &p)) {
                    handle(&p, &image)?;
                }
                Ok(())
            })
        } else {
            crate::adapters::o2r::for_each_texture(input, include, &|rel, image| {
                handle(rel, &image)
            })
        }
    })?;
    let mut lines = lines.into_inner().unwrap();
    lines.sort();
    let n = lines.len();
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(out, format!("{HEADER}\n{}\n", lines.join("\n")))
        .with_context(|| format!("writing {}", out.display()))?;
    Ok(n)
}

/// 3×5 digit glyphs, one row per `u8` (3 low bits, MSB left).
const DIGITS: [[u8; 5]; 10] = [
    [7, 5, 5, 5, 7],
    [2, 6, 2, 2, 7],
    [7, 1, 7, 4, 7],
    [7, 1, 7, 1, 7],
    [5, 5, 7, 1, 1],
    [7, 4, 7, 1, 7],
    [7, 4, 7, 5, 7],
    [7, 1, 1, 1, 1],
    [7, 5, 7, 5, 7],
    [7, 5, 7, 1, 7],
];

/// Draws `n` at (x, y) with `scale`-sized glyph texels, white on a black box.
fn draw_number(px: &mut [[f32; 4]], w: u32, h: u32, x: u32, y: u32, n: usize, scale: u32) {
    let text = n.to_string();
    let (gw, gh) = (4 * scale * text.len() as u32 + scale, 7 * scale);
    for yy in y..(y + gh).min(h) {
        for xx in x..(x + gw).min(w) {
            px[(yy * w + xx) as usize] = [0.0, 0.0, 0.0, 1.0];
        }
    }
    for (i, ch) in text.bytes().enumerate() {
        let g = DIGITS[(ch - b'0') as usize];
        for (row, bits) in g.iter().enumerate() {
            for col in 0..3u32 {
                if bits >> (2 - col) & 1 == 1 {
                    for sy in 0..scale {
                        for sx in 0..scale {
                            let (xx, yy) = (
                                x + scale + (i as u32 * 4 + col) * scale + sx,
                                y + scale + row as u32 * scale + sy,
                            );
                            if xx < w && yy < h {
                                px[(yy * w + xx) as usize] = [1.0, 1.0, 1.0, 1.0];
                            }
                        }
                    }
                }
            }
        }
    }
}

/// A labeled contact sheet: one tile per listed path (a TSV whose first column is a path under
/// `dir`, without `.png`), numbered from `first` in list order, framed in `frame` (RGB).
pub fn sheet(
    dir: &Path,
    rels: &[String],
    out: &Path,
    thumb: u32,
    cols: u32,
    first: usize,
    frame: [f32; 3],
) -> Result<()> {
    anyhow::ensure!(!rels.is_empty(), "nothing to show");
    let gap = 6;
    let cols = cols.max(1).min(rels.len() as u32);
    let rows = (rels.len() as u32).div_ceil(cols);
    let (w, h) = (cols * (thumb + gap) + gap, rows * (thumb + gap) + gap);
    let mut pixels = vec![[0.25, 0.25, 0.25, 1.0]; (w * h) as usize];
    let tiles: Vec<Option<Image>> = rels
        .par_iter()
        .map(|rel| {
            png_io::read(&dir.join(format!("{rel}.png")))
                .ok()
                .map(|img| crate::compare::fit(&img, thumb))
        })
        .collect();
    for (i, tile) in tiles.iter().enumerate() {
        let (ox, oy) = (
            gap + (i as u32 % cols) * (thumb + gap),
            gap + (i as u32 / cols) * (thumb + gap),
        );
        for y in oy - 2..(oy + thumb + 2).min(h) {
            for x in ox - 2..(ox + thumb + 2).min(w) {
                pixels[(y * w + x) as usize] = [frame[0], frame[1], frame[2], 1.0];
            }
        }
        if let Some(t) = tile {
            for y in 0..t.height.min(thumb) {
                for x in 0..t.width.min(thumb) {
                    let p = t.pixels[(y * t.width + x) as usize];
                    let dst = &mut pixels[((oy + y) * w + ox + x) as usize];
                    for k in 0..3 {
                        dst[k] = p[k] * p[3] + 0.5 * (1.0 - p[3]);
                    }
                }
            }
        }
        draw_number(&mut pixels, w, h, ox, oy, first + i, (thumb / 64).max(2));
    }
    let image = Image {
        width: w,
        height: h,
        pixels,
        source: SourceFormat {
            color: SourceColor::Rgb,
            bit_depth: 8,
            has_alpha: false,
        },
        source_scale: None,
        tint_safe: None,
    };
    if let Some(d) = out.parent() {
        std::fs::create_dir_all(d)?;
    }
    png_io::write(&image, out)
}
