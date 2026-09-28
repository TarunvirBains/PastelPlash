//! Style rules checked on rendered output (GPU). Every style in `styles/` is rendered with the
//! default target on procedural, license-clean test images and checked against the parameters
//! the style sets, within the bounds and tolerances of `rules.toml`. Skips (passes with a
//! message) when no GPU adapter is available. See docs/RULES.md.

mod common;

use common::*;
use pastelplash::analysis;
use pastelplash::config::{Category, Config, Style};
use pastelplash::image::Image;

/// Renders `img` with every style; `f(style name, style, config, output)`.
fn each_render(category: Category, img: &Image, mut f: impl FnMut(&str, &Style, &Config, &Image)) {
    for path in styles() {
        let config = load(&path, &default_target());
        let Some(out) = render(&path, &config, category, img) else {
            return;
        };
        f(&name(&path), &config.style, &config, &out);
    }
}

/// Fails if more than the contract's outlier budget of opaque texels fail `bad`.
fn assert_few(name: &str, rule: &str, out: &Image, bad: impl Fn([f32; 4]) -> bool) {
    assert_few_plus(name, rule, out, 0.0, bad);
}

/// Like [`assert_few`], with `extra` more of the texels allowed to fail (e.g. a style's accent
/// darks, which have their own rule).
fn assert_few_plus(
    name: &str,
    rule: &str,
    out: &Image,
    extra: f32,
    bad: impl Fn([f32; 4]) -> bool,
) {
    let opaque: Vec<[f32; 4]> = out.pixels.iter().copied().filter(|p| p[3] > 0.5).collect();
    let failing: Vec<[f32; 4]> = opaque.iter().copied().filter(|&p| bad(p)).collect();
    let budget = (opaque.len() as f32 * (contract().tolerance.outliers + extra)).ceil() as usize;
    assert!(
        failing.len() <= budget,
        "{name}: {rule}: {} of {} texels fail (budget {budget}); e.g. {:?} = LCh {:?}",
        failing.len(),
        opaque.len(),
        failing[0],
        lch(failing[0])
    );
}

fn median(mut v: Vec<f32>) -> f32 {
    let i = v.len() / 2;
    *v.select_nth_unstable_by(i, f32::total_cmp).1
}

/// Median lightness standard deviation over 7×7 windows on a grid.
fn local_std(img: &Image) -> f32 {
    let (w, h) = (img.width as usize, img.height as usize);
    let l: Vec<f32> = img.pixels.iter().map(|&p| lch(p)[0]).collect();
    let mut v = Vec::new();
    for y in (3..h - 3).step_by(5) {
        for x in (3..w - 3).step_by(5) {
            let (mut s, mut s2) = (0.0, 0.0);
            for yy in y - 3..=y + 3 {
                for xx in x - 3..=x + 3 {
                    let t = l[yy * w + xx];
                    s += t;
                    s2 += t * t;
                }
            }
            let m = s / 49.0;
            v.push((s2 / 49.0 - m * m).max(0.0f32).sqrt());
        }
    }
    median(v)
}

#[test]
fn rule_darks_are_colored_never_black() {
    let k = contract();
    for img in [
        dark_hues(192, 1),
        dark_foliage(192, 2),
        grayscale_dark(192, 3),
    ] {
        each_render(Category::World, &img, |name, _, _, out| {
            assert_few(name, "crushed black", out, |p| {
                lch(p)[0] < k.palette.min_l - k.tolerance.lightness
            });
            assert_few(name, "neutral dark", out, |p| {
                let [l, c, _] = lch(p);
                l < k.palette.dark_l && c < k.palette.dark_min_chroma - 1e-3
            });
        });
    }
}

