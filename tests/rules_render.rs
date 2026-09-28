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

fn green_floor(style: &Style) -> f32 {
    let green = contract().palette.green_hue;
    let mid = (green[0] + green[1]) / 2.0;
    style
        .palette
        .groups
        .iter()
        .filter(|g| in_hue_range(mid, g.hue_range))
        .map(|g| g.l_floor)
        .fold(f32::NAN, f32::min)
}

/// Fails if more than the contract's outlier budget of opaque texels fail `bad`.
fn assert_few(name: &str, rule: &str, out: &Image, bad: impl Fn([f32; 4]) -> bool) {
    let opaque: Vec<[f32; 4]> = out.pixels.iter().copied().filter(|p| p[3] > 0.5).collect();
    let failing: Vec<[f32; 4]> = opaque.iter().copied().filter(|&p| bad(p)).collect();
    let budget = (opaque.len() as f32 * contract().tolerance.outliers).ceil() as usize;
    assert!(
        failing.len() <= budget,
        "{name}: {rule}: {} of {} texels fail (budget {budget}); e.g. {:?} = LCh {:?}",
        failing.len(),
        opaque.len(),
        failing[0],
        lch(failing[0])
    );
}

fn is_accent(style: &Style, [_, c, h]: [f32; 3]) -> bool {
    c < 0.02 || in_hue_range(h, contract().accents.hue) || {
        let d = (h - style.palette.accent_hue + 540.0).rem_euclid(360.0) - 180.0;
        d.abs() < 45.0
    }
}

#[test]
fn rule_no_dark_greens() {
    let k = contract();
    for seed in 0..3 {
        each_render(
            Category::World,
            &dark_foliage(256, seed),
            |name, style, _, out| {
                let floor =
                    green_floor(style) - style.watercolor.floor_margin - k.tolerance.lightness;
                assert_few(name, "dark green", out, |p| {
                    let [l, c, h] = lch(p);
                    c >= k.palette.green_min_chroma
                        && in_hue_range(h, k.palette.green_hue)
                        && l < floor
                });
            },
        );
    }
}

#[test]
fn rule_all_hue_pastel_floor_except_accents() {
    let k = contract();
    each_render(
        Category::World,
        &dark_hues(256, 1),
        |name, style, _, out| {
            let floor =
                style.palette.l_floor - style.watercolor.floor_margin - k.tolerance.lightness;
            assert_few(name, "below floor and not an accent", out, |p| {
                let v = lch(p);
                v[0] < floor && !is_accent(style, v)
            });
            // Accent darks: bounded in count and depth, and cool.
            let dark: Vec<[f32; 3]> = out
                .pixels
                .iter()
                .map(|&p| lch(p))
                .filter(|v| v[0] < floor)
                .collect();
            let frac = dark.len() as f32 / out.pixels.len() as f32;
            assert!(
                frac <= style.palette.accent_fraction + k.tolerance.outliers * 5.0,
                "{name}: {frac} of texels below the floor (accent_fraction {})",
                style.palette.accent_fraction
            );
            for v in dark {
                assert!(
                    v[0] >= style.palette.accent_min_l - k.tolerance.lightness,
                    "{name}: accent too dark: {v:?}"
                );
            }
        },
    );
}

#[test]
fn rule_accents_are_never_green() {
    let k = contract();
    each_render(
        Category::World,
        &dark_foliage(256, 7),
        |name, style, _, out| {
            // Anything that got darker than the green floor must have left the green family.
            let floor = green_floor(style) - style.watercolor.floor_margin - k.tolerance.lightness;
            assert_few(name, "green accent", out, |p| {
                let [l, c, h] = lch(p);
                l < floor && c >= k.palette.green_min_chroma && in_hue_range(h, k.palette.green_hue)
            });
        },
    );
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
        let cap = style
            .palette
            .chroma_cap
            .max(style.palette.vivid_max_chroma)
            .min(k.vivid.max_chroma);
        // Watercolor pooling may add a little chroma on top of the palette's cap.
        assert_few(name, "chroma above cap", out, |p| {
            lch(p)[1] > cap * 1.15 + k.tolerance.chroma
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
    let path = repo().join("styles/skyward-watercolor.toml");
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
    let style = repo().join("styles/skyward-watercolor.toml");
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
    let style = repo().join("styles/skyward-watercolor.toml");
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
            assert!((a[k] - b[k]).abs() < 1e-3, "chunk seam: {a:?} vs {b:?}");
        }
    }
}
