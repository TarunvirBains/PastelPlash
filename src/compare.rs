//! Developer helper (`dev-compare`): downsized before/after views and matching 1:1 crops from the
//! most detailed region, for judging the filter by eye.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use rayon::prelude::*;

use crate::image::{Image, SourceColor, SourceFormat};
use crate::png_io;

const RGBA8: SourceFormat = SourceFormat {
    color: SourceColor::Rgba,
    bit_depth: 8,
    has_alpha: true,
};

/// Box-downsizes (premultiplied) by an integer factor so the long side is at most `max_side`.
pub fn downsize(image: &Image, max_side: u32) -> Image {
    let long = image.width.max(image.height);
    let f = long.div_ceil(max_side.max(1)).max(1) as usize;
    let (w, h) = (image.width as usize, image.height as usize);
    let (ow, oh) = (w.div_ceil(f), h.div_ceil(f));
    let pixels = (0..ow * oh)
        .into_par_iter()
        .map(|i| {
            let (ox, oy) = (i % ow, i / ow);
            let mut acc = [0.0f32; 4];
            let mut n = 0.0;
            for y in oy * f..((oy + 1) * f).min(h) {
                for x in ox * f..((ox + 1) * f).min(w) {
                    let p = image.pixels[y * w + x];
                    for c in 0..3 {
                        acc[c] += p[c] * p[3];
                    }
                    acc[3] += p[3];
                    n += 1.0;
                }
            }
            if acc[3] > 0.0 {
                [
                    acc[0] / acc[3],
                    acc[1] / acc[3],
                    acc[2] / acc[3],
                    acc[3] / n,
                ]
            } else {
                [0.0; 4]
            }
        })
        .collect();
    Image {
        width: ow as u32,
        height: oh as u32,
        pixels,
        source: RGBA8,
        source_scale: None,
        tint_safe: None,
    }
}

pub fn crop(image: &Image, x0: u32, y0: u32, size: u32) -> Image {
    let (cw, ch) = (size.min(image.width - x0), size.min(image.height - y0));
    let mut pixels = Vec::with_capacity((cw * ch) as usize);
    for y in y0..y0 + ch {
        let row = (y * image.width) as usize;
        pixels.extend_from_slice(&image.pixels[row + x0 as usize..row + (x0 + cw) as usize]);
    }
    Image {
        width: cw,
        height: ch,
        pixels,
        source: RGBA8,
        source_scale: None,
        tint_safe: None,
    }
}

/// Top-left corner of the `size` square with the most (alpha-weighted) gradient energy.
pub fn detailed_region(image: &Image, size: u32) -> (u32, u32) {
    let (w, h) = (image.width, image.height);
    if w <= size && h <= size {
        return (0, 0);
    }
    let luma = |x: u32, y: u32| {
        let p = image.pixels[(y * w + x) as usize];
        (0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]) * p[3]
    };
    let step = (size / 8).max(8);
    let xs: Vec<u32> = (0..=w.saturating_sub(size))
        .step_by(step as usize)
        .collect();
    let ys: Vec<u32> = (0..=h.saturating_sub(size))
        .step_by(step as usize)
        .collect();
    let cands: Vec<(u32, u32)> = ys
        .iter()
        .flat_map(|&y| xs.iter().map(move |&x| (x, y)))
        .collect();
    cands
        .par_iter()
        .map(|&(x0, y0)| {
            let mut e = 0.0f32;
            let (cw, ch) = (size.min(w - x0), size.min(h - y0));
            for y in (y0..y0 + ch - 1).step_by(4) {
                for x in (x0..x0 + cw - 1).step_by(4) {
                    let c = luma(x, y);
                    e += (luma(x + 1, y) - c).abs() + (luma(x, y + 1) - c).abs();
                }
            }
            (e, x0, y0)
        })
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map_or((0, 0), |(_, x, y)| (x, y))
}