#[test]
fn rule_coarse_identity_is_kept() {
    // "From across the room" each texture stays recognizably the same: per 16×16 cell, the
    // dark-half and light-half mean colors stay close to the source's (brushwork doesn't count).
    use pastelplash::report::{CoarsePart, coarse_delta_e};
    let k = contract();
    let cases = [
        ("bark", bark(256, 11)),
        ("dark brown bark", dark_brown_bark(256, 12)),
        ("foliage", mid_foliage(256, 13)),
        ("blocks", gritty_blocks(256, 14)),
    ];
    for (label, img) in cases {
        each_render(Category::World, &img, |name, _, _, out| {
            let (max_c, max_l) = k.identity.coarse_bounds(name);
            let (_, c90, _) = coarse_delta_e(&img, out, 16, CoarsePart::Color);
            let (_, l90, _) = coarse_delta_e(&img, out, 16, CoarsePart::Lightness);
            assert!(
                c90 <= max_c,
                "{name}: {label}: coarse chroma change p90 {c90:.3} > {max_c}"
            );
            assert!(
                l90 <= max_l,
                "{name}: {label}: coarse lightness change p90 {l90:.3} > {max_l}"
            );
        });
    }
}

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
    let k = contract();
    let (c_stick, c_square) = (contrast(&img, &is_stick), contrast(&img, &is_square));
    for (label, path, config, mood) in style_moods() {
        let Some(out) = render_mood(&path, &config, Category::World, &mood, &img) else {
            return;
        };
        let (s1, q1) = (contrast(&out, &is_stick), contrast(&out, &is_square));
        let style_name = label.split(" [").next().unwrap_or_default();
        let keep = k
            .technique
            .small_object_styles
            .get(style_name)
            .copied()
            .unwrap_or(k.technique.small_object_min_contrast);
        assert!(
            s1 >= keep * c_stick,
            "{label}: stick contrast {c_stick:.3} -> {s1:.3}"
        );
        assert!(
            q1 >= keep * c_square,
            "{label}: square contrast {c_square:.3} -> {q1:.3}"
        );
    }
}

#[test]
fn rule_bark_does_not_turn_blue() {
    // Regression: v2 lifted bark and cliff darks toward navy. Dark bark (brown, and near-neutral
    // olive-gray with near-black grooves) must keep a warm hue in every style.
    for img in [dark_brown_bark(192, 1), bark(192, 2)] {
        each_render(Category::World, &img, |name, style, _, out| {
            let accents = style.palette.accent_fraction;
            assert_few_plus(name, "dark bark turned blue", out, accents, |p| {
                let [l, c, h] = lch(p);
                l < 0.45 && c >= 0.02 && (200.0..320.0).contains(&h)
            });
        });
    }
}

#[test]
fn rule_adaptive_contrast_targets_high_contrast_textures() {
    // Trunk-like textures (large mid-scale lightness spread) are compressed noticeably; textures
    // below the style's target spread are not touched by adaptivity at all.
    let k = contract();
    let trunk = bark(256, 3);
    let ground = mid_foliage(256, 4);
    for path in styles() {
        let config = load(&path, &default_target());
        if config.style.contrast.strength <= 0.0 {
            continue;
        }
        let n = name(&path);
        let mut off = config.clone();
        off.style.contrast.strength = 0.0;
        let Some(on_t) = render(&path, &config, Category::World, &trunk) else {
            return;
        };
        let off_t = render(&path, &off, Category::World, &trunk).unwrap();
        let (s_src, s_on) = (mid_std(&trunk), mid_std(&on_t));
        assert!(
            s_on <= s_src * (1.0 - k.technique.adaptive_min_effect),
            "{n}: trunk mid-scale L std {s_src:.4} -> {s_on:.4}"
        );
        assert!(
            mid_std(&on_t) < mid_std(&off_t),
            "{n}: adaptivity did not compress the trunk"
        );
        let on_g = render(&path, &config, Category::World, &ground).unwrap();
        let off_g = render(&path, &off, Category::World, &ground).unwrap();
        assert_eq!(
            on_g.pixels, off_g.pixels,
            "{n}: adaptivity changed a low-contrast texture"
        );
    }
}

/// Median lightness standard deviation over 25×25 windows on a grid.
fn mid_std(img: &Image) -> f32 {
    let (w, h) = (img.width as usize, img.height as usize);
    let l: Vec<f32> = img.pixels.iter().map(|&p| lch(p)[0]).collect();
    let mut v = Vec::new();
    for y in (12..h - 12).step_by(9) {
        for x in (12..w - 12).step_by(9) {
            let (mut s, mut s2, mut n) = (0.0f32, 0.0f32, 0.0f32);
            for yy in (y - 12..=y + 12).step_by(2) {
                for xx in (x - 12..=x + 12).step_by(2) {
                    let t = l[yy * w + xx];
                    s += t;
                    s2 += t * t;
                    n += 1.0;
                }
            }
            let m = s / n;
            v.push((s2 / n - m * m).max(0.0).sqrt());
        }
    }
    median(v)
}

