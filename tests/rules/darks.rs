//! Darks: colored, never crushed black, never brown mud, hue-true.

use super::*;

#[test]
fn rule_darks_are_colored_never_black() {
    let k = contract();
    let mut report = Report::new("darks are colored, never black");
    let matrix = Matrix::full(&[Category::World]);
    for (label, img) in [
        ("dark hues", dark_hues(192, 1)),
        ("dark foliage", dark_foliage(192, 2)),
        ("crushed neutrals", grayscale_dark(192, 3)),
    ] {
        let rendered = matrix.check(&mut report, &img, |_, out| {
            few(out, 0.0, |p| {
                lch(p)[0] < k.palette.min_l - k.tolerance.lightness
            })
            .map_err(|e| format!("{label}: crushed black: {e}"))?;
            few(out, 0.0, |p| {
                let [l, c, _] = lch(p);
                l < k.palette.dark_l && c < k.palette.dark_min_chroma - 1e-3
            })
            .map_err(|e| format!("{label}: neutral dark: {e}"))
        });
        if !rendered {
            return;
        }
    }
    report.finish();
}

#[test]
fn rule_no_brown_mud() {
    let k = contract();
    let mut report = Report::new("no brown mud");
    let rendered =
        Matrix::full(&[Category::World]).check(&mut report, &dull_browns(192, 4), |_, out| {
            few(out, 0.0, |p| k.palette.is_mud(lch(p)))
        });
    if rendered {
        report.finish();
    }
}

#[test]
fn rule_bark_does_not_turn_blue() {
    // Regression: v2 lifted bark and cliff darks toward navy. Dark bark (brown, and near-neutral
    // olive-gray with near-black grooves) must keep a warm hue in every style.
    let mut report = Report::new("bark does not turn blue");
    let matrix = Matrix::full(&[Category::World]);
    for (label, img) in [
        ("dark brown bark", dark_brown_bark(192, 1)),
        ("bark", bark(192, 2)),
    ] {
        let rendered = matrix.check(&mut report, &img, |case, out| {
            let style = case.config.style.for_mood(&case.mood).unwrap();
            few(out, style.palette.accent_fraction, |p| {
                let [l, c, h] = lch(p);
                l < 0.45 && c >= 0.02 && (200.0..320.0).contains(&h)
            })
            .map_err(|e| format!("{label}: dark bark turned blue: {e}"))
        });
        if !rendered {
            return;
        }
    }
    report.finish();
}

#[test]
fn rule_near_black_darks_take_a_muted_midnight() {
    // In moods with a moonlight cast, near-black and near-neutral darks take at most a muted
    // midnight: a small lift (they stay the darkest part; fades to black stay fades) and chroma
    // capped in proportion to lightness, unless held at the no-mud chroma as a warm dark. Never
    // ink.
    let k = contract();
    let img = near_neutral_darks(192, 91);
    let mut report = Report::new("near-black darks take a muted midnight");
    let rendered = Matrix::full(&[Category::World, Category::Background]).check(
        &mut report,
        &img,
        |case, out| {
            let Some(r) = k.mood(&case.mood.name) else {
                return Ok(());
            };
            let (Some(bl), Some(nc), Some(lift), Some(per_l)) = (
                r.near_black_l,
                r.near_neutral_c,
                r.near_black_max_lift,
                r.dark_chroma_per_l,
            ) else {
                return Ok(());
            };
            let (mut n, mut lifted, mut ink) = (0usize, 0usize, 0usize);
            let mut example = None;
            for (p, q) in img.pixels.iter().zip(&out.pixels) {
                let [l0, c0, _] = lch(*p);
                if l0 >= bl || c0 >= nc {
                    continue;
                }
                n += 1;
                let [l1, c1, h1] = lch(*q);
                if l1 > l0.max(k.palette.min_l) + lift + k.tolerance.lightness {
                    lifted += 1;
                    example.get_or_insert(format!("L {l0:.3} -> {l1:.3}"));
                }
                let warm_hold = in_hue_range(h1, k.palette.mud_hue);
                if !warm_hold && c1 > per_l * l1 + k.tolerance.chroma {
                    ink += 1;
                    example.get_or_insert(format!("L {l1:.3} C {c1:.3} h {h1:.0}"));
                }
            }
            let budget = (n as f32 * k.tolerance.outliers).ceil() as usize;
            ensure(lifted <= budget && ink <= budget, || {
                format!(
                    "{lifted} lifted too far, {ink} too saturated of {n} near-black texels \
                     (budget {budget}); e.g. {}",
                    example.unwrap_or_default()
                )
            })
        },
    );
    if rendered {
        report.finish();
    }
}
