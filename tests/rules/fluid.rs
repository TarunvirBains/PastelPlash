//! Fluids: caustic water stays soft light over smooth depth (no outlined cells), keeps its
//! lightness and leans toward the reference water tone; engine-tinted gray water stays gray;
//! lava keeps its glow and heat colors in every mood.

use super::*;
use pastelplash::color::hue_diff;

/// Every fluid category.
const FLUIDS: [Category; 3] = [Category::Water, Category::Lava, Category::Liquid];

#[test]
fn fluid_treatments_stay_within_the_contract() {
    let k = &contract().fluid;
    for path in targets() {
        let config = Config::load(None, Some(&path), None).unwrap();
        for c in FLUIDS {
            let t = config.target.treatment(c);
            let label = format!("{} {c:?}", name(&path));
            for (what, v, max) in [
                ("wet_edges", t.wet_edges, k.max_wet_edges),
                ("granulation", t.granulation, k.max_granulation),
                ("value_contrast", t.value_contrast, k.max_value_contrast),
                ("abstraction", t.abstraction, k.max_abstraction),
                ("accent", t.accent, k.max_accent),
                ("radius_scale", t.radius_scale, k.max_radius_scale),
            ] {
                assert!(v <= max, "{label}: {what} = {v} (max {max})");
            }
            assert!(!c.may_group(), "{label}: fluids are never value-grouped");
        }
        let lava = config.target.treatment(Category::Lava);
        assert!(
            !lava.palette && lava.cast == 0.0 && Category::Lava.is_emissive(),
            "{}: lava keeps its own colors and light",
            name(&path)
        );
    }
}

/// Mean OKLab lightness of the texels whose weight passes `keep`.
fn mean_l(img: &Image, w: &[f32], keep: impl Fn(f32) -> bool) -> f32 {
    let v: Vec<f32> = img
        .pixels
        .iter()
        .zip(w)
        .filter(|(_, t)| keep(**t))
        .map(|(p, _)| lch(*p)[0])
        .collect();
    v.iter().sum::<f32>() / v.len().max(1) as f32
}

/// Depth texels: far from every line (the texel and its 8 neighbors).
fn depth_mask(w: &[f32], size: usize) -> Vec<bool> {
    (0..size * size)
        .map(|i| {
            let (x, y) = (i % size, i / size);
            (0..9).all(|k| {
                let (xx, yy) = ((x + size + k % 3 - 1) % size, (y + size + k / 3 - 1) % size);
                w[yy * size + xx] < 0.02
            })
        })
        .collect()
}

#[test]
fn rule_caustic_water_stays_luminous_and_smooth() {
    let k = contract();
    let f = &k.fluid;
    // Large enough for world-sized paint marks: rendered as World (the v5 look), this texture
    // fails with outlined cells and dulled highlights.
    let size = 768usize;
    let (img, w) = caustic_water(size as u32, 21, 0.05);
    let depth = depth_mask(&w, size);
    let mut report = Report::new("caustic water stays luminous and smooth");
    let rendered =
        Matrix::full(&[Category::Water, Category::Liquid]).check(&mut report, &img, |case, out| {
            let src = dimmed(case, &img);
            let line = |i: &Image| mean_l(i, &w, |t| t > 0.6) - mean_l(i, &w, |t| t < 0.02);
            let (c0, c1) = (line(&src), line(out));
            ensure(c1 >= f.highlight_min_contrast * c0, || {
                format!("highlight contrast {c0:.3} -> {c1:.3}")
            })?;
            // Depth grit: 1-texel high-pass lightness on the depth.
            let l: Vec<f32> = out.pixels.iter().map(|p| lch(*p)[0]).collect();
            let at = |x: usize, y: usize| l[(y % size) * size + x % size];
            let hp: Vec<f32> = (0..size * size)
                .filter(|&i| depth[i])
                .map(|i| {
                    let (x, y) = (i % size + size, i / size + size);
                    at(x, y) - (at(x - 1, y) + at(x + 1, y) + at(x, y - 1) + at(x, y + 1)) / 4.0
                })
                .collect();
            let grit = (hp.iter().map(|v| v * v).sum::<f32>() / hp.len() as f32).sqrt();
            ensure(grit <= f.depth_max_grit, || {
                format!("depth grit {grit:.4} (max {})", f.depth_max_grit)
            })?;
            // No cell outlines: depth texels darker than the (dimmed) source.
            let n = depth.iter().filter(|&&d| d).count();
            let dark = (0..size * size)
                .filter(|&i| depth[i] && l[i] < lch(src.pixels[i])[0] - f.outline_drop)
                .count();
            ensure(dark as f32 <= f.max_outline_share * n as f32, || {
                format!(
                    "{dark} of {n} depth texels outlined (darker by > {})",
                    f.outline_drop
                )
            })?;
            let all = |i: &Image| mean_l(i, &w, |_| true);
            let (m0, m1) = (all(&src), all(out));
            ensure((m1 - m0).abs() <= f.max_mean_l, || {
                format!("mean L {m0:.3} -> {m1:.3}")
            })
        });
    if rendered {
        report.finish();
    }
}