#[test]
fn rule_no_brown_mud() {
    let k = contract();
    each_render(Category::World, &dull_browns(192, 4), |name, _, _, out| {
        assert_few(name, "brown mud", out, |p| k.palette.is_mud(lch(p)));
    });
}

#[test]
fn rule_identity_is_kept() {
    // Each texture's mean color stays close to the source's, and hue families stay put.
    let k = contract();
    let reference =
        pastelplash::report::Reference::load(&repo().join("reference/ss-lit.toml")).unwrap();
    for img in [tiling(256, 5), mid_foliage(256, 6), dull_browns(192, 7)] {
        let src_mean = pastelplash::report::mean_oklab(&img);
        each_render(Category::World, &img, |name, _, _, out| {
            let m = pastelplash::report::mean_oklab(out);
            let de = (0..3)
                .map(|i| (m[i] - src_mean[i]).powi(2))
                .sum::<f32>()
                .sqrt();
            let bound = k.identity.bound(name);
            assert!(
                de <= bound,
                "{name}: mean color moved ΔE {de:.3} (bound {bound})"
            );
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
                assert!(
                    mean.abs() <= k.identity.max_group_hue_shift,
                    "{name}: {} hue moved {mean:.1} degrees",
                    g.name
                );
            }
        });
    }
}

#[test]
fn rule_value_contrast_is_compressed_color_is_kept() {
    // Fine light/dark detail is reduced by at least the configured amount, while the texture's
    // mean lightness and its color stay.
    let k = contract();
    let img = gritty_blocks(256, 8);
    let (std0, mean0) = (local_std(&img), pastelplash::report::mean_oklab(&img)[0]);
    let c0 = median(img.pixels.iter().map(|&p| lch(p)[1]).collect());
    for path in styles() {
        let config = load(&path, &default_target());
        let Some(out) = render(&path, &config, Category::World, &img) else {
            return;
        };
        let n = name(&path);
        let fine = config.style.value_contrast.fine;
        let std1 = local_std(&out);
        let want = std0 * (1.0 - k.technique.value_min_effect * fine);
        assert!(
            std1 <= want,
            "{n}: local L std {std0:.4} -> {std1:.4} (want <= {want:.4})"
        );
        let mean1 = pastelplash::report::mean_oklab(&out)[0];
        let bound = k.technique.value_mean_tolerance.max(k.identity.bound(&n));
        assert!(
            (mean1 - mean0).abs() <= bound,
            "{n}: mean L {mean0:.3} -> {mean1:.3}"
        );
        let c1 = median(out.pixels.iter().map(|&p| lch(p)[1]).collect());
        assert!(
            c1 >= k.palette.retained(c0),
            "{n}: median chroma {c0:.3} -> {c1:.3}"
        );
    }
}

#[test]
fn rule_actor_keeps_color_and_value() {
    // A pale peach skin-like actor texture keeps its hue and chroma (never gray or tint-safe)
    // and its lightness (held only to the target's ceiling).
    let k = contract();
    let img = pale_skin(128, 9);
    let c0 = median(img.pixels.iter().map(|&p| lch(p)[1]).collect());
    let l0 = median(img.pixels.iter().map(|&p| lch(p)[0]).collect());
    each_render(Category::Actor, &img, |name, _, config, out| {
        let ceiling = config
            .target
            .treatment(Category::Actor)
            .lightness_ceiling
            .unwrap_or(1.0);
        let c1 = median(out.pixels.iter().map(|&p| lch(p)[1]).collect());
        let l1 = median(out.pixels.iter().map(|&p| lch(p)[0]).collect());
        assert!(
            c1 >= k.actor.retention_ratio * c0,
            "{name}: actor skin lost color: C {c0:.3} -> {c1:.3}"
        );
        assert!(
            (l1 - l0.min(ceiling)).abs() <= k.actor.max_lightness_shift,
            "{name}: actor lightness {l0:.3} -> {l1:.3} (ceiling {ceiling})"
        );
    });
}

