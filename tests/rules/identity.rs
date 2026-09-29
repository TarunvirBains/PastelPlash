//! Identity: it still looks like the source game (mean color, hue families, the coarse pattern,
//! a pre-rendered room's exposure).

use super::*;
use pastelplash::report::{CoarsePart, coarse_delta_e, coarse_l_pattern, mean_oklab};

#[test]
fn rule_coarse_identity_is_kept() {
    // "From across the room" each texture stays recognizably the same: per 16×16 cell, the
    // dark-half and light-half mean colors stay close to the source's (brushwork doesn't count).
    let k = contract();
    let mut report = Report::new("coarse identity is kept");
    let matrix = Matrix::full(&[Category::World]);
    for (label, img) in [
        ("bark", bark(256, 11)),
        ("dark brown bark", dark_brown_bark(256, 12)),
        ("foliage", mid_foliage(256, 13)),
        ("blocks", gritty_blocks(256, 14)),
    ] {
        let rendered = matrix.check(&mut report, &img, |case, out| {
            let (max_c, max_l) = k.identity.coarse_bounds(&case.style);
            let (_, c90, _) = coarse_delta_e(&img, out, 16, CoarsePart::Color);
            let (_, l90, _) = coarse_delta_e(&img, out, 16, CoarsePart::Lightness);
            ensure(c90 <= max_c, || {
                format!("{label}: coarse chroma change p90 {c90:.3} > {max_c}")
            })?;
            ensure(l90 <= max_l, || {
                format!("{label}: coarse lightness change p90 {l90:.3} > {max_l}")
            })
        });
        if !rendered {
            return;
        }
    }
    report.finish();
}

#[test]
fn rule_coarse_light_pattern_survives() {
    // Busy bark with large pale lichen patches: compression and glare calming must keep the
    // coarse light/dark pattern (the patches), not flatten it into a monotone surface.
    let k = contract();
    let wall = bark(256, 31);
    let img = image(256, 256, |x, y| {
        let p = wall.pixels[(y * 256 + x) as usize];
        let (fx, fy) = (x as f32, y as f32);
        let lichen = smooth_noise(fx, fy, 3, 256, 32) > 0.62;
        if lichen {
            let [r, g, b] = from_oklch(0.8 + 0.08 * (noise(x, y, 33) - 0.5), 0.012, 100.0);
            [r, g, b, 1.0]
        } else {
            p
        }
    });
    let mut report = Report::new("coarse light pattern survives");
    let rendered = Matrix::full(&[Category::World]).check(&mut report, &img, |_, out| {
        let (corr, range) = coarse_l_pattern(&img, out, 16);
        ensure(corr >= k.identity.coarse_min_pattern_corr, || {
            format!("lichen pattern correlation {corr:.2}")
        })?;
        ensure(range >= k.identity.coarse_min_pattern_range, || {
            format!("lichen pattern range kept {range:.2}")
        })
    });
    if rendered {
        report.finish();
    }
}

#[test]
fn rule_identity_is_kept() {
    // Each texture's mean color stays close to the source's, and hue families stay put.
    let k = contract();
    let reference =
        pastelplash::report::Reference::load(&repo().join("reference/ss-lit.toml")).unwrap();
    let mut report = Report::new("identity is kept");
    let matrix = Matrix::full(&[Category::World]);
    for (label, img) in [
        ("tiling", tiling(256, 5)),
        ("foliage", mid_foliage(256, 6)),
        ("dull browns", dull_browns(192, 7)),
    ] {
        let rendered = matrix.check(&mut report, &img, |case, out| {
            let src_mean = mean_oklab(&dimmed(case, &img));
            let m = mean_oklab(out);
            let de = (0..3)
                .map(|i| (m[i] - src_mean[i]).powi(2))
                .sum::<f32>()
                .sqrt();
            let bound = k.identity.bound(&case.style);
            ensure(de <= bound, || {
                format!("{label}: mean color moved ΔE {de:.3} (bound {bound})")
            })?;
            // Per hue group (by source hue): the mean hue shift of colored texels.
            let mut shifts = vec![(0.0f32, 0usize); reference.groups.len()];
            for (a, b) in img.pixels.iter().zip(&out.pixels) {
                let (sa, sb) = (lch(*a), lch(*b));
                if sa[1] < 0.05 || sb[1] < 0.04 {
                    continue;
                }
                if let Some(g) = reference.group_of(sa[2]) {
                    shifts[g].0 += pastelplash::color::hue_diff(sa[2], sb[2]);
                    shifts[g].1 += 1;
                }
            }
            let total: usize = shifts.iter().map(|s| s.1).sum();
            for (g, (sum, n)) in reference.groups.iter().zip(shifts) {
                if n * 20 < total {
                    continue; // groups with under 5% of the texels
                }
                let mean = sum / n as f32;
                ensure(mean.abs() <= k.identity.max_group_hue_shift, || {
                    format!("{label}: {} hue moved {mean:.1} degrees", g.name)
                })?;
            }
            Ok(())
        });
        if !rendered {
            return;
        }
    }
    report.finish();
}