/// Writes a contact sheet of every PNG under `dir` (recursively, in sorted order): `cols`
/// columns of `thumb`-sized tiles on gray, and prints the index of each tile.
pub fn sheet(dir: &Path, out: &Path, thumb: u32, cols: u32) -> Result<()> {
    let walk_opts = crate::walk::WalkOptions {
        recursive: true,
        follow_links: false,
        exclude: None,
    };
    let names: Vec<_> = crate::walk::walk(dir, &walk_opts)?
        .entries
        .into_iter()
        .filter(|e| e.is_png)
        .map(|e| e.rel)
        .collect();
    anyhow::ensure!(!names.is_empty(), "no PNGs under {}", dir.display());
    let rows = (names.len() as u32).div_ceil(cols);
    let (w, h) = (cols * thumb, rows * thumb);
    let mut pixels = vec![[0.5, 0.5, 0.5, 1.0]; (w * h) as usize];
    for (i, name) in names.iter().enumerate() {
        let img = downsize(&png_io::read(&dir.join(name))?, thumb);
        let (ox, oy) = ((i as u32 % cols) * thumb, (i as u32 / cols) * thumb);
        for y in 0..img.height.min(thumb) {
            for x in 0..img.width.min(thumb) {
                let p = img.pixels[(y * img.width + x) as usize];
                let dst = &mut pixels[((oy + y) * w + ox + x) as usize];
                for c in 0..3 {
                    dst[c] = p[c] * p[3] + dst[c] * (1.0 - p[3]);
                }
            }
        }
        println!("{i:3} {}", name.display());
    }
    let sheet = Image {
        width: w,
        height: h,
        pixels,
        source: RGBA8,
        source_scale: None,
        tint_safe: None,
    };
    png_io::write(&sheet, out)
}

/// Resamples `image` to fit a `size`×`size` box (aspect kept): area average when shrinking,
/// nearest texel when enlarging (so texels stay visible in crops).
pub fn fit(image: &Image, size: u32) -> Image {
    let (w, h) = (image.width as f32, image.height as f32);
    let s = size as f32 / w.max(h);
    let (ow, oh) = (
        ((w * s).round() as u32).max(1),
        ((h * s).round() as u32).max(1),
    );
    let pixels = (0..ow * oh)
        .into_par_iter()
        .map(|i| {
            let (ox, oy) = ((i % ow) as f32, (i / ow) as f32);
            let (x0, x1) = (ox / s, (ox + 1.0) / s);
            let (y0, y1) = (oy / s, (oy + 1.0) / s);
            let (xa, xb) = (x0.floor() as u32, (x1.ceil() as u32).max(x0 as u32 + 1));
            let (ya, yb) = (y0.floor() as u32, (y1.ceil() as u32).max(y0 as u32 + 1));
            let mut acc = [0.0f32; 4];
            let mut n = 0.0;
            for y in ya..yb.min(image.height) {
                for x in xa..xb.min(image.width) {
                    let p = image.pixels[(y * image.width + x) as usize];
                    for c in 0..3 {
                        acc[c] += p[c] * p[3];
                    }
                    acc[3] += p[3];
                    n += 1.0;
                }
            }
            if acc[3] > 0.0 {
                [
                    acc[0] / acc[3],
                    acc[1] / acc[3],
                    acc[2] / acc[3],
                    acc[3] / n,
                ]
            } else {
                [0.0; 4]
            }
        })
        .collect();
    Image {
        width: ow,
        height: oh,
        pixels,
        source: RGBA8,
        source_scale: None,
        tint_safe: None,
    }
}

