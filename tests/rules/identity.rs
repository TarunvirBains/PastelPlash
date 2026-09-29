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

#[test]
fn rule_moss_is_not_warmed_into_brown() {
    // Earth warmth targets earth: olive moss (the Deku Tree's walls, Kokiri Forest's mossy
    // ground) keeps its green-olive hue; on average it moves toward the warm earth hues by at most
    // the contract's bound.
    let k = contract();
    let img = olive_moss(192, 101, k.identity.moss_hue);
    let mut report = Report::new("moss is not warmed into brown");
    let rendered = Matrix::full(&[Category::World]).check(&mut report, &img, |_, out| {
        let (mut sum, mut n) = (0.0f32, 0.0f32);
        for (p, q) in img.pixels.iter().zip(&out.pixels) {
            let ([_, _, h0], [_, c1, h1]) = (lch(*p), lch(*q));
            if c1 < 0.03 {
                continue;
            }
            sum += pastelplash::color::hue_diff(h0, h1);
            n += 1.0;
        }
        let shift = sum / n.max(1.0);
        ensure(shift >= -k.identity.moss_max_warm_shift, || {
            format!("moss hue moved {shift:.1} degrees toward the warm earth hues")
        })
    });
    if rendered {
        report.finish();
    }
}

#[test]
fn rule_world_colors_bring_no_new_hue() {
    // No output texel of a world texture takes a hue family its source neighborhood doesn't
    // have: no navy flecks on warm treehouse bark or olive Deku Tree moss (v6a2: cool accent darks
    // on the pits left alone once the grit around them was cleaned). A texel colored at least
    // identity.new_hue_min_chroma stays within new_hue_max_gap degrees of some texel of its 5x5
    // source neighborhood colored at least half that. The one exception is a mood's moonlight
    // cast: near its hue, up to moods.<name>.cast_max_chroma.
    let k = contract();
    let (min_c, gap) = (k.identity.new_hue_min_chroma, k.identity.new_hue_max_gap);
    let mut report = Report::new("world colors bring no new hue");
    let matrix = Matrix::full(&[Category::World]);
    for (label, img) in [
        ("pitted bark", pitted_bark(192, 171)),
        ("bark", bark(192, 172)),
        ("grooved wood", grooved_wood(192, 173)),
        ("moss on wood", moss_on_wood(192, 174)),
        ("olive moss", olive_moss(192, 175, k.identity.moss_hue)),
        ("dark brown bark", dark_brown_bark(192, 176)),
    ] {
        let rendered = matrix.check(&mut report, &img, |case, out| {
            brings_no_new_hue(label, &img, case, out, min_c, gap)
        });
        if !rendered {
            return;
        }
    }
    // Neutral and faintly cool darks keep their family in the world and in pre-rendered rooms:
    // the umber floor for neutral darks is for warm neighborhoods. Steel doors and iron grids
    // came out copper, the Forest Temple's navy-violet boss door ochre, the Water Temple's slate
    // carvings rust, a green-gray door ochre (the umber floor, and the LUT's interpolation
    // between umber and cool nodes).
    let matrix = Matrix::full(&[Category::World, Category::Background]);
    for (label, img) in [
        ("steel", faint_cast_darks(192, 177, 250.0, 0.006)),
        ("violet iron", faint_cast_darks(192, 178, 295.0, 0.006)),
        ("green-gray door", faint_cast_darks(192, 179, 145.0, 0.006)),
        ("black wall", black_wall(192, 180)),
    ] {
        let rendered = matrix.check(&mut report, &img, |case, out| {
            brings_no_new_hue(label, &img, case, out, min_c, gap)?;
            neutral_darks_gain_no_warmth(label, &img, out)
        });
        if !rendered {
            return;
        }
    }
    report.finish();
}

