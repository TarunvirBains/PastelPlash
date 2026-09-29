//! Value and technique: softer internal contrast, value grouping, small objects and lettering,
//! no blur, seamless tiling, alpha.

use super::*;
use pastelplash::analysis;

#[test]
fn rule_small_objects_survive() {
    // Busy textures get a large-scale abstraction; it must never erase small salient objects
    // (hooks, tools, bowls painted into a wall). Thin dark sticks and small bright squares on a
    // gritty, high-contrast wall keep at least half their contrast against their surroundings,
    // in every style and mood.
    let wall = bark(256, 21);
    let is_stick =
        |x: u32, y: u32| (x % 64 == 20 || x % 64 == 21 || x % 64 == 22) && (40..216).contains(&y);
    let is_square = |x: u32, y: u32| (100..112).contains(&x) && (y % 80) < 12 && y >= 16;
    let img = image(256, 256, |x, y| {
        let p = wall.pixels[(y * 256 + x) as usize];
        if is_stick(x, y) {
            let [r, g, b] = from_oklch(0.12, 0.03, 60.0);
            [r, g, b, 1.0]
        } else if is_square(x, y) {
            let [r, g, b] = from_oklch(0.92, 0.02, 90.0);
            [r, g, b, 1.0]
        } else {
            p
        }
    });
    // Mean L of the object texels vs. of the wall texels within 6 px of them.
    let contrast = |out: &Image, is_obj: &dyn Fn(u32, u32) -> bool| {
        let (mut o, mut no, mut s, mut ns) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        for y in 6..250 {
            for x in 6..250 {
                let l = lch(out.pixels[(y * 256 + x) as usize])[0];
                if is_obj(x, y) {
                    o += l;
                    no += 1.0;
                } else if (x - 6..=x + 6).any(|xx| is_obj(xx, y))
                    || (y - 6..=y + 6).any(|yy| is_obj(x, yy))
                {
                    s += l;
                    ns += 1.0;
                }
            }
        }
        (o / no - s / ns).abs()
    };
    let keep = contract().technique.small_object_min_contrast;
    let mut report = Report::new("small objects survive");
    let rendered = Matrix::full(&[Category::World]).check(&mut report, &img, |case, out| {
        let src = dimmed(case, &img);
        let (c_stick, c_square) = (contrast(&src, &is_stick), contrast(&src, &is_square));
        let (s1, q1) = (contrast(out, &is_stick), contrast(out, &is_square));
        ensure(s1 >= keep * c_stick, || {
            format!("stick contrast {c_stick:.3} -> {s1:.3}")
        })?;
        ensure(q1 >= keep * c_square, || {
            format!("square contrast {c_square:.3} -> {q1:.3}")
        })
    });
    if rendered {
        report.finish();
    }
}

#[test]
fn rule_text_stays_legible() {
    // A busy, grained sign (value grouping and abstraction both active) with small blocky
    // lettering: the letters keep most of their contrast against the board around them, in
    // every style and mood.
    let img = sign(256, 41);
    let contrast = |out: &Image| {
        let (mut o, mut no, mut s, mut ns) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        for y in 4..252u32 {
            for x in 4..252u32 {
                let l = lch(out.pixels[(y * 256 + x) as usize])[0];
                if is_letter(x, y) {
                    o += l;
                    no += 1.0;
                } else if (x - 4..=x + 4).any(|xx| is_letter(xx, y))
                    || (y - 4..=y + 4).any(|yy| is_letter(x, yy))
                {
                    s += l;
                    ns += 1.0;
                }
            }
        }
        s / ns - o / no
    };
    let keep = contract().technique.text_min_contrast;
    let mut report = Report::new("text stays legible");
    let rendered = Matrix::full(&[Category::World]).check(&mut report, &img, |case, out| {
        let c0 = contrast(&dimmed(case, &img));
        let c1 = contrast(out);
        ensure(c1 >= keep * c0, || {
            format!("lettering contrast {c0:.3} -> {c1:.3} (keep {keep})")
        })
    });
    if rendered {
        report.finish();
    }
}