/// Writes a comparison grid (`dev-grid`): one row per relative path in `rows`, one column per
/// folder in `cols` (the same file in each), every cell fitted into a `thumb` square on gray.
/// With `crop`, each cell is instead a `crop`-sized 1:1 region (the most detailed one of the first
/// column's image, at the same relative position in the others). Missing files stay gray.
/// Prints the row index of every path.
pub fn grid(
    cols: &[std::path::PathBuf],
    rows: &[String],
    out: &Path,
    thumb: u32,
    crop_size: Option<u32>,
    wrap: u32,
) -> Result<()> {
    anyhow::ensure!(!cols.is_empty() && !rows.is_empty(), "nothing to show");
    let gap = 4;
    // Each path is a block of `cols` cells; `wrap` blocks per sheet row (with a wider gap).
    let wrap = wrap.max(1).min(rows.len() as u32);
    let nc = cols.len() as u32;
    let block = nc * (thumb + gap) + 3 * gap;
    let nr = (rows.len() as u32).div_ceil(wrap);
    let (w, h) = (wrap * block + gap, nr * (thumb + gap) + gap);
    let mut pixels = vec![[0.3, 0.3, 0.3, 1.0]; (w * h) as usize];
    for (r, rel) in rows.iter().enumerate() {
        let rel = if rel.to_ascii_lowercase().ends_with(".png") {
            rel.clone()
        } else {
            format!("{rel}.png")
        };
        let mut region = None;
        for (c, dir) in cols.iter().enumerate() {
            let path = dir.join(&rel);
            let Ok(img) = png_io::read(&path) else {
                continue;
            };
            let cell = match crop_size {
                Some(size) => {
                    let (fx, fy) = *region.get_or_insert_with(|| {
                        let (x, y) = detailed_region(&img, size);
                        (
                            x as f32 / img.width.max(1) as f32,
                            y as f32 / img.height.max(1) as f32,
                        )
                    });
                    let (x, y) = (
                        ((fx * img.width as f32) as u32).min(img.width.saturating_sub(1)),
                        ((fy * img.height as f32) as u32).min(img.height.saturating_sub(1)),
                    );
                    fit(&crop(&img, x, y, size), thumb)
                }
                None => fit(&img, thumb),
            };
            let (bx, by) = (r as u32 % wrap, r as u32 / wrap);
            let (ox, oy) = (
                gap + bx * block + c as u32 * (thumb + gap),
                gap + by * (thumb + gap),
            );
            for y in 0..cell.height.min(thumb) {
                for x in 0..cell.width.min(thumb) {
                    let p = cell.pixels[(y * cell.width + x) as usize];
                    let dst = &mut pixels[((oy + y) * w + ox + x) as usize];
                    for k in 0..3 {
                        dst[k] = p[k] * p[3] + 0.5 * (1.0 - p[3]);
                    }
                }
            }
        }
        println!("{r:3} {rel}");
    }
    let sheet = Image {
        width: w,
        height: h,
        pixels,
        source: RGBA8,
        source_scale: None,
        tint_safe: None,
    };
    if let Some(dir) = out.parent() {
        fs::create_dir_all(dir)?;
    }
    png_io::write(&sheet, out)
}

pub fn run(before: &Path, after: &Path, out: &Path, max_side: u32, crop_size: u32) -> Result<()> {
    fs::create_dir_all(out).with_context(|| format!("creating {}", out.display()))?;
    // Recursive, so exported pack trees work; nested files are named `<folder>__<file>`.
    let walk_opts = crate::walk::WalkOptions {
        recursive: true,
        follow_links: false,
        exclude: Some(out.to_path_buf()),
    };
    let names: Vec<_> = crate::walk::walk(after, &walk_opts)?
        .entries
        .into_iter()
        .filter(|e| e.is_png)
        .map(|e| e.rel)
        .collect();
    for name in names {
        let src = before.join(&name);
        if !src.exists() {
            continue;
        }
        let file = name.file_stem().unwrap().to_string_lossy();
        let stem = match name.parent().and_then(Path::file_name) {
            Some(dir) => format!("{}__{file}", dir.to_string_lossy()),
            None => file.to_string(),
        };
        let a = png_io::read(&src)?;
        let b = png_io::read(&after.join(&name))?;
        png_io::write(
            &downsize(&a, max_side),
            &out.join(format!("{stem}_before.png")),
        )?;
        png_io::write(
            &downsize(&b, max_side),
            &out.join(format!("{stem}_after.png")),
        )?;
        let (x, y) = detailed_region(&a, crop_size);
        png_io::write(
            &crop(&a, x, y, crop_size),
            &out.join(format!("{stem}_crop_before.png")),
        )?;
        png_io::write(
            &crop(&b, x, y, crop_size),
            &out.join(format!("{stem}_crop_after.png")),
        )?;
        println!("{stem}: crop at ({x}, {y})");
    }
    Ok(())
}