/// No near-neutral source texel that comes out dark (below `palette.mud_l`) and whose 9x9 source neighborhood does not lean warm
/// (`palette.warm_context_*`) comes out warm (`palette.neutral_dark_max_warm_chroma`), beyond
/// the outlier budget.
fn neutral_darks_gain_no_warmth(label: &str, img: &Image, out: &Image) -> Result<(), String> {
    let k = contract();
    let (w, h) = (img.width as i32, img.height as i32);
    let src: Vec<[f32; 3]> = img
        .pixels
        .iter()
        .map(|&p| pastelplash::color::srgb_to_oklab([p[0], p[1], p[2]]))
        .collect();
    let (mut bad, mut n) = (0usize, 0usize);
    let mut example = None;
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            let s = src[i];
            if img.pixels[i][3] < 0.5
                || !k.palette.near_neutral(s)
                || lch(out.pixels[i])[0] >= k.palette.mud_l
            {
                continue;
            }
            let (mut a, mut b, mut m) = (0.0, 0.0, 0.0);
            for dy in -4..=4 {
                for dx in -4..=4 {
                    let j = ((y + dy).clamp(0, h - 1) * w + (x + dx).clamp(0, w - 1)) as usize;
                    let c = src[j][1].hypot(src[j][2]);
                    let f = if c > 0.03 { 0.03 / c } else { 1.0 };
                    a += src[j][1] * f;
                    b += src[j][2] * f;
                    m += 1.0;
                }
            }
            if k.palette.warm_leaning([a / m, b / m]) {
                continue;
            }
            n += 1;
            let o = lch(out.pixels[i]);
            if k.palette.turned_warm(o) {
                bad += 1;
                example.get_or_insert_with(|| {
                    let s = lch(img.pixels[i]);
                    format!(
                        "({x},{y}) LCh {:.2} {:.3} {:.0} -> {:.2} {:.3} {:.0}",
                        s[0], s[1], s[2], o[0], o[1], o[2]
                    )
                });
            }
        }
    }
    let budget = (n as f32 * k.tolerance.outliers).ceil() as usize;
    ensure(bad <= budget, || {
        format!(
            "{label}: {bad} of {n} neutral darks in a cool or neutral neighborhood turned warm \
             (budget {budget}); e.g. {}",
            example.unwrap_or_default()
        )
    })
}

#[test]
fn rule_sky_colors_bring_no_new_hue() {
    // The same for skies: a night sky of dark blue cloud with black gaps stays blue and neutral.
    // The palette's warm umber floor for neutral darks turned the black gaps of OoT Reloaded's
    // night and sunset skies (vr_fine3, vr_cloud2) into brown blotches, and its LUT
    // interpolation turned faint blue darks brown as well.
    let k = contract();
    let (min_c, gap) = (k.identity.new_hue_min_chroma, k.identity.new_hue_max_gap);
    let size = 192;
    let img = image(size, size, |x, y| {
        let t = smooth_noise(x as f32, y as f32, 5, size, 181);
        let gaps = smooth_noise(x as f32, y as f32, 3, size, 182);
        let [r, g, b] = if gaps > 0.62 {
            [0.0, 0.0, 0.0]
        } else {
            from_oklch(0.02 + 0.2 * t, 0.005 + 0.05 * t, 255.0)
        };
        [r, g, b, 1.0]
    });
    let mut report = Report::new("sky colors bring no new hue");
    let rendered = Matrix::full(&[Category::Skybox]).check(&mut report, &img, |case, out| {
        brings_no_new_hue("night sky", &img, case, out, min_c, gap)
    });
    if rendered {
        report.finish();
    }
}

/// No output texel colored at least `min_c` takes a hue more than `gap` degrees from every texel
/// of its 5x5 source neighborhood colored at least half that (a mood's moonlight cast excepted:
/// near its hue, up to `moods.<name>.cast_max_chroma`), beyond the outlier budget.
fn brings_no_new_hue(
    label: &str,
    img: &Image,
    case: &Case,
    out: &Image,
    min_c: f32,
    gap: f32,
) -> Result<(), String> {
    let k = contract();
    let (w, h) = (img.width as i32, img.height as i32);
    let src: Vec<[f32; 3]> = img.pixels.iter().map(|&p| lch(p)).collect();
    let style = case.config.style.for_mood(&case.mood).unwrap();
    let scale = case.config.target.treatment(case.category).cast;
    let cast = (pastelplash::palette::cast_strength(&style.palette, scale) > 0.0)
        .then(|| k.mood(&case.mood.name).and_then(|r| r.cast_max_chroma))
        .flatten()
        .map(|max_c| (style.palette.cast.hue, max_c));
    let (mut bad, mut n) = (0usize, 0usize);
    let mut example = None;
    for y in 0..h {
        for x in 0..w {
            let q = out.pixels[(y * w + x) as usize];
            if q[3] < 0.5 {
                continue;
            }
            n += 1;
            let [l1, c1, h1] = lch(q);
            if c1 < min_c {
                continue;
            }
            let near = |hue: f32| pastelplash::color::hue_diff(hue, h1).abs() <= gap;
            let mut family = false;
            'nb: for dy in -2..=2 {
                for dx in -2..=2 {
                    let i = (y + dy).clamp(0, h - 1) * w + (x + dx).clamp(0, w - 1);
                    let [_, c0, h0] = src[i as usize];
                    if c0 >= 0.5 * min_c && near(h0) {
                        family = true;
                        break 'nb;
                    }
                }
            }
            if family || cast.is_some_and(|(ch, max_c)| near(ch) && c1 <= max_c) {
                continue;
            }
            bad += 1;
            example.get_or_insert_with(|| {
                let [l0, c0, h0] = src[(y * w + x) as usize];
                format!("({x},{y}) LCh {l0:.2} {c0:.3} {h0:.0} -> {l1:.2} {c1:.3} {h1:.0}")
            });
        }
    }
    let budget = (n as f32 * k.tolerance.outliers).ceil() as usize;
    ensure(bad <= budget, || {
        format!(
            "{label}: {bad} of {n} texels took a new hue (budget {budget}); e.g. {}",
            example.unwrap_or_default()
        )
    })
}