#[test]
fn rule_grouping_only_touches_busy_world_textures() {
    // Value grouping never touches actors (the cel shader bands them already), even if a target
    // asked for it, nor calm, shape-based textures (ground, foliage).
    let trunk = bark(256, 51);
    let calm = mid_foliage(256, 52);
    let mut report = Report::new("grouping only touches busy world textures");
    for (label, path, config, mood) in style_moods() {
        if config.style.grouping.strength <= 0.0 {
            continue;
        }
        let n = label;
        let off = tweaked(&config, "grouping", "strength", 0.0);
        let mut greedy = config.clone();
        greedy
            .target
            .categories
            .entry(Category::Actor)
            .or_default()
            .grouping = 1.0;
        let Some(a_on) = render_mood(&path, &greedy, Category::Actor, &mood, &trunk) else {
            return;
        };
        let a_off = render_mood(&path, &off, Category::Actor, &mood, &trunk).unwrap();
        report.check(
            &n,
            ensure(a_on.pixels == a_off.pixels, || {
                "grouping changed an actor".into()
            }),
        );
        let c_on = render_mood(&path, &config, Category::World, &mood, &calm).unwrap();
        let c_off = render_mood(&path, &off, Category::World, &mood, &calm).unwrap();
        report.check(
            &n,
            ensure(c_on.pixels == c_off.pixels, || {
                "grouping changed a calm texture".into()
            }),
        );
    }
    report.finish();
}

#[test]
fn rule_grouping_forms_value_masses() {
    // On a busy photographic texture, grouping simplifies the values within each mass (less
    // lightness variation inside the source's light and dark regions) while the masses keep
    // their separation and the coarse light/dark pattern stays.
    use pastelplash::report::coarse_l_pattern;
    let k = contract();
    let trunk = bark(256, 61);
    // Source regions: 5×5 box-smoothed source L above / below its median.
    let size = 256usize;
    let src_l: Vec<f32> = trunk.pixels.iter().map(|&p| lch(p)[0]).collect();
    let smooth: Vec<f32> = (0..size * size)
        .map(|i| {
            let (x, y) = ((i % size) as isize, (i / size) as isize);
            let mut s = 0.0;
            for dy in -2..=2isize {
                for dx in -2..=2isize {
                    let (xx, yy) = (
                        (x + dx).rem_euclid(size as isize),
                        (y + dy).rem_euclid(size as isize),
                    );
                    s += src_l[yy as usize * size + xx as usize];
                }
            }
            s / 25.0
        })
        .collect();
    // Two-means (isodata) threshold: the source's own dark and light masses.
    let mut split = median(smooth.clone());
    for _ in 0..20 {
        let (mut lo, mut nl, mut hi, mut nh) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        for &s in &smooth {
            if s > split {
                hi += s;
                nh += 1.0;
            } else {
                lo += s;
                nl += 1.0;
            }
        }
        split = 0.5 * (lo / nl.max(1.0) + hi / nh.max(1.0));
    }
    // (mean within-region L std, light-minus-dark region mean L).
    let masses = |out: &Image| {
        let mut acc = [[0.0f64; 3]; 2];
        for (p, &s) in out.pixels.iter().zip(&smooth) {
            let l = lch(*p)[0] as f64;
            let r = usize::from(s > split);
            acc[r][0] += l;
            acc[r][1] += l * l;
            acc[r][2] += 1.0;
        }
        let stat = |a: [f64; 3]| {
            let m = a[0] / a[2];
            (m, (a[1] / a[2] - m * m).max(0.0).sqrt())
        };
        let ((m0, s0), (m1, s1)) = (stat(acc[0]), stat(acc[1]));
        (((s0 + s1) / 2.0) as f32, (m1 - m0) as f32)
    };
    let mut report = Report::new("grouping forms value masses");
    for (label, path, config, mood) in style_moods() {
        if config.style.grouping.strength <= 0.0 {
            continue;
        }
        let n = label;
        let off = tweaked(&config, "grouping", "strength", 0.0);
        let Some(on) = render_mood(&path, &config, Category::World, &mood, &trunk) else {
            return;
        };
        let off = render_mood(&path, &off, Category::World, &mood, &trunk).unwrap();
        let ((w_on, sep_on), (w_off, sep_off)) = (masses(&on), masses(&off));
        report.check(
            &n,
            ensure(w_on < w_off, || {
                format!(
                    "grouping did not simplify values within masses: L std {w_on:.4} vs \
                     {w_off:.4} without"
                )
            }),
        );
        report.check(
            &n,
            ensure(sep_on >= sep_off - k.tolerance.lightness, || {
                format!(
                    "grouping pulled the masses together: separation {sep_on:.3} vs {sep_off:.3}"
                )
            }),
        );
        let (corr, _) = coarse_l_pattern(&trunk, &on, 16);
        report.check(
            &n,
            ensure(corr >= k.identity.coarse_min_pattern_corr, || {
                format!("grouped pattern correlation {corr:.2}")
            }),
        );
    }
    report.finish();
}