#[test]
fn rule_actor_lightness_ceiling() {
    let k = contract();
    // Very light, very saturated input (vivid candidates) as an actor texture.
    let img = image(128, 128, |x, y| {
        let [r, g, b] = from_oklch(0.75 + 0.2 * noise(x, y, 3), 0.25, x as f32 * 2.8);
        [r, g, b, 1.0]
    });
    each_render(Category::Actor, &img, |name, _, config, out| {
        let ceiling = config
            .target
            .treatment(Category::Actor)
            .lightness_ceiling
            .unwrap_or(1.0);
        assert_few(name, "above actor ceiling", out, |p| {
            lch(p)[0] > ceiling + k.tolerance.lightness
        });
    });
}

#[test]
fn rule_vivid_colors_are_bounded() {
    let k = contract();
    let img = image(128, 128, |x, y| {
        let [r, g, b] = from_oklch(0.4 + 0.4 * noise(x, y, 5), 0.3, x as f32 * 2.8);
        [r, g, b, 1.0]
    });
    each_render(Category::World, &img, |name, style, _, out| {
        // Brushwork and pooling may add a little chroma on top of the palette's cap; the
        // contract bounds the result.
        let cap = (style.palette.chroma_cap.max(style.palette.vivid_max_chroma) * 1.25)
            .min(k.vivid.max_chroma);
        assert_few(name, "chroma above cap", out, |p| {
            lch(p)[1] > cap + k.tolerance.chroma
        });
    });
}

#[test]
fn rule_tint_safe_grayscale_stays_gray() {
    let k = contract();
    for category in [Category::World, Category::Actor] {
        each_render(category, &grayscale(192, 2), |name, _, _, out| {
            assert_few(name, "gray input gained chroma", out, |p| {
                lch(p)[1] > k.tolerance.chroma * 0.5
            });
        });
    }
}

#[test]
fn rule_actor_has_no_temperature_shift() {
    // With the target's actor warm_cool at 0, turning the style's temperature off must not
    // change an actor render at all.
    let img = dark_hues(128, 4);
    for path in styles() {
        let config = load(&path, &default_target());
        let Some(with) = render(&path, &config, Category::Actor, &img) else {
            return;
        };
        let mut cold = config.clone();
        cold.style.temperature.chroma = 0.0;
        let without = render(&path, &cold, Category::Actor, &img).unwrap();
        assert_eq!(
            with.pixels,
            without.pixels,
            "{}: temperature changed an actor",
            name(&path)
        );
    }
}

