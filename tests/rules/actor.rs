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