#[test]
fn rule_adaptive_contrast_targets_high_contrast_textures() {
    // Trunk-like textures (large mid-scale lightness spread) are compressed noticeably; textures
    // below the style's target spread are not touched by adaptivity at all.
    let k = contract();
    let trunk = bark(256, 3);
    let ground = mid_foliage(256, 4);
    let mut report = Report::new("adaptive contrast targets high-contrast textures");
    for (label, path, config, mood) in style_moods() {
        if config.style.contrast.strength <= 0.0 {
            continue;
        }
        let n = label;
        let off = tweaked(&config, "contrast", "strength", 0.0);
        let Some(on_t) = render_mood(&path, &config, Category::World, &mood, &trunk) else {
            return;
        };
        let off_t = render_mood(&path, &off, Category::World, &mood, &trunk).unwrap();
        let (s_src, s_on) = (mid_std(&trunk), mid_std(&on_t));
        report.check(
            &n,
            ensure(
                s_on <= s_src * (1.0 - k.technique.adaptive_min_effect),
                || format!("trunk mid-scale L std {s_src:.4} -> {s_on:.4}"),
            ),
        );
        report.check(
            &n,
            ensure(mid_std(&on_t) < mid_std(&off_t), || {
                "adaptivity did not compress the trunk".into()
            }),
        );
        let on_g = render_mood(&path, &config, Category::World, &mood, &ground).unwrap();
        let off_g = render_mood(&path, &off, Category::World, &mood, &ground).unwrap();
        report.check(
            &n,
            ensure(on_g.pixels == off_g.pixels, || {
                "adaptivity changed a low-contrast texture".into()
            }),
        );
    }
    report.finish();
}

#[test]
fn rule_value_contrast_is_compressed_color_is_kept() {
    // Fine light/dark detail is reduced by at least the configured amount, while the texture's
    // mean lightness and its color stay.
    let k = contract();
    let img = gritty_blocks(256, 8);
    let std0 = local_std(&img);
    let c0 = median(img.pixels.iter().map(|&p| lch(p)[1]).collect());
    let mut report = Report::new("value contrast is compressed, color is kept");
    let rendered = Matrix::full(&[Category::World]).check(&mut report, &img, |case, out| {
        let mean0 = pastelplash::report::mean_oklab(&dimmed(case, &img))[0];
        let style = case.config.style.for_mood(&case.mood).unwrap();
        let fine = style.value_contrast.fine;
        let std1 = local_std(out);
        let want = std0 * (1.0 - k.technique.value_min_effect * fine);
        ensure(std1 <= want, || {
            format!("local L std {std0:.4} -> {std1:.4} (want <= {want:.4})")
        })?;
        let mean1 = pastelplash::report::mean_oklab(out)[0];
        let bound = k
            .technique
            .value_mean_tolerance
            .max(k.identity.bound(&case.style));
        ensure((mean1 - mean0).abs() <= bound, || {
            format!("mean L {mean0:.3} -> {mean1:.3}")
        })?;
        let c1 = median(out.pixels.iter().map(|&p| lch(p)[1]).collect());
        ensure(c1 >= k.palette.retained(c0), || {
            format!("median chroma {c0:.3} -> {c1:.3}")
        })
    });
    if rendered {
        report.finish();
    }
}

#[test]
fn rule_vivid_colors_are_bounded() {
    let k = contract();
    let img = image(128, 128, |x, y| {
        let [r, g, b] = from_oklch(0.4 + 0.4 * noise(x, y, 5), 0.3, x as f32 * 2.8);
        [r, g, b, 1.0]
    });
    let mut report = Report::new("vivid colors are bounded");
    let rendered = Matrix::full(&[Category::World]).check(&mut report, &img, |case, out| {
        // Brushwork and pooling may add a little chroma on top of the palette's cap; the
        // contract bounds the result.
        let style = case.config.style.for_mood(&case.mood).unwrap();
        let cap = (style.palette.chroma_cap.max(style.palette.vivid_max_chroma) * 1.25)
            .min(k.vivid.max_chroma);
        few(out, 0.0, |p| lch(p)[1] > cap + k.tolerance.chroma)
            .map_err(|e| format!("chroma above cap: {e}"))
    });
    if rendered {
        report.finish();
    }
}

