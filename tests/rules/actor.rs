//! Actors (relit by the target's cel shader) and tint-safe textures.

use super::*;

/// The target's actor lightness ceiling.
fn ceiling(case: &Case) -> f32 {
    case.config
        .target
        .treatment(Category::Actor)
        .lightness_ceiling
        .unwrap_or(1.0)
}

#[test]
fn rule_actor_keeps_color_and_value() {
    // A pale peach skin-like actor texture keeps its hue and chroma (never gray or tint-safe)
    // and its lightness (held only to the target's ceiling).
    let k = contract();
    let img = pale_skin(128, 9);
    let c0 = median(img.pixels.iter().map(|&p| lch(p)[1]).collect());
    let l0 = median(img.pixels.iter().map(|&p| lch(p)[0]).collect());
    let mut report = Report::new("actor keeps color and value");
    let rendered = Matrix::full(&[Category::Actor]).check(&mut report, &img, |case, out| {
        let ceiling = ceiling(case);
        let c1 = median(out.pixels.iter().map(|&p| lch(p)[1]).collect());
        let l1 = median(out.pixels.iter().map(|&p| lch(p)[0]).collect());
        ensure(c1 >= k.actor.retention_ratio * c0, || {
            format!("actor skin lost color: C {c0:.3} -> {c1:.3}")
        })?;
        ensure(
            (l1 - l0.min(ceiling)).abs() <= k.actor.max_lightness_shift,
            || format!("actor lightness {l0:.3} -> {l1:.3} (ceiling {ceiling})"),
        )
    });
    if rendered {
        report.finish();
    }
}

#[test]
fn rule_actor_lightness_ceiling() {
    let k = contract();
    // Very light, very saturated input (vivid candidates) as an actor texture.
    let img = image(128, 128, |x, y| {
        let [r, g, b] = from_oklch(0.75 + 0.2 * noise(x, y, 3), 0.25, x as f32 * 2.8);
        [r, g, b, 1.0]
    });
    let mut report = Report::new("actor lightness ceiling");
    let rendered = Matrix::full(&[Category::Actor]).check(&mut report, &img, |case, out| {
        let ceiling = ceiling(case);
        few(out, 0.0, |p| lch(p)[0] > ceiling + k.tolerance.lightness)
            .map_err(|e| format!("above actor ceiling: {e}"))
    });
    if rendered {
        report.finish();
    }
}

#[test]
fn rule_tint_safe_grayscale_stays_gray() {
    let k = contract();
    let mut report = Report::new("tint-safe grayscale stays gray");
    let rendered = Matrix::full(&[Category::World, Category::Actor]).check(
        &mut report,
        &grayscale(192, 2),
        |_, out| {
            few(out, 0.0, |p| lch(p)[1] > k.tolerance.chroma * 0.5)
                .map_err(|e| format!("gray input gained chroma: {e}"))
        },
    );
    if rendered {
        report.finish();
    }
}

#[test]
fn rule_actor_has_no_temperature_shift() {
    // With the target's actor warm_cool at 0, turning the style's temperature off must not
    // change an actor render at all.
    let img = dark_hues(128, 4);
    let mut report = Report::new("actor has no temperature shift");
    for (label, path, config, mood) in style_moods() {
        let Some(with) = render_mood(&path, &config, Category::Actor, &mood, &img) else {
            return;
        };
        let cold = tweaked(&config, "temperature", "chroma", 0.0);
        let without = render_mood(&path, &cold, Category::Actor, &mood, &img).unwrap();
        report.check(
            &label,
            ensure(with.pixels == without.pixels, || {
                "temperature changed an actor".into()
            }),
        );
    }
    report.finish();
}

#[test]
fn rule_tint_safe_actors_are_raised_and_keep_their_folds() {
    // Engine-tinted grayscale actor textures (Link's tunic): the gray is raised toward the
    // target's tint-safe gray, painted with brightness-only strokes, never above the cap, and the
    // folds stay. The output stays gray (rule_tint_safe_grayscale_stays_gray).
    use pastelplash::report::coarse_l_pattern;
    let k = contract();
    let mut img = cloth_folds(192, 121);
    img.tint_safe = Some(true);
    let mut report = Report::new("tint-safe actors are raised and keep their folds");
    let rendered = Matrix::full(&[Category::Actor]).check(&mut report, &img, |case, out| {
        let Some(target) = case.config.target.treatment(Category::Actor).tint_safe_gray else {
            return Ok(());
        };
        let l1 = median(out.pixels.iter().map(|&p| lch(p)[0]).collect());
        ensure((l1 - target).abs() <= 0.05, || {
            format!("median gray {l1:.3} (target {target})")
        })?;
        few(out, 0.0, |p| {
            lch(p)[0] > k.actor.max_tint_safe_l + k.tolerance.lightness
        })
        .map_err(|e| format!("above the tint-safe cap: {e}"))?;
        let (corr, _) = coarse_l_pattern(&img, out, 16);
        ensure(corr >= k.actor.tint_safe_min_structure, || {
            format!("folds lost: coarse correlation {corr:.2}")
        })
    });
    if rendered {
        report.finish();
    }
}