/// Mean hue (degrees) of the texels colored at least 0.03.
fn mean_hue(img: &Image) -> f32 {
    let (mut a, mut b) = (0.0f64, 0.0f64);
    for p in img.pixels.iter().filter(|p| p[3] > 0.5) {
        let lab = pastelplash::color::srgb_to_oklab([p[0], p[1], p[2]]);
        if lab[1].hypot(lab[2]) >= 0.03 {
            a += lab[1] as f64;
            b += lab[2] as f64;
        }
    }
    (b.atan2(a).to_degrees() as f32).rem_euclid(360.0)
}

#[test]
fn rule_terracotta_only_on_grain_free_earth() {
    // Terracotta (Skyward Sword's Faron Woods earth) turns mid-value, grain-free earth browns of
    // world textures toward a rose-sienna, and nothing else: bark and wood (grain), light sand,
    // olive moss and every other category (actors, pre-rendered backgrounds, fluids, skies)
    // render exactly as without it. Moods may turn it off (nocturne).
    let k = contract();
    let earth = image(128, 128, |x, y| {
        let t = smooth_noise(x as f32, y as f32, 6, 128, 201);
        let [r, g, b] = from_oklch(
            0.4 + 0.12 * t + 0.04 * (noise(x, y, 202) - 0.5),
            0.06 + 0.02 * t,
            68.0 + 10.0 * (noise(x, y, 203) - 0.5),
        );
        [r, g, b, 1.0]
    });
    let sand = image(128, 128, |x, y| {
        let t = smooth_noise(x as f32, y as f32, 6, 128, 204);
        let [r, g, b] = from_oklch(0.78 + 0.06 * t, 0.06, 78.0);
        [r, g, b, 1.0]
    });
    let others = [
        ("grooved wood", grooved_wood(128, 205)),
        ("light sand", sand),
        ("olive moss", olive_moss(128, 206, k.identity.moss_hue)),
    ];
    let mut report = Report::new("terracotta only on grain-free earth");
    for case in Matrix::full(&STYLIZED).cases {
        let style = case.config.style.for_mood(&case.mood).unwrap();
        let tc = &style.terracotta;
        if !(k.terracotta.hue[0]..=k.terracotta.hue[1]).contains(&tc.hue) {
            report.fail(
                &case.label(),
                format!("target hue {} outside {:?}", tc.hue, k.terracotta.hue),
            );
        }
        let plain = Case {
            config: tweaked(&case.config, "terracotta", "strength", 0.0),
            style: case.style.clone(),
            path: case.path.clone(),
            mood: case.mood.clone(),
            category: case.category,
        };
        let same = |label: &str, img: &Image| -> Result<(), String> {
            let (a, b) = (case.render(img).unwrap(), plain.render(img).unwrap());
            let d = a
                .pixels
                .iter()
                .zip(&b.pixels)
                .map(|(p, q)| (0..3).map(|c| (p[c] - q[c]).abs()).fold(0.0f32, f32::max))
                .fold(0.0f32, f32::max);
            ensure(d <= 1e-6, || {
                format!("{label} changed by terracotta (max {d:.4})")
            })
        };
        if case.render(&earth).is_none() {
            return;
        }
        for (label, img) in &others {
            report.check(&case.label(), same(label, img));
        }
        if case.category.may_turn_terracotta() && tc.strength > 0.0 {
            let (a, b) = (case.render(&earth).unwrap(), plain.render(&earth).unwrap());
            let (h1, h0) = (mean_hue(&a), mean_hue(&b));
            let toward = -pastelplash::color::hue_diff(tc.hue, h0).signum()
                * pastelplash::color::hue_diff(h0, h1);
            report.check(
                &case.label(),
                ensure(toward >= k.terracotta.min_shift, || {
                    format!(
                        "earth: mean hue {h0:.1} -> {h1:.1}, not {} degrees toward {}",
                        k.terracotta.min_shift, tc.hue
                    )
                }),
            );
        } else {
            report.check(&case.label(), same("earth", &earth));
        }
    }
    report.finish();
}