#[test]
fn rule_backgrounds_keep_their_exposure() {
    // Pre-rendered rooms carry the designers' lighting: crushed darks are still lifted into
    // readable shadow, but the room's mean lightness and its coarse light/dark pattern stay.
    use pastelplash::exposure::mean_l;
    let k = contract();
    let img = room(256, 71);
    let dark = |im: &Image| {
        let (mut s, mut n) = (0.0f32, 0.0f32);
        for (p, q) in img.pixels.iter().zip(&im.pixels) {
            if lch(*p)[0] < 0.13 {
                s += lch(*q)[0];
                n += 1.0;
            }
        }
        s / n
    };
    let mut report = Report::new("backgrounds keep their exposure");
    let rendered = Matrix::full(&[Category::Background]).check(&mut report, &img, |case, out| {
        // A mood's moonlight dims the room on purpose: its exposure is the reference.
        let src = dimmed(case, &img);
        let (m0, m1) = (mean_l(&src.pixels), mean_l(&out.pixels));
        ensure((m1 - m0).abs() <= k.identity.background_max_mean_l, || {
            format!("room mean L {m0:.3} -> {m1:.3}")
        })?;
        let (_, range) = coarse_l_pattern(&src, out, 16);
        ensure(range >= k.identity.background_min_pattern_range, || {
            format!("room light/dark range kept {range:.2}")
        })?;
        ensure(
            dark(out) >= k.palette.min_l - k.tolerance.lightness && dark(out) > dark(&img),
            || {
                format!(
                    "crushed corners {:.3} -> {:.3} (not lifted)",
                    dark(&img),
                    dark(out)
                )
            },
        )
    });
    if rendered {
        report.finish();
    }
}

#[test]
fn rule_hue_families_survive() {
    // Moss stays greener than wood: on a two-material texture, each hue family keeps its share of
    // the texels and the two families stay apart in hue (in every mood: a moonlight cast shifts
    // every color the same way, so the differences survive).
    let k = contract();
    let img = moss_on_wood(256, 81);
    // (green share, brown share, circular mean hue of each) over texels with some chroma.
    let families = |im: &Image| {
        let (mut g, mut b, mut n) = (0.0f32, 0.0f32, 0.0f32);
        let (mut gv, mut bv) = ([0.0f32; 2], [0.0f32; 2]);
        for p in &im.pixels {
            let [_, c, h] = lch(*p);
            n += 1.0;
            if c < 0.02 {
                continue;
            }
            let v = [h.to_radians().cos(), h.to_radians().sin()];
            if (95.0..200.0).contains(&h) {
                g += 1.0;
                gv = [gv[0] + v[0], gv[1] + v[1]];
            } else if (15.0..95.0).contains(&h) {
                b += 1.0;
                bv = [bv[0] + v[0], bv[1] + v[1]];
            }
        }
        let deg = |v: [f32; 2]| v[1].atan2(v[0]).to_degrees().rem_euclid(360.0);
        (g / n, b / n, deg(gv), deg(bv))
    };
    let (g0, b0, hg0, hb0) = families(&img);
    let sep0 = pastelplash::color::hue_diff(hb0, hg0);
    let mut report = Report::new("hue families survive");
    let rendered = Matrix::full(&[Category::World]).check(&mut report, &img, |_, out| {
        let (g1, b1, hg1, hb1) = families(out);
        let bound = k.identity.family_share_max_change;
        ensure((g1 - g0).abs() <= bound && (b1 - b0).abs() <= bound, || {
            format!("family shares moved: green {g0:.2} -> {g1:.2}, brown {b0:.2} -> {b1:.2}")
        })?;
        let sep1 = pastelplash::color::hue_diff(hb1, hg1);
        ensure(sep1 >= k.identity.family_min_separation * sep0, || {
            format!("moss/wood hue separation {sep0:.0} -> {sep1:.0} degrees")
        })
    });
    if rendered {
        report.finish();
    }
}