#[test]
fn rule_alpha_preserved_and_no_halos() {
    let k = contract();
    let img = cutout(192, 3);
    each_render(Category::World, &img, |name, _, _, out| {
        for (a, b) in img.pixels.iter().zip(&out.pixels) {
            assert_eq!(a[3].to_bits(), b[3].to_bits(), "{name}: alpha changed");
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
        assert!(
            rim >= inner - 0.05 - k.tolerance.lightness,
            "{name}: dark halo: rim L {rim} vs interior {inner}"
        );
    });
}

#[test]
fn rule_tiling_textures_stay_seamless() {
    let img = tiling(256, 9);
    let before = [
        analysis::seam_ratio(&img, false),
        analysis::seam_ratio(&img, true),
    ];
    each_render(Category::World, &img, |name, style, _, out| {
        for (axis, &b) in before.iter().enumerate() {
            let after = analysis::seam_ratio(out, axis == 1);
            let limit = (b * 1.5).max(style.tiling.threshold);
            assert!(
                after <= limit,
                "{name}: seam ratio axis {axis}: {b} -> {after} (limit {limit})"
            );
        }
    });
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
    each_render(Category::World, &img, |name, _, _, out| {
        let w1 = edge_width(out);
        assert!(
            w1 <= k.technique.max_edge_width,
            "{name}: step edge widened {w0} -> {w1} texels"
        );
        let s1 = interior_std(out);
        assert!(
            s1 < s0,
            "{name}: flat-region noise did not decrease ({s0} -> {s1})"
        );
    });
}

#[test]
fn rule_deterministic() {
    let img = dark_foliage(128, 5);
    for path in styles() {
        let config = load(&path, &default_target());
        let Some(a) = render(&path, &config, Category::World, &img) else {
            return;
        };
        let b = render(&path, &config, Category::World, &img).unwrap();
        assert_eq!(a.pixels, b.pixels, "{}: nondeterministic", name(&path));
    }
}

#[test]
fn rule_output_in_gamut_and_finite() {
    for img in [dark_hues(128, 2), cutout(128, 1), grayscale(128, 3)] {
        each_render(Category::World, &img, |name, _, _, out| {
            for p in &out.pixels {
                assert!(
                    p.iter().all(|c| c.is_finite() && (0.0..=1.0).contains(c)),
                    "{name}: {p:?}"
                );
            }
        });
    }
}

#[test]
fn rule_identity_when_all_strengths_are_zero() {
    // A palette at strength 0 with every other effect off must leave the image unchanged.
    let path = repo().join("styles/watercolor.toml");
    let dir = tempfile::tempdir().unwrap();
    let neutral = dir.path().join("neutral.toml");
    std::fs::write(&neutral, "[palette]\nenabled = true\nstrength = 0.0\n").unwrap();
    let config = Config::load(
        Some(&neutral),
        Some(&repo().join("targets/soh-celshade.toml")),
        None,
    )
    .unwrap();
    let img = dark_hues(96, 8);
    let _ = path;
    // Categories with no ceiling: world.
    let Some(out) = render(&neutral, &config, Category::World, &img) else {
        return;
    };
    for (a, b) in img.pixels.iter().zip(&out.pixels) {
        for k in 0..4 {
            assert!((a[k] - b[k]).abs() < 2e-3, "{a:?} -> {b:?}");
        }
    }
}

#[test]
fn rule_sixteen_bit_inputs_are_handled() {
    use pastelplash::pipeline::Pipeline;
    use pastelplash::process::{self, Options};
    let style = repo().join("styles/watercolor.toml");
    if stylizer(&style).is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in");
    std::fs::create_dir_all(&input).unwrap();
    let mut img = dark_foliage(64, 1);
    img.source.bit_depth = 16;
    img.source.color = pastelplash::image::SourceColor::Rgb;
    img.source.has_alpha = false;
    pastelplash::png_io::write(&img, &input.join("a.png")).unwrap();
    let config = load(&style, &default_target());
    let pipeline = Pipeline::from_config(&config).unwrap();
    let opts = Options {
        input,
        output: dir.path().join("out"),
        category: Some(Category::World),
        ..Default::default()
    };
    let summary = process::run(&opts, &config, &pipeline).unwrap();
    assert_eq!((summary.processed, summary.failed), (1, 0));
    let out = pastelplash::png_io::read(&dir.path().join("out/a.png")).unwrap();
    assert_eq!(out.source.bit_depth, 16);
    assert!(out.pixels.iter().all(|p| p.iter().all(|c| c.is_finite())));
}

#[test]
fn rule_neutral_config_is_identity_without_a_gpu() {
    // The default (empty) config builds no stages, so it never needs a GPU.
    let config = Config::default();
    let pipeline = pastelplash::pipeline::Pipeline::from_config(&config).unwrap();
    let mut img = dark_hues(32, 1);
    let before = img.clone();
    let ctx = pastelplash::pipeline::FileContext {
        rel: std::path::Path::new("x.png"),
        category: Category::World,
        mood: Default::default(),
        config: &config,
    };
    pipeline.run(&mut img, &ctx).unwrap();
    assert_eq!(img, before);
}

#[test]
fn rule_chunked_processing_matches_whole_image() {
    // Textures above the device limit are processed in overlapping chunks; the seams must not
    // show. (Accent thresholds are per chunk, so accents are off for this comparison.)
    use pastelplash::pipeline::{FileContext, Stage};
    use pastelplash::stylize::Stylize;
    let style = repo().join("styles/watercolor.toml");
    if stylizer(&style).is_none() {
        return;
    }
    let mut config = load(&style, &default_target());
    config.style.palette.accent_fraction = 0.0;
    let img = tiling(320, 4);
    let run = |max: Option<u32>| {
        let stage = Stylize::with_max_chunk(&config, max).unwrap();
        let mut out = img.clone();
        let ctx = FileContext {
            rel: std::path::Path::new("c.png"),
            category: Category::World,
            mood: Default::default(),
            config: &config,
        };
        stage.apply(&mut out, &ctx).unwrap();
        out
    };
    let (whole, chunked) = (run(None), run(Some(160)));
    for (a, b) in whole.pixels.iter().zip(&chunked.pixels) {
        for k in 0..3 {
            assert!((a[k] - b[k]).abs() < 3e-3, "chunk seam: {a:?} vs {b:?}");
        }
    }
}