#[test]
fn rule_alpha_preserved_and_no_halos() {
    let k = contract();
    let img = cutout(192, 3);
    let mut report = Report::new("alpha preserved, no halos");
    let rendered = Matrix::full(&STYLIZED).check(&mut report, &img, |_, out| {
        for (a, b) in img.pixels.iter().zip(&out.pixels) {
            ensure(a[3].to_bits() == b[3].to_bits(), || "alpha changed".into())?;
        }
        // Opaque texels next to transparent (black) ones must not be darker than the interior.
        let w = img.width as usize;
        let (mut rim, mut inner) = (Vec::new(), Vec::new());
        for y in 1..img.height as usize - 1 {
            for x in 1..w - 1 {
                if img.pixels[y * w + x][3] < 1.0 {
                    continue;
                }
                let near = [
                    (1, 0),
                    (-1, 0),
                    (0, 1),
                    (0, -1),
                    (2, 0),
                    (-2, 0),
                    (0, 2),
                    (0, -2),
                ]
                .iter()
                .any(|&(dx, dy): &(isize, isize)| {
                    let (xx, yy) = ((x as isize + dx) as usize, (y as isize + dy) as usize);
                    xx < w && yy < img.height as usize && img.pixels[yy * w + xx][3] == 0.0
                });
                let l = lch(out.pixels[y * w + x])[0];
                if near { rim.push(l) } else { inner.push(l) }
            }
        }
        let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
        let (rim, inner) = (mean(&rim), mean(&inner));
        ensure(rim >= inner - 0.05 - k.tolerance.lightness, || {
            format!("dark halo: rim L {rim} vs interior {inner}")
        })
    });
    if rendered {
        report.finish();
    }
}

#[test]
fn rule_tiling_textures_stay_seamless() {
    let img = tiling(256, 9);
    let before = [
        analysis::seam_ratio(&img, false),
        analysis::seam_ratio(&img, true),
    ];
    let mut report = Report::new("tiling textures stay seamless");
    let rendered = Matrix::full(&[Category::World]).check(&mut report, &img, |case, out| {
        for (axis, &b) in before.iter().enumerate() {
            let after = analysis::seam_ratio(out, axis == 1);
            let limit = (b * 1.5).max(case.config.style.tiling.threshold);
            ensure(after <= limit, || {
                format!("seam ratio axis {axis}: {b} -> {after} (limit {limit})")
            })?;
        }
        Ok(())
    });
    if rendered {
        report.finish();
    }
}

/// 10%–90% transition width of the mean row profile across the step at the image center.
fn edge_width(img: &Image) -> f32 {
    let (w, h) = (img.width as usize, img.height as usize);
    let profile: Vec<f32> = (0..w)
        .map(|x| {
            (h / 4..3 * h / 4)
                .map(|y| lch(img.pixels[y * w + x])[0])
                .sum::<f32>()
                / (h / 2) as f32
        })
        .collect();
    let (lo, hi) = (
        profile[w / 2 - 24..w / 2 - 12].iter().sum::<f32>() / 12.0,
        profile[w / 2 + 12..w / 2 + 24].iter().sum::<f32>() / 12.0,
    );
    let at = |f: f32| {
        let t = lo + (hi - lo) * f;
        (w / 2 - 16..w / 2 + 16)
            .find(|&x| profile[x] >= t)
            .unwrap_or(w / 2) as f32
    };
    at(0.9) - at(0.1)
}