#[test]
fn rule_actor_brushwork_makes_no_large_patches() {
    // The cel shader bands actors into lit and shadow at runtime: brushwork must not paint large
    // light or dark patches of its own. Cell-mean lightness (16x16) moves together.
    let k = contract();
    let img = pale_skin(192, 122);
    let cells = |im: &Image| {
        let n = 16usize;
        let (w, h) = (im.width as usize, im.height as usize);
        let mut acc = vec![[0.0f32; 2]; n * n];
        for (i, p) in im.pixels.iter().enumerate() {
            let c = &mut acc[(i / w) * n / h * n + (i % w) * n / w];
            c[0] += lch(*p)[0];
            c[1] += 1.0;
        }
        acc.iter().map(|c| c[0] / c[1]).collect::<Vec<f32>>()
    };
    let c0 = cells(&img);
    let mut report = Report::new("actor brushwork makes no large patches");
    let c0 = &c0;
    // Also with a prop's raised brushwork (pack-map `brushwork`, at its contract maximum).
    let check = |what: &'static str| {
        move |_: &Case, out: &Image| {
            let c1 = cells(out);
            let d: Vec<f32> = c0.iter().zip(&c1).map(|(a, b)| b - a).collect();
            let mean = d.iter().sum::<f32>() / d.len() as f32;
            let mut dev: Vec<f32> = d.iter().map(|v| (v - mean).abs()).collect();
            dev.sort_by(f32::total_cmp);
            let p95 = dev[(dev.len() - 1) * 95 / 100];
            ensure(p95 <= k.actor.max_patch_l, || {
                format!("{what}: cell lightness moved unevenly: p95 {p95:.3}")
            })
        }
    };
    let matrix = Matrix::full(&[Category::Actor]);
    let rendered = matrix.check(&mut report, &img, check("default"))
        && matrix.with_brushwork(k.actor.max_brushwork).check(
            &mut report,
            &img,
            check("prop brushwork"),
        );
    if rendered {
        report.finish();
    }
}

#[test]
fn rule_actor_colors_bring_no_new_hue() {
    // Props and characters keep the hues of their neighborhood: no blue flecks at a gold stud's
    // highlight, no teal at a band's edge. A colored output texel's hue stays within
    // actor.max_local_hue_change of its 9x9 source neighborhood's mean color (where that
    // neighborhood is clearly colored).
    let k = contract();
    let img = gold_studs(192, 171);
    let (w, h) = (img.width as i32, img.height as i32);
    let local: Vec<[f32; 3]> = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let mut s = [0.0f32; 3];
            for dy in -4..=4 {
                for dx in -4..=4 {
                    let p = img.pixels
                        [((y + dy).clamp(0, h - 1) * w + (x + dx).clamp(0, w - 1)) as usize];
                    let lab = pastelplash::color::srgb_to_oklab([p[0], p[1], p[2]]);
                    for k in 0..3 {
                        s[k] += lab[k] / 81.0;
                    }
                }
            }
            s
        })
        .collect();
    let mut report = Report::new("actor colors bring no new hue");
    let rendered = Matrix::full(&[Category::Actor]).check(&mut report, &img, |_, out| {
        let mut bad = 0usize;
        for (p, m) in out.pixels.iter().zip(&local) {
            let [_, c, hue] = lch(*p);
            let mc = m[1].hypot(m[2]);
            if c > 0.03 && mc > 0.05 {
                let mh = m[2].atan2(m[1]).to_degrees();
                if pastelplash::color::hue_diff(mh, hue).abs() > k.actor.max_local_hue_change {
                    bad += 1;
                }
            }
        }
        let share = bad as f32 / out.pixels.len() as f32;
        ensure(share <= k.tolerance.outliers, || {
            format!("{:.2}% of texels took a new hue", share * 100.0)
        })
    });
    if rendered {
        report.finish();
    }
}

