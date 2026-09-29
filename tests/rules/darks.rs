//! Darks: colored, never crushed black, never brown mud, hue-true.

use super::*;

#[test]
fn rule_darks_are_colored_never_black() {
    let k = contract();
    let mut report = Report::new("darks are colored, never black");
    let matrix = Matrix::base(&[Category::World]);
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
        Matrix::base(&[Category::World]).check(&mut report, &dull_browns(192, 4), |_, out| {
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
    let matrix = Matrix::base(&[Category::World]);
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