#[test]
fn rule_engine_tinted_gray_water_stays_gray() {
    let k = contract();
    let (img, _) = caustic_water(192, 22, 0.0);
    let mut report = Report::new("engine-tinted gray water stays gray");
    let rendered = Matrix::full(&FLUIDS).check(&mut report, &img, |_, out| {
        few(out, 0.0, |p| lch(p)[1] > k.tolerance.chroma)
    });
    if rendered {
        report.finish();
    }
}

/// Chroma-weighted mean hue of the colored texels.
fn mean_hue(img: &Image) -> f32 {
    let (mut a, mut b) = (0.0f32, 0.0f32);
    for p in &img.pixels {
        let lab = pastelplash::color::srgb_to_oklab([p[0], p[1], p[2]]);
        a += lab[1];
        b += lab[2];
    }
    b.atan2(a).to_degrees().rem_euclid(360.0)
}

#[test]
fn rule_water_leans_toward_the_reference_tone() {
    let (img, _) = caustic_water(192, 23, 0.06);
    let h0 = mean_hue(&img);
    let mut report = Report::new("water leans toward the reference tone");
    let rendered = Matrix::full(&[Category::Water]).check(&mut report, &img, |case, out| {
        let style = case.config.style.for_mood(&case.mood).unwrap();
        let tr = case.config.target.treatment(case.category);
        let pull = style.palette.water.pull * tr.reference;
        let r = style.palette.water.hue;
        let h1 = mean_hue(out);
        let (d0, d1) = (hue_diff(h0, r).abs(), hue_diff(h1, r).abs());
        // (Without a pull, only the mood's moonlight moves the hue.)
        ensure(pull <= 0.0 || d1 < d0, || {
            format!("hue {h0:.0} -> {h1:.0}, reference {r:.0}")
        })
    });
    if rendered {
        report.finish();
    }
}

#[test]
fn rule_lava_keeps_its_glow_and_heat_colors() {
    let k = contract();
    let f = &k.fluid;
    let (img, veins) = lava(256, 24);
    let mut report = Report::new("lava keeps its glow and heat colors");
    let rendered = Matrix::full(&[Category::Lava]).check(&mut report, &img, |_, out| {
        let idx: Vec<usize> = (0..veins.len()).filter(|&i| veins[i] > 0.5).collect();
        let (mut darkened, mut grayed) = (0usize, 0usize);
        let (mut sl0, mut sl1) = (0.0f32, 0.0f32);
        let (mut a0, mut b0, mut a1, mut b1) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        for &i in &idx {
            let (p, q) = (img.pixels[i], out.pixels[i]);
            let ([l0, c0, _], [l1, c1, _]) = (lch(p), lch(q));
            (sl0, sl1) = (sl0 + l0, sl1 + l1);
            if l1 < l0 - 4.0 * f.lava_max_darkening {
                darkened += 1;
            }
            if c1 < f.lava_min_chroma_retention * c0 - k.tolerance.chroma {
                grayed += 1;
            }
            let (s, t) = (
                pastelplash::color::srgb_to_oklab([p[0], p[1], p[2]]),
                pastelplash::color::srgb_to_oklab([q[0], q[1], q[2]]),
            );
            (a0, b0, a1, b1) = (a0 + s[1], b0 + s[2], a1 + t[1], b1 + t[2]);
        }
        let budget = (idx.len() as f32 * (k.tolerance.outliers + 0.01)).ceil() as usize;
        let n = idx.len().max(1) as f32;
        let (m0, m1) = (sl0 / n, sl1 / n);
        ensure(m1 >= m0 - f.lava_max_darkening, || {
            format!("vein mean L {m0:.3} -> {m1:.3}")
        })?;
        ensure(darkened <= budget, || {
            format!(
                "{darkened} of {} vein texels darkened by more than {}",
                idx.len(),
                4.0 * f.lava_max_darkening
            )
        })?;
        ensure(grayed <= budget, || {
            format!("{grayed} of {} vein texels lost chroma", idx.len())
        })?;
        let dh = hue_diff(b0.atan2(a0).to_degrees(), b1.atan2(a1).to_degrees()).abs();
        ensure(dh <= f.lava_max_hue_shift, || {
            format!("vein hue moved {dh:.1} degrees")
        })
    });
    if rendered {
        report.finish();
    }
}
