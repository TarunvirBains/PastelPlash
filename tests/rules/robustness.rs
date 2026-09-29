//! Robustness: determinism, gamut, identity at zero strength, 16-bit files, chunking.

use super::*;

#[test]
fn rule_deterministic() {
    let img = dark_foliage(128, 5);
    let mut report = Report::new("deterministic");
    let rendered = Matrix::full(&STYLIZED).check(&mut report, &img, |case, a| {
        let b = case.render(&img).unwrap();
        ensure(a.pixels == b.pixels, || "nondeterministic".into())
    });
    if rendered {
        report.finish();
    }
}

#[test]
fn rule_output_in_gamut_and_finite() {
    let mut report = Report::new("output in gamut and finite");
    let matrix = Matrix::full(&STYLIZED);
    for (label, img) in [
        ("dark hues", dark_hues(128, 2)),
        ("cutout", cutout(128, 1)),
        ("grayscale", grayscale(128, 3)),
    ] {
        let rendered = matrix.check(&mut report, &img, |_, out| {
            for p in &out.pixels {
                ensure(
                    p.iter().all(|c| c.is_finite() && (0.0..=1.0).contains(c)),
                    || format!("{label}: {p:?}"),
                )?;
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
fn rule_identity_when_all_strengths_are_zero() {
    // A palette at strength 0 with every other effect off must leave the image unchanged.
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
    let style = repo().join(format!(
        "styles/{}.toml",
        pastelplash::config::DEFAULT_STYLE
    ));
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
        upscale: 1.0,
        source: None,
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
    // The watercolor base: with the impressionist brushwork overlay, strokes and temperature
    // still differ by up to ~0.006 at chunk borders (open issue; chunking only happens above the
    // device limit, 8192 texels per side, which no OoT Reloaded texture reaches).
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
            upscale: 1.0,
            source: None,
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

#[test]
fn rule_effects_are_left_untouched() {
    // Effects (glows, sparks, puffs, shadow blobs) are light, not paint: their gray is intensity
    // and falloff, often drawn additively. A soft gray radial glow the pack map didn't name is
    // left exactly as it was, as an actor or world texture. A hard-edged gray cutout that also
    // falls off radially (a bomb, a statue knob) is an object and is still restyled.
    let img = glow(128);
    let ball = gray_ball(128, 151);
    let mut report = Report::new("effects are left untouched");
    let matrix = Matrix::full(&[Category::Actor, Category::World]);
    let rendered = matrix.check(&mut report, &img, |_, out| {
        ensure(out.pixels == img.pixels, || "an effect was restyled".into())
    }) && matrix.check(&mut report, &ball, |_, out| {
        ensure(out.pixels != ball.pixels, || {
            "a hard-edged gray object was left untouched".into()
        })
    });
    if rendered {
        report.finish();
    }
}

#[test]
fn rule_no_new_clipping() {
    // Brushwork and the palette never clip: the share of texels with a channel at 0 or 255 (as
    // written to an 8-bit file) never grows from source to output. Small blown glints stay paper
    // white; large blown regions only darken.
    let clipped = |im: &Image| {
        im.pixels
            .iter()
            .filter(|p| p[3] > 0.5)
            .filter(|p| {
                p[..3].iter().any(|&c| {
                    let v = (c.clamp(0.0, 1.0) * 255.0).round();
                    v <= 0.0 || v >= 255.0
                })
            })
            .count() as f32
            / im.pixels.iter().filter(|p| p[3] > 0.5).count().max(1) as f32
    };
    let k = contract();
    let mut report = Report::new("no new clipping");
    let matrix = Matrix::full(&STYLIZED);
    for (label, img) in [
        ("highlights", highlights(192, 131)),
        ("dark hues", dark_hues(128, 132)),
        ("pale skin", pale_skin(128, 133)),
    ] {
        let s0 = clipped(&img);
        let rendered = matrix.check(&mut report, &img, |_, out| {
            let s1 = clipped(out);
            ensure(s1 <= s0 + k.tolerance.outliers, || {
                format!(
                    "{label}: clipped texels {:.2}% -> {:.2}%",
                    s0 * 100.0,
                    s1 * 100.0
                )
            })?;
            if label == "highlights" {
                // The compact glints on the mid-gray surface (not the big patch, not the spots
                // inside the near-white band: rule_no_glitter_on_bright_surfaces) stay white.
                let w = img.width as usize;
                for (i, (p, q)) in img.pixels.iter().zip(&out.pixels).enumerate() {
                    let (x, y) = ((i % w) as u32, (i / w) as u32);
                    if p[0] >= 1.0 && !(x > 144 && y < 64) && y < 128 {
                        ensure(q[..3].iter().all(|&c| c >= 254.0 / 255.0), || {
                            format!("a glint at ({x}, {y}) lost its white: {q:?}")
                        })?;
                    }
                }
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
fn rule_no_glitter_on_bright_surfaces() {
    // Blown-white spots inside an already bright surface (grit on pale cracked ground) are part of
    // the surface, not glints: they stand out no more than in the source (they read as glitter
    // once the painted surface around them settles). Glints on darker surfaces keep their white
    // (rule_no_new_clipping).
    let is_spot = |x: u32, y: u32| (x % 31 < 4) && (y % 27 < 4) && x > 8 && y > 8;
    let img = image(192, 192, |x, y| {
        if is_spot(x, y) {
            return [1.0, 1.0, 1.0, 1.0];
        }
        let t = smooth_noise(x as f32, y as f32, 6, 192, 181);
        let v = 0.84 + 0.1 * t + 0.03 * (noise(x, y, 182) - 0.5);
        [v, v, v * 0.97, 1.0]
    });
    let contrast = |im: &Image| {
        let (mut o, mut no, mut s, mut ns) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        for y in 10..182u32 {
            for x in 10..182u32 {
                let l = lch(im.pixels[(y * 192 + x) as usize])[0];
                if is_spot(x, y) {
                    o += l;
                    no += 1.0;
                } else if (x % 31) >= 6 && (x % 31) < 9 && (y % 27) < 4 {
                    s += l;
                    ns += 1.0;
                }
            }
        }
        o / no - s / ns
    };
    let c0 = contrast(&img);
    let mut report = Report::new("no glitter on bright surfaces");
    let rendered = Matrix::full(&[Category::World, Category::Background]).check(
        &mut report,
        &img,
        |_, out| {
            let c1 = contrast(out);
            ensure(c1 <= c0 + 0.01, || {
                format!("blown spots stand out more: {c0:.3} -> {c1:.3}")
            })
        },
    );
    if rendered {
        report.finish();
    }
}

#[test]
fn rule_no_glitter_on_light_textured_surfaces() {
    // Single blown-white texels scattered over a gritty, textured surface (the light treehouse
    // bark, spot04_room_0Tex_016508) are photographic highlights on grit, not glints: painted,
    // they stand alone as glitter. They blend into the surface: at most
    // technique.glitter_max_contrast lighter than the painted surface around them. A larger
    // highlight area on the same surface stays light.
    let k = contract();
    let is_spot = |x: u32, y: u32| {
        matches!((x % 13, y % 11), (4, 3) | (8, 3) | (4, 7)) && x > 8 && y > 8 && x < 120
    };
    let is_patch = |x: u32, y: u32| (140..156).contains(&x) && (40..56).contains(&y);
    let img = image(192, 192, |x, y| {
        if is_spot(x, y) || is_patch(x, y) {
            return [1.0, 1.0, 1.0, 1.0];
        }
        let v = 0.74
            + 0.1 * smooth_noise(x as f32, y as f32, 8, 192, 191)
            + 0.3 * (noise(x, y, 192) - 0.5);
        let [r, g, b] = from_oklch(v.clamp(0.3, 0.95), 0.03, 85.0);
        [r, g, b, 1.0]
    });
    let spot_contrast = |im: &Image| {
        let (mut d, mut n) = (0.0f32, 0.0f32);
        for y in 9..183u32 {
            for x in 9..183u32 {
                if !is_spot(x, y) {
                    continue;
                }
                let ring: f32 = [
                    (3i32, 0i32),
                    (-3, 0),
                    (0, 3),
                    (0, -3),
                    (2, 2),
                    (-2, 2),
                    (2, -2),
                    (-2, -2),
                ]
                .iter()
                .map(|&(dx, dy)| {
                    lch(im.pixels[((y as i32 + dy) * 192 + x as i32 + dx) as usize])[0]
                })
                .sum::<f32>()
                    / 8.0;
                d += lch(im.pixels[(y * 192 + x) as usize])[0] - ring;
                n += 1.0;
            }
        }
        d / n
    };
    let patch_l = |im: &Image| lch(im.pixels[(48 * 192 + 148) as usize])[0];
    let c0 = spot_contrast(&img);
    let mut report = Report::new("no glitter on light textured surfaces");
    let rendered = Matrix::full(&[Category::World, Category::Background]).check(
        &mut report,
        &img,
        |_, out| {
            let c1 = spot_contrast(out);
            let p = patch_l(out);
            ensure(c1 <= k.technique.glitter_max_contrast, || {
                format!(
                    "single blown texels stand out: {c1:.3} above the surface (source {c0:.3}, max {}; highlight L {p:.3})",
                    k.technique.glitter_max_contrast
                )
            })
        },
    );
    if rendered {
        report.finish();
    }
}