/// Texel-level (1 px high-pass) lightness noise inside the left flat region: grit that the
/// painterly filter must turn into flat patches. Coarser watercolor texture (granulation, paper,
/// strokes) lives at larger scales and barely registers here.
fn interior_std(img: &Image) -> f32 {
    let (w, h) = (img.width as usize, img.height as usize);
    let l = |x: usize, y: usize| lch(img.pixels[y * w + x])[0];
    let v: Vec<f32> = (h / 4..3 * h / 4)
        .flat_map(|y| (w / 8..3 * w / 8).map(move |x| (x, y)))
        .map(|(x, y)| l(x, y) - (l(x - 1, y) + l(x + 1, y) + l(x, y - 1) + l(x, y + 1)) / 4.0)
        .collect();
    (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt()
}

#[test]
fn rule_no_blur_edges_stay_crisp_noise_becomes_flat() {
    let k = contract();
    let img = step_edge(512, 11);
    let (w0, s0) = (edge_width(&img), interior_std(&img));
    let mut report = Report::new("no blur: edges stay crisp, noise becomes flat");
    let rendered = Matrix::full(&[Category::World]).check(&mut report, &img, |_, out| {
        let w1 = edge_width(out);
        ensure(w1 <= k.technique.max_edge_width, || {
            format!("step edge widened {w0} -> {w1} texels")
        })?;
        let s1 = interior_std(out);
        ensure(s1 < s0, || {
            format!("flat-region noise did not decrease ({s0} -> {s1})")
        })
    });
    if rendered {
        report.finish();
    }
}

#[test]
fn rule_no_negative_dark_and_light_stay_one_family() {
    // Within one material, dark and light masses stay in the same color family (no warm-light /
    // cool-dark temperature split beyond the source's) and value stays the main carrier: the
    // darks are not lifted up toward the lights. (Darks may gain chroma along their own hue — a
    // dull warm dark must, or it is mud — but not turn cooler or warmer than their lights.)
    use pastelplash::report::{dark_light_split, split_hue};
    let k = contract();
    let mut report = Report::new("no negative: dark and light stay one family");
    let matrix = Matrix::full(&STYLIZED);
    // Halves grayer than this have no hue to split.
    let min_c = k.tolerance.chroma;
    for (label, img) in [
        ("olive bark", bark(256, 111)),
        ("warm wood", grooved_wood(256, 112)),
    ] {
        let rendered = matrix.check(&mut report, &img, |case, out| {
            // The reference: the source as the mood dims it, with darks at least at the contract's
            // min_l (lifting crushed blacks that far is required, not a "negative").
            let mut src = dimmed(case, &img);
            for p in &mut src.pixels {
                let [l, a, b] = pastelplash::color::srgb_to_oklab([p[0], p[1], p[2]]);
                if l < k.palette.min_l {
                    let rgb = pastelplash::color::oklab_to_srgb([k.palette.min_l, a, b]);
                    for c in 0..3 {
                        p[c] = rgb[c].clamp(0.0, 1.0);
                    }
                }
            }
            let (l0, k0, d0) = dark_light_split(&img, &src);
            let (l1, k1, d1) = dark_light_split(&img, out);
            // A source half too gray to have a hue counts as the other half's family.
            let h0 = split_hue(l0, k0, min_c).unwrap_or(0.0);
            if let Some(h1) = split_hue(l1, k1, min_c) {
                let dh = pastelplash::color::hue_diff(h0, h1).abs();
                ensure(dh <= k.technique.split_max_hue_change, || {
                    format!("{label}: dark/light hue split {h0:.0} -> {h1:.0} degrees")
                })?;
            }
            ensure(d1 >= k.technique.split_min_separation * d0, || {
                format!("{label}: dark/light separation {d0:.3} -> {d1:.3}")
            })
        });
        if !rendered {
            return;
        }
    }
    report.finish();
}

#[test]
fn rule_specks_are_cleaned() {
    // Specks of a few texels on a busy surface (dirt in a cobweb, photographic grit, blown-white
    // glitter on cracked ground) are noise and are cleaned, dark or bright; real small objects
    // keep their contrast (rule_small_objects_survive, rule_text_stays_legible) and compact
    // glints larger than a speck keep their white (rule_no_new_clipping).
    let k = contract();
    let wall = bark(256, 141);
    let is_speck = |x: u32, y: u32| (x % 23 < 2) && (y % 29 < 2) && x > 8 && y > 8;
    // Dark specks on a pale wall (as in a cobweb), then blown-white specks on a darker one.
    let make = |speck: [f32; 3], base: f32| {
        image(256, 256, |x, y| {
            if is_speck(x, y) {
                [speck[0], speck[1], speck[2], 1.0]
            } else {
                let mut p = wall.pixels[(y * 256 + x) as usize];
                for c in &mut p[..3] {
                    *c = base + 0.4 * *c;
                }
                p
            }
        })
    };
    let dark = from_oklch(0.1, 0.02, 60.0);
    let cases = [
        ("dark", make(dark, 0.5)),
        ("bright", make([1.0, 1.0, 1.0], 0.3)),
    ];
    // Mean L of the speck texels vs. of the texels 3..5 away from them (absolute).
    let contrast = |im: &Image| {
        let (mut o, mut no, mut s, mut ns) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        for y in 10..246u32 {
            for x in 10..246u32 {
                let l = lch(im.pixels[(y * 256 + x) as usize])[0];
                if is_speck(x, y) {
                    o += l;
                    no += 1.0;
                } else if (x % 23) >= 5 && (x % 23) < 8 && (y % 29) < 2 {
                    s += l;
                    ns += 1.0;
                }
            }
        }
        (s / ns - o / no).abs()
    };
    let mut report = Report::new("specks are cleaned");
    let matrix = Matrix::full(&[Category::World, Category::Background]);
    for (label, img) in &cases {
        let c0 = contrast(img);
        let rendered = matrix.check(&mut report, img, |_, out| {
            let c1 = contrast(out);
            ensure(c1 <= k.technique.speck_max_contrast * c0, || {
                format!("{label} speck contrast {c0:.3} -> {c1:.3}")
            })
        });
        if !rendered {
            return;
        }
    }
    report.finish();
}

#[test]
fn rule_no_ink_lines_on_tinted_ground() {
    // Engine-tinted (vertex-colored) gray ground with thin cracks: the cracks stay soft painted
    // cracks. Their darkest texels (p2 of L) darken by at most technique.ink_max_darkening (as the mood dims the source; in
    // v5/v6a, accent darks turned them into near-black ink: a gray accent cannot take its cool
    // hue, and the engine's tint darkens it further).
    let k = contract();
    let (mut img, is_crack) = cracked_ground(256, 161);
    img.tint_safe = Some(true);
    let darkest = |im: &Image| {
        let mut l: Vec<f32> = (0..256 * 256u32)
            .filter(|i| is_crack(i % 256, i / 256))
            .map(|i| lch(im.pixels[i as usize])[0])
            .collect();
        l.sort_by(f32::total_cmp);
        l[l.len() / 50]
    };
    let mut report = Report::new("no ink lines on tinted ground");
    let rendered = Matrix::full(&[Category::World, Category::Background]).check(
        &mut report,
        &img,
        |case, out| {
            // Against the source as the mood's moonlight dims it.
            let l0 = darkest(&dimmed(case, &img));
            let l1 = darkest(out);
            ensure(l0 - l1 <= k.technique.ink_max_darkening, || {
                format!("darkest crack texels L {l0:.3} -> {l1:.3}")
            })
        },
    );
    if rendered {
        report.finish();
    }
}

#[test]
fn rule_gameplay_cues_stay_readable() {
    // The cracks of a bombable wall are a gameplay cue: on files the pack map lists as cues,
    // crack-vs-wall lightness contrast keeps at least technique.cue_min_contrast of the source's
    // (as the mood dims it). The survey found the Desert Colossus and Zora's Fountain cracks
    // abstracted away and the Death Mountain Crater crack decal lifted from black to gray.
    let k = contract();
    let size = 256u32;
    let (gray, thin) = cracked_ground(size, 162);
    let crack = |x: u32, y: u32| {
        (x.saturating_sub(1)..=(x + 1).min(size - 1))
            .any(|xx| (y.saturating_sub(1)..=(y + 1).min(size - 1)).any(|yy| thin(xx, yy)))
    };
    // A busy sandstone wall (grit and mottling: grouped and abstracted as a plain wall) with
    // dark cracks 3-4 texels wide.
    let sand = image(size, size, |x, y| {
        let (fx, fy) = (x as f32, y as f32);
        let [r, g, b] = if crack(x, y) {
            from_oklch(0.25 + 0.04 * noise(x, y, 164), 0.025, 60.0)
        } else {
            let t = smooth_noise(fx, fy, 9, size, 165);
            from_oklch(
                0.48 + 0.26 * t + 0.22 * (noise(x, y, 166) - 0.5),
                0.05 + 0.02 * t,
                72.0 + 8.0 * noise(x, y, 167),
            )
        };
        [r, g, b, 1.0]
    });
    let contrast = |im: &Image, is: &dyn Fn(u32, u32) -> bool| {
        let (mut c, mut nc, mut w, mut nw) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        for y in 0..size {
            for x in 0..size {
                let l = lch(im.pixels[(y * size + x) as usize])[0];
                if is(x, y) {
                    (c, nc) = (c + l, nc + 1.0);
                } else {
                    (w, nw) = (w + l, nw + 1.0);
                }
            }
        }
        w / nw - c / nc
    };
    let mut report = Report::new("gameplay cues stay readable");
    let mut matrix = Matrix::full(&[Category::World, Category::Actor]);
    for case in &mut matrix.cases {
        case.config.pack.cues = vec!["**".into()];
    }
    for (label, img, is) in [
        ("gray crack decal", gray, &thin as &dyn Fn(u32, u32) -> bool),
        (
            "cracked sandstone",
            sand,
            &crack as &dyn Fn(u32, u32) -> bool,
        ),
    ] {
        let rendered = matrix.check(&mut report, &img, |case, out| {
            let c0 = contrast(&dimmed(case, &img), is);
            let c1 = contrast(out, is);
            ensure(c1 >= k.technique.cue_min_contrast * c0, || {
                format!("{label}: crack contrast {c0:.3} -> {c1:.3}")
            })
        });
        if !rendered {
            return;
        }
    }
    report.finish();
}

#[test]
fn rule_thin_structures_survive() {
    // Thin, elongated objects (tool handles, poles, rails, ropes: 3 texels wide) keep their
    // contrast against a busy wall in every style and mood, backgrounds included: the pitchfork
    // handles in Link's house must not dissolve into the wall. Shaft-vs-wall contrast (texels
    // 3..6 away across the shaft) keeps at least technique.thin_min_contrast of the source's for a
    // shaft that differs by color, technique.thin_min_value_contrast for lighter or darker ones.
    let k = contract();
    let (img, shaft) = wall_with_shafts(256, 191);
    // Lightness contrast for the light and dark shafts, color (a/b) distance for the reddish one.
    let contrast = |im: &Image, sign: i32| {
        let (mut o, mut no, mut s, mut ns) = ([0.0f32; 3], 0.0f32, [0.0f32; 3], 0.0f32);
        for y in 30..226u32 {
            for x in 8..248u32 {
                let p = im.pixels[(y * 256 + x) as usize];
                let lab = pastelplash::color::srgb_to_oklab([p[0], p[1], p[2]]);
                if shaft(x, y) == sign {
                    for k in 0..3 {
                        o[k] += lab[k];
                    }
                    no += 1.0;
                } else if shaft(x, y) == 0
                    && ((x - 6)..=(x + 6)).any(|xx| shaft(xx, y) == sign)
                    && !((x - 2)..=(x + 2)).any(|xx| shaft(xx, y) == sign)
                {
                    for k in 0..3 {
                        s[k] += lab[k];
                    }
                    ns += 1.0;
                }
            }
        }
        let d = [0, 1, 2].map(|k| o[k] / no - s[k] / ns);
        if sign == 2 {
            d[1].hypot(d[2])
        } else {
            d[0] * sign as f32
        }
    };
    let mut report = Report::new("thin structures survive");
    let rendered = Matrix::full(&[Category::World, Category::Background]).check(
        &mut report,
        &img,
        |case, out| {
            // Against the source as the mood's moonlight dims it.
            let src = dimmed(case, &img);
            let mut errs = Vec::new();
            for (sign, what) in [(1, "light"), (-1, "dark"), (2, "reddish")] {
                let (c0, c1) = (contrast(&src, sign), contrast(out, sign));
                let keep = if sign == 2 {
                    k.technique.thin_min_contrast
                } else {
                    k.technique.thin_min_value_contrast
                };
                if c1 < keep * c0 {
                    errs.push(format!("{what} shafts: contrast {c0:.3} -> {c1:.3}"));
                }
            }
            ensure(errs.is_empty(), || errs.join("; "))
        },
    );
    if rendered {
        report.finish();
    }
}

/// Mean lightness step across source-texel borders over the mean step inside source texels, for
/// an image enlarged `k` times (both axes).
fn blockiness(img: &Image, k: usize) -> f32 {
    let (w, h) = (img.width as usize, img.height as usize);
    let l: Vec<f32> = img.pixels.iter().map(|&p| lch(p)[0]).collect();
    let (mut border, mut nb, mut inside, mut ni) = (0.0f64, 0usize, 0.0f64, 0usize);
    let mut add = |d: f32, at_border: bool| {
        if at_border {
            border += d as f64;
            nb += 1;
        } else {
            inside += d as f64;
            ni += 1;
        }
    };
    for y in 0..h {
        for x in 0..w - 1 {
            add((l[y * w + x + 1] - l[y * w + x]).abs(), (x + 1) % k == 0);
        }
    }
    for y in 0..h - 1 {
        for x in 0..w {
            add((l[(y + 1) * w + x] - l[y * w + x]).abs(), (y + 1) % k == 0);
        }
    }
    ((border / nb.max(1) as f64) / (inside / ni.max(1) as f64).max(1e-9)) as f32
}

/// p90 over a 16×16 grid of the OKLab distance between the cells' mean colors.
fn coarse_delta_e(a: &Image, b: &Image) -> f32 {
    let (w, h) = (a.width as usize, a.height as usize);
    let mut d = Vec::new();
    for cy in 0..16 {
        for cx in 0..16 {
            let mean = |img: &Image| {
                let mut m = [0.0f32; 3];
                let mut n = 0.0;
                for y in cy * h / 16..(cy + 1) * h / 16 {
                    for x in cx * w / 16..(cx + 1) * w / 16 {
                        let p = img.pixels[y * w + x];
                        let lab = pastelplash::color::srgb_to_oklab([p[0], p[1], p[2]]);
                        for c in 0..3 {
                            m[c] += lab[c];
                        }
                        n += 1.0;
                    }
                }
                m.map(|v| v / n)
            };
            let (p, q) = (mean(a), mean(b));
            d.push(((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt());
        }
    }
    d.sort_by(f32::total_cmp);
    d[d.len() * 9 / 10]
}

/// Low-resolution textures are painted at the resolution floor: enlarged by an integer factor to
/// the contract's floor (or its maximum factor), with no texel grid, the same painting as at the
/// source size, and a tiling texture still tiles. (The Deku Tree's 128×256 ring walls showed
/// blocky pixel steps in a 4K game.)
#[test]
fn rule_low_res_textures_are_painted_at_the_floor() {
    let rr = &contract().resolution;
    let mut report = Report::new("low-res textures are painted at the floor");
    let src = bark(128, 3);
    let tile = tiling(64, 9);
    let tile_before = [
        analysis::seam_ratio(&tile, false),
        analysis::seam_ratio(&tile, true),
    ];
    let long = src.width.max(src.height);
    let want = rr.min_floor.min(rr.max_factor * long);
    for case in Matrix::base(&STYLIZED).cases {
        let label = case.label();
        let Some(out) = case.render_driven(&src) else {
            return;
        };
        let native = case.render(&src).unwrap();
        report.check(
            &label,
            (|| {
                let k = out.width / src.width;
                ensure(
                    out.width.max(out.height) >= want
                        && (out.width, out.height) == (src.width * k, src.height * k),
                    || {
                        format!(
                            "{}x{} -> {}x{} (want an integer enlargement to a long side >= {want})",
                            src.width, src.height, out.width, out.height
                        )
                    },
                )?;
                let b = blockiness(&out, k as usize);
                ensure(b <= rr.max_blockiness, || {
                    format!(
                        "texel grid shows: blockiness {b:.2} (max {})",
                        rr.max_blockiness
                    )
                })?;
                let small = pastelplash::resample::downsample(&out, k);
                let d = coarse_delta_e(&small, &native);
                ensure(d <= rr.max_coarse_delta_e, || {
                    format!(
                        "not the same painting: coarse ΔE p90 {d:.4} (max {})",
                        rr.max_coarse_delta_e
                    )
                })
            })(),
        );
        if case.category == Category::World {
            let out = case.render_driven(&tile).unwrap();
            for (axis, &b) in tile_before.iter().enumerate() {
                let after = analysis::seam_ratio(&out, axis == 1);
                let limit = (b * 1.5).max(case.config.style.tiling.threshold);
                report.check(
                    &label,
                    ensure(after <= limit, || {
                        format!("enlarged tiling texture: seam ratio axis {axis}: {b} -> {after} (limit {limit})")
                    }),
                );
            }
        }
    }
    report.finish();
}