#[test]
fn rule_solid_actors_read_painted() {
    // Solid actor surfaces (gear, props, clothing) sit in the painted world: their brushwork
    // (fine-scale lightness marks the stylization adds on a smooth, evenly colored surface) is at
    // least actor.min_mark_energy times the world's on the same surface (the world reads painted
    // mostly through its abstracted photographic detail, which smooth models lack).
    let k = contract();
    let img = image(192, 192, |x, y| {
        let t = smooth_noise(x as f32, y as f32, 3, 192, 201);
        let [r, g, b] = from_oklch(0.55 + 0.05 * t, 0.08, 140.0);
        [r, g, b, 1.0]
    });
    // Mean absolute difference between each texel's L and its 5x5 box mean.
    let marks = |im: &Image| {
        let (w, h) = (im.width as i32, im.height as i32);
        let l: Vec<f32> = im.pixels.iter().map(|&p| lch(p)[0]).collect();
        let mut sum = 0.0;
        let mut n = 0.0;
        for y in 4..h - 4 {
            for x in 4..w - 4 {
                let mut m = 0.0;
                for dy in -2..=2 {
                    for dx in -2..=2 {
                        m += l[((y + dy) * w + x + dx) as usize] / 25.0;
                    }
                }
                sum += (l[(y * w + x) as usize] - m).abs();
                n += 1.0;
            }
        }
        sum / n
    };
    let src = marks(&img);
    let mut report = Report::new("solid actors read painted");
    let world: std::collections::HashMap<String, f32> = {
        let m = std::cell::RefCell::new(std::collections::HashMap::new());
        let rendered = Matrix::base(&[Category::World]).check(&mut report, &img, |case, out| {
            m.borrow_mut().insert(case.style.clone(), marks(out) - src);
            Ok(())
        });
        if !rendered {
            return;
        }
        m.into_inner()
    };
    let rendered = Matrix::base(&[Category::Actor]).check(&mut report, &img, |case, out| {
        let (a, wv) = (marks(out) - src, world[&case.style]);
        ensure(a >= k.actor.min_mark_energy * wv, || {
            format!("actor marks {a:.4} vs world {wv:.4}")
        })
    });
    if rendered {
        report.finish();
    }
}

#[test]
fn rule_dark_tint_safe_actors_keep_their_value() {
    // The tint-safe raise gives light cloth headroom for the engine's tint (Link's tunic). A dark
    // engine-tinted texture (a dark plate with light lettering, a black silhouette) is raised to
    // at most actor.max_tint_safe_gain times its own median lightness, and its lettering keeps at
    // least technique.text_min_contrast of its contrast: no black Poe or fishing rod turned near
    // white, no lettering clipped away at the tint-safe cap.
    let k = contract();
    let plate = |size: u32, seed: u32, board: f32| {
        let mut img = image(size, size, |x, y| {
            let v = if is_letter(x, y) {
                0.9
            } else {
                board + 0.04 * (smooth_noise(x as f32, y as f32, 6, size, seed) - 0.5)
            };
            let [r, g, b] = from_oklch(v, 0.0, 0.0);
            [r, g, b, 1.0]
        });
        img.tint_safe = Some(true);
        img
    };
    let contrast = |im: &Image| {
        let (mut o, mut no, mut s, mut ns) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        for y in 4..252u32 {
            for x in 4..252u32 {
                let l = lch(im.pixels[(y * 256 + x) as usize])[0];
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
        o / no - s / ns
    };
    let mut report = Report::new("dark tint-safe actors keep their value");
    let matrix = Matrix::full(&[Category::Actor]);
    for (label, img) in [
        ("dark plate", plate(256, 211, 0.2)),
        ("black plate", plate(256, 212, 0.03)),
    ] {
        let m0 = median(img.pixels.iter().map(|&p| lch(p)[0]).collect());
        let c0 = contrast(&img);
        let rendered = matrix.check(&mut report, &img, |_, out| {
            let m1 = median(out.pixels.iter().map(|&p| lch(p)[0]).collect());
            let bound = k.actor.max_tint_safe_gain * m0 + 2.0 * k.tolerance.lightness;
            ensure(m1 <= bound, || {
                format!("{label}: median {m0:.3} raised to {m1:.3} (at most {bound:.3})")
            })?;
            let c1 = contrast(out);
            ensure(c1 >= k.technique.text_min_contrast * c0, || {
                format!("{label}: lettering contrast {c0:.3} -> {c1:.3}")
            })
        });
        if !rendered {
            return;
        }
    }
    report.finish();
}
