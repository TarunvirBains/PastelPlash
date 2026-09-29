//! Style rules checked on configuration and palette math (CPU only; runs everywhere).
//!
//! Every style in `styles/`, in every mood it defines, and every target in `targets/` is checked
//! against the bounds in `rules.toml`. See docs/RULES.md for what each rule protects and why.

mod common;

use std::sync::OnceLock;

use common::*;
use pastelplash::color;
use pastelplash::config::{Category, Config, Style};
use pastelplash::lut::Lut3d;
use pastelplash::palette::Mapping;
use proptest::prelude::*;

/// Every style × mood, resolved.
fn resolved() -> &'static [(String, Style)] {
    static R: OnceLock<Vec<(String, Style)>> = OnceLock::new();
    R.get_or_init(|| {
        style_moods()
            .into_iter()
            .map(|(label, path, config, mood)| {
                let style = config
                    .style
                    .for_mood(&mood)
                    .unwrap_or_else(|e| panic!("{}: {e:#}", path.display()));
                (label, style)
            })
            .collect()
    })
}

fn target() -> &'static pastelplash::config::Target {
    static T: OnceLock<pastelplash::config::Target> = OnceLock::new();
    T.get_or_init(|| {
        Config::load(None, Some(&default_target()), None)
            .unwrap()
            .target
    })
}

/// Each style × mood × (world, actor): the exact mapping's LUT, baked once.
fn luts() -> &'static [(String, Style, Category, Lut3d)] {
    static L: OnceLock<Vec<(String, Style, Category, Lut3d)>> = OnceLock::new();
    L.get_or_init(|| {
        let mut v = Vec::new();
        for (label, style) in resolved() {
            for cat in [Category::World, Category::Actor] {
                let lut = Mapping::new(&style.palette, &target().treatment(cat)).bake();
                v.push((format!("{label} {cat:?}"), style.clone(), cat, lut));
            }
        }
        v
    })
}

fn mapped_lch(lut: &Lut3d, rgb: [f32; 3]) -> [f32; 3] {
    let o = lut.sample(rgb);
    lch([o[0], o[1], o[2], 1.0])
}

#[test]
fn rule_styles_stay_within_the_contract() {
    let c = contract();
    for (name, s) in resolved() {
        let p = &s.palette;
        assert!(
            p.enabled || s.lut.is_some(),
            "{name}: styles must set a palette"
        );
        assert!(
            (c.palette.min_strength..=c.palette.max_strength).contains(&p.strength),
            "{name}: palette.strength {}",
            p.strength
        );
        assert!(
            s.watercolor.floor_margin <= c.palette.max_floor_margin,
            "{name}: floor_margin {}",
            s.watercolor.floor_margin
        );
        // Accents: bounded, cool, never below the contract's minimum lightness.
        assert!(
            p.accent_fraction <= c.accents.max_fraction,
            "{name}: accent_fraction {}",
            p.accent_fraction
        );
        assert!(
            in_hue_range(p.accent_hue, c.accents.hue),
            "{name}: accent_hue {}",
            p.accent_hue
        );
        assert!(
            p.accent_min_l >= c.accents.min_l,
            "{name}: accent_min_l {}",
            p.accent_min_l
        );
        assert!(
            p.accent_chroma <= c.accents.max_chroma,
            "{name}: accent_chroma"
        );
        assert!(p.vivid <= c.vivid.max_amount, "{name}: vivid {}", p.vivid);
        assert!(
            p.vivid_max_chroma <= c.vivid.max_chroma,
            "{name}: vivid_max_chroma"
        );
        let t = &c.technique;
        assert!(
            s.kuwahara.radius <= t.max_kuwahara_radius,
            "{name}: kuwahara.radius"
        );
        assert!(
            s.kuwahara.max_radius <= t.max_kuwahara_radius,
            "{name}: kuwahara.max_radius"
        );
        assert!(
            s.watercolor.edge_darkening <= t.max_edge_darkening,
            "{name}: edge_darkening"
        );
        assert!(
            s.watercolor.granulation <= t.max_granulation,
            "{name}: granulation"
        );
        assert!(
            s.watercolor.paper_grain <= t.max_paper_grain,
            "{name}: paper_grain"
        );
        assert!(
            s.watercolor.paper_tint <= t.max_paper_tint,
            "{name}: paper_tint"
        );
        assert!(
            s.strokes.strength <= t.max_stroke_strength,
            "{name}: strokes.strength"
        );
        assert!(s.strokes.smear <= t.max_smear, "{name}: strokes.smear");
        assert!(
            s.temperature.chroma <= t.max_temperature_chroma,
            "{name}: temperature"
        );
        let vc = &s.value_contrast;
        for (k, v) in [("fine", vc.fine), ("mid", vc.mid), ("coarse", vc.coarse)] {
            assert!((0.0..=1.0).contains(&v), "{name}: value_contrast.{k} {v}");
        }
    }
}

/// Palette keys the impressionist brushwork overlay may set (accents and a little vivid color).
const BRUSHWORK_PALETTE: [&str; 7] = [
    "accent_fraction",
    "accent_min_l",
    "accent_chroma",
    "accent_min_depth",
    "accent_radius",
    "vivid",
    "vivid_max_chroma",
];

fn own_table(rel: &str) -> toml::Table {
    toml::from_str(&std::fs::read_to_string(repo().join(rel)).unwrap()).unwrap()
}

/// `palette` with the brushwork overlay's keys taken from `from`.
fn without_brushwork(
    p: &pastelplash::config::Palette,
    from: &pastelplash::config::Palette,
) -> pastelplash::config::Palette {
    let mut p = p.clone();
    p.accent_fraction = from.accent_fraction;
    p.accent_min_l = from.accent_min_l;
    p.accent_chroma = from.accent_chroma;
    p.accent_min_depth = from.accent_min_depth;
    p.accent_radius = from.accent_radius;
    p.vivid = from.vivid;
    p.vivid_max_chroma = from.vivid_max_chroma;
    p
}

#[test]
fn impressionist_inherits_the_default_look() {
    // Impressionist is the watercolor base plus the brushwork overlay (bolder brushwork,
    // somewhat stronger warm/cool and accents). The overlay may only touch those, and the style
    // itself adds nothing, so the base's SS hue nudges, value compression and colored darks carry
    // over automatically.
    let overlay = own_table("styles/overlays/impressionist-brushwork.toml");
    for (section, keys) in &overlay {
        match section.as_str() {
            "kuwahara" | "strokes" | "temperature" | "watercolor" => {}
            "palette" => {
                for key in keys.as_table().unwrap().keys() {
                    assert!(
                        BRUSHWORK_PALETTE.contains(&key.as_str()),
                        "the brushwork overlay sets palette.{key}; only brushwork, warm/cool and \
                         accents may differ from the base"
                    );
                }
            }
            other => panic!("the brushwork overlay sets [{other}]"),
        }
    }
    let own = own_table("styles/impressionist.toml");
    let extends: Vec<&str> = own["extends"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(
        extends,
        ["watercolor.toml", "overlays/impressionist-brushwork.toml"]
    );
    assert!(
        own.keys().all(|k| k == "extends" || k == "name"),
        "impressionist adds its own settings; put brushwork in the overlay"
    );
    let base = Style::load(&repo().join("styles/watercolor.toml")).unwrap();
    let imp = Style::load(&repo().join("styles/impressionist.toml")).unwrap();
    assert_eq!(imp.value_contrast, base.value_contrast);
    assert_eq!(imp.grouping, base.grouping);
    assert_eq!(without_brushwork(&imp.palette, &base.palette), base.palette);
}

#[test]
fn ss_impressionist_is_ss_baseline_palette_with_impressionist_brushwork() {
    let own = own_table("styles/ss-impressionist.toml");
    assert!(
        own.keys().all(|k| k == "extends" || k == "name"),
        "ss-impressionist adds its own settings; it must only compose ss-baseline and the overlay"
    );
    let ssb = Style::load(&repo().join("styles/ss-baseline.toml")).unwrap();
    let imp = Style::load(&repo().join("styles/impressionist.toml")).unwrap();
    let ssi = Style::load(&repo().join("styles/ss-impressionist.toml")).unwrap();
    // Palette and color settings: ss-baseline's.
    assert_eq!(without_brushwork(&ssi.palette, &ssb.palette), ssb.palette);
    assert_eq!(ssi.value_contrast, ssb.value_contrast);
    assert_eq!(ssi.contrast, ssb.contrast);
    assert_eq!(ssi.grouping, ssb.grouping);
    assert_eq!(ssi.moods, ssb.moods);
    // Brushwork, warm/cool and accents: impressionist's.
    assert_eq!(ssi.kuwahara, imp.kuwahara);
    assert_eq!(ssi.strokes, imp.strokes);
    assert_eq!(ssi.temperature, imp.temperature);
    assert_eq!(ssi.watercolor, imp.watercolor);
    assert_eq!(without_brushwork(&imp.palette, &ssi.palette), imp.palette);
}

#[test]
fn rule_cool_darks_only_where_the_mood_allows() {
    // A cool bias on darks is the nocturne's Impressionist device; the base look and moods the
    // contract doesn't list must keep darks hue-true.
    let k = contract();
    for path in styles() {
        let style = Style::load(&path).unwrap();
        let n = name(&path);
        assert!(
            style.palette.dark_cool_bias <= k.max_cool_bias("base"),
            "{n}: base dark_cool_bias {}",
            style.palette.dark_cool_bias
        );
        for mood in style.moods.keys() {
            let m = style
                .for_mood(&pastelplash::config::Mood {
                    name: mood.clone(),
                    strength: 1.0,
                    dark_greens: None,
                })
                .unwrap();
            assert!(
                m.palette.dark_cool_bias <= k.max_cool_bias(mood),
                "{n} [{mood}]: dark_cool_bias {} > {}",
                m.palette.dark_cool_bias,
                k.max_cool_bias(mood)
            );
        }
    }
}

/// Every leaf key path of a TOML table, as dotted strings (array-of-tables entries by index).
fn leaves(t: &toml::Table, prefix: &str, out: &mut Vec<(String, toml::Value)>) {
    for (k, v) in t {
        let path = if prefix.is_empty() {
            k.clone()
        } else {
            format!("{prefix}.{k}")
        };
        match v {
            toml::Value::Table(sub) => leaves(sub, &path, out),
            toml::Value::Array(a) if a.iter().all(|x| x.is_table()) && !a.is_empty() => {
                for (i, x) in a.iter().enumerate() {
                    leaves(x.as_table().unwrap(), &format!("{path}[{i}]"), out);
                }
            }
            _ => out.push((path, v.clone())),
        }
    }
}

#[test]
fn moods_inherit_everything_they_do_not_override() {
    // A mood is an overlay: every setting it doesn't name equals the base style's, so base
    // improvements (abstraction, marks, contrast, strokes, ...) flow into it automatically.
    for path in styles() {
        let style = Style::load(&path).unwrap();
        let raw = style.raw.clone().unwrap();
        let mut base_leaves = Vec::new();
        leaves(&raw, "", &mut base_leaves);
        for (mood, over) in &style.moods {
            let mut overridden = Vec::new();
            leaves(over, "", &mut overridden);
            let over_keys: Vec<&String> = overridden.iter().map(|(k, _)| k).collect();
            let derived = pastelplash::config::layers::merge(&raw, over, 1.0).unwrap();
            let mut derived_leaves = Vec::new();
            leaves(&derived, "", &mut derived_leaves);
            for (key, value) in &base_leaves {
                if key.starts_with("moods.") || over_keys.contains(&key) {
                    continue;
                }
                let got = derived_leaves
                    .iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, v)| v);
                assert_eq!(
                    got,
                    Some(value),
                    "{} [{mood}]: {key} differs from the base style",
                    path.display()
                );
            }
            // And the resolved style is exactly that overlay.
            let m = style
                .for_mood(&pastelplash::config::Mood {
                    name: mood.clone(),
                    strength: 1.0,
                    dark_greens: None,
                })
                .unwrap();
            let mut table = derived.clone();
            table.remove("moods");
            let expected: Style = toml::Value::Table(table).try_into().unwrap();
            assert_eq!(m.abstraction, expected.abstraction, "{mood}");
            assert_eq!(m.marks, expected.marks, "{mood}");
            assert_eq!(m.contrast, expected.contrast, "{mood}");
            assert_eq!(m.strokes, expected.strokes, "{mood}");
            assert_eq!(m.palette, expected.palette, "{mood}");
        }
    }
}

#[test]
fn rule_actor_targets_leave_lighting_to_the_renderer() {
    let c = &contract().target;
    for path in targets() {
        let config = Config::load(None, Some(&path), None).unwrap();
        let actor = config.target.treatment(Category::Actor);
        let n = name(&path);
        let ceiling = actor.lightness_ceiling.unwrap_or(1.0);
        assert!(
            ceiling <= c.max_actor_ceiling,
            "{n}: actor lightness_ceiling {ceiling}"
        );
        assert!(
            actor.warm_cool <= c.max_actor_warm_cool,
            "{n}: actor warm_cool"
        );
        assert!(
            actor.shadow_tint <= c.max_actor_shadow_tint,
            "{n}: actor shadow_tint"
        );
        assert!(
            actor.delight <= c.max_actor_delight,
            "{n}: actor delight {}",
            actor.delight
        );
        assert!(
            actor.floor_scale <= c.max_actor_lift,
            "{n}: actor floor_scale (lift)"
        );
        assert!(actor.hue <= c.max_actor_hue, "{n}: actor hue {}", actor.hue);
    }
}

#[test]
fn rule_palette_output_is_in_gamut_and_finite() {
    for (name, _, _, lut) in luts() {
        for v in &lut.data {
            assert!(
                v.iter().all(|c| c.is_finite() && (0.0..=1.0).contains(c)),
                "{name}: {v:?}"
            );
        }
    }
}

#[test]
fn rule_palette_is_identity_at_zero_strength() {
    for path in styles() {
        let mut s = Style::load(&path).unwrap();
        s.palette.strength = 0.0;
        let lut = Mapping::new(&s.palette, &Default::default()).bake();
        let identity = Lut3d::identity(lut.size);
        for (a, b) in lut.data.iter().zip(&identity.data) {
            for k in 0..3 {
                assert!(
                    (a[k] - b[k]).abs() < 2e-3,
                    "{}: {a:?} vs {b:?}",
                    path.display()
                );
            }
        }
    }
}

#[test]
fn rule_value_order_preserved() {
    // Lightness ramps (with the chroma a shaded surface has) must map monotonically, so value
    // structure stays readable. Checked on the exact mapping; LUT fidelity is checked below.
    let tol = contract().tolerance.lightness;
    for (name, style) in resolved() {
        let m = Mapping::new(&style.palette, &target().treatment(Category::World));
        for h in (0..360).step_by(15) {
            for c in [0.0, 0.03, 0.08] {
                let mut prev = -1.0;
                for i in 0..=80 {
                    let l = i as f32 / 80.0;
                    let rgb = from_oklch(l, c * l.min(1.0 - l) * 4.0, h as f32);
                    let out = lch(m.map(rgb))[0];
                    assert!(
                        out >= prev - tol,
                        "{name}: h {h} c {c}: L {l} -> {out} after {prev}"
                    );
                    prev = out;
                }
            }
        }
    }
}

#[test]
fn lut_matches_the_mapping_outside_the_darkest_cell() {
    // The GPU uses the baked LUT; away from near-black (where hue families meet within one
    // lattice cell) it must agree with the exact mapping. Near-grays, where the mapping moves
    // between the neutral and a hue group's curve within a few lattice steps, get extra slack.
    let tol = contract().tolerance.lightness * 2.5;
    for (name, style, cat, lut) in luts() {
        let m = Mapping::new(&style.palette, &target().treatment(*cat));
        for i in 0..2000u32 {
            let rgb = [noise(i, 0, 1), noise(i, 1, 2), noise(i, 2, 3)];
            if rgb.iter().cloned().fold(0.0, f32::max) < 2.0 / (lut.size - 1) as f32 {
                continue;
            }
            let a = lch(lut.sample(rgb))[0];
            let b = lch(m.map(rgb))[0];
            assert!(
                (a - b).abs() <= tol,
                "{name}: {rgb:?}: LUT L {a} vs exact {b}"
            );
        }
    }
}

/// Deterministic property tests: a fixed seed (so CI and local runs see the same cases) and
/// enough cases to cover the color cube well. Failures found are also persisted in
/// `tests/rules_config.proptest-regressions` and replayed first on every run.
/// `PASTELPLASH_PROPTEST_CASES` raises the count for a deeper local sweep.
fn proptest_config() -> ProptestConfig {
    let cases = std::env::var("PASTELPLASH_PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4000);
    ProptestConfig {
        cases,
        rng_seed: proptest::test_runner::RngSeed::Fixed(0x5041_5354_454C),
        ..ProptestConfig::default()
    }
}

#[test]
fn rule_hued_near_black_darks_keep_their_hue() {
    // Regression (found by proptest): a near-black navy has tiny absolute chroma but is clearly
    // blue; it was treated as neutral and warmed into umber "mud".
    let k = contract();
    for rgb in [
        [0.016155554, 0.02241493, 0.045453776], // near-black navy
        [0.045, 0.02, 0.012],                   // near-black red-brown
        [0.012, 0.03, 0.014],                   // near-black green
    ] {
        let [_, _, h_src] = lch([rgb[0], rgb[1], rgb[2], 1.0]);
        for (name, _, _, lut) in luts() {
            let out = mapped_lch(lut, rgb);
            assert!(
                !k.palette.is_mud(out),
                "{name}: {rgb:?} -> {out:?} (brown mud)"
            );
            if out[1] >= 0.02 {
                let d = color::hue_diff(h_src, out[2]).abs();
                assert!(
                    d <= k.dark_max_hue_shift(name),
                    "{name}: {rgb:?} hue {h_src} -> {out:?}"
                );
            }
        }
    }
}

proptest! {
    #![proptest_config(proptest_config())]

    #[test]
    fn rule_darks_are_colored_never_black(r in 0.0f32..1.0, g in 0.0f32..1.0, b in 0.0f32..1.0) {
        let k = contract();
        for (name, _, _, lut) in luts() {
            let [l, c, h] = mapped_lch(lut, [r, g, b]);
            prop_assert!(l >= k.palette.min_l - k.tolerance.lightness,
                "{}: {:?} -> L {} (crushed black)", name, [r, g, b], l);
            if l < k.palette.dark_l {
                prop_assert!(c >= k.palette.dark_min_chroma - 1e-3,
                    "{}: {:?} -> L {} C {} h {} (neutral dark)", name, [r, g, b], l, c, h);
            }
        }
    }

    #[test]
    fn rule_no_brown_mud(r in 0.0f32..1.0, g in 0.0f32..1.0, b in 0.0f32..1.0) {
        let k = contract();
        for (name, _, _, lut) in luts() {
            let out = mapped_lch(lut, [r, g, b]);
            prop_assert!(!k.palette.is_mud(out), "{}: {:?} -> {:?} (brown mud)", name, [r, g, b], out);
        }
    }

    #[test]
    fn rule_pastel_is_not_gray(l in 0.15f32..0.95, c in 0.05f32..0.25, h in 0.0f32..360.0) {
        // Colored sources keep their color in every style: lifted colors never go chalky.
        let k = contract();
        let rgb = from_oklch(l, c, h);
        let [_, c_src, _] = lch([rgb[0], rgb[1], rgb[2], 1.0]);
        prop_assume!(c_src >= k.palette.retention_min_source_chroma);
        for (name, _, _, lut) in luts() {
            let [lo, co, ho] = mapped_lch(lut, rgb);
            prop_assert!(co >= k.palette.retained(c_src) - k.tolerance.chroma,
                "{}: C {} -> {} (L {}, h {}) lost color", name, c_src, co, lo, ho);
        }
    }

    #[test]
    fn rule_hue_shifts_are_bounded(l in 0.2f32..0.9, c in 0.06f32..0.2, h in 0.0f32..360.0) {
        let k = contract();
        let rgb = from_oklch(l, c, h);
        let [_, c_src, h_src] = lch([rgb[0], rgb[1], rgb[2], 1.0]);
        prop_assume!(c_src >= 0.06);
        for (name, _, _, lut) in luts() {
            let [_, co, ho] = mapped_lch(lut, rgb);
            if co < 0.05 {
                continue;
            }
            let d = color::hue_diff(h_src, ho).abs();
            prop_assert!(d <= k.palette.max_hue_shift + 2.0,
                "{}: hue {} -> {} ({} degrees)", name, h_src, ho, d);
        }
    }

    #[test]
    fn rule_lifted_darks_keep_their_hue(l in 0.04f32..0.35, c in 0.03f32..0.15, h in 0.0f32..360.0) {
        // Dark brown stays brown, dark green stays green: never shifted toward blue.
        let k = contract();
        let rgb = from_oklch(l, c, h);
        let [l_src, c_src, h_src] = lch([rgb[0], rgb[1], rgb[2], 1.0]);
        // Many dark saturated OKLCH colors are outside sRGB; skip those without counting a reject.
        if l_src >= 0.35 || c_src < 0.03 {
            return Ok(());
        }
        for (name, _, _, lut) in luts() {
            let [_, co, ho] = mapped_lch(lut, rgb);
            if co < 0.02 {
                continue;
            }
            let d = color::hue_diff(h_src, ho).abs();
            prop_assert!(d <= k.dark_max_hue_shift(name) + 2.0,
                "{}: dark L {} C {} hue {} -> {} ({} degrees)", name, l_src, c_src, h_src, ho, d);
        }
    }

    #[test]
    fn rule_warmth_is_targeted(l in 0.1f32..0.95, c in 0.0f32..0.2, h in 0.0f32..360.0) {
        // Outside the style's warmth band, the warmth has no effect at all.
        let rgb = from_oklch(l, c, h);
        let [_, _, h_src] = lch([rgb[0], rgb[1], rgb[2], 1.0]);
        for (name, style) in resolved() {
            let w = &style.palette.warmth;
            if w.strength <= 0.0 {
                continue;
            }
            let [from, to] = w.band;
            if (h_src - from).rem_euclid(360.0) < (to - from).rem_euclid(360.0) {
                continue;
            }
            for cat in [Category::World, Category::Actor] {
                let tr = target().treatment(cat);
                let on = Mapping::new(&style.palette, &tr).map(rgb);
                let mut cold = tr.clone();
                cold.warmth = 0.0;
                let off = Mapping::new(&style.palette, &cold).map(rgb);
                prop_assert_eq!(on, off, "{} {:?}: warmth changed hue {} outside its band", name, cat, h_src);
            }
        }
    }

    #[test]
    fn rule_warmth_stays_in_band(l in 0.15f32..0.9, c in 0.03f32..0.2, h in 40.0f32..125.0) {
        // Warmth pulls earth hues toward its target but never out of the band: compared with the
        // same mapping without warmth, the output hue moves toward the target and never overshoots.
        let rgb = from_oklch(l, c, h);
        let [_, c_src, h_src] = lch([rgb[0], rgb[1], rgb[2], 1.0]);
        prop_assume!(c_src >= 0.03);
        for (name, style) in resolved() {
            let w = &style.palette.warmth;
            if w.strength <= 0.0 {
                continue;
            }
            let [from, to] = w.band;
            let inside = |hh: f32| (hh - from).rem_euclid(360.0) <= (to - from).rem_euclid(360.0);
            if !(inside(h_src)) { continue; }
            let tr = target().treatment(Category::World);
            let mut cold = tr.clone();
            cold.warmth = 0.0;
            // Warmth in isolation: a mood's cool bias on darks is a separate, later rotation.
            let mut pal = style.palette.clone();
            pal.dark_cool_bias = 0.0;
            let on = lch(Mapping::new(&pal, &tr).map(rgb));
            let off = lch(Mapping::new(&pal, &cold).map(rgb));
            if !(off[1] >= 0.02 && on[1] >= 0.02) { continue; }
            let (d_off, d_on) = (color::hue_diff(off[2], w.hue), color::hue_diff(on[2], w.hue));
            prop_assert!(d_on.abs() <= d_off.abs() + 1.0 && (d_on * d_off >= -1e-3 || d_on.abs() < 1.0),
                "{}: hue {} -> {} without warmth, {} with (target {})", name, h_src, off[2], on[2], w.hue);
            if inside(off[2]) {
                prop_assert!(inside(on[2]) || color::hue_diff(on[2], w.hue).abs() < 1.0,
                    "{}: warmth moved hue {} out of the band ({})", name, off[2], on[2]);
            }
        }
    }

    #[test]
    fn oklch_round_trip_is_accurate(r in 0.0f32..1.0, g in 0.0f32..1.0, b in 0.0f32..1.0) {
        let lab = color::srgb_to_oklab([r, g, b]);
        let back = color::oklab_to_srgb(color::oklch_to_oklab(color::oklab_to_oklch(lab)));
        for (x, y) in back.iter().zip([r, g, b]) {
            prop_assert!((x - y).abs() < 1e-4);
        }
    }

    #[test]
    fn cube_round_trip(size in 2usize..10, seed in 0u32..1000) {
        let lut = Lut3d::bake(size, |[r, g, b]| [
            noise((r * 100.0) as u32, (g * 100.0) as u32, seed),
            b, r * g, 0.0,
        ]);
        let back = Lut3d::parse_cube(&lut.to_cube("t")).unwrap();
        prop_assert_eq!(back.size, size);
        for (a, b) in lut.data.iter().zip(&back.data) {
            for k in 0..3 {
                prop_assert!((a[k] - b[k]).abs() < 1e-5);
            }
        }
    }
}

#[test]
fn rule_unknown_keys_are_rejected() {
    for text in [
        "[palette]\nl_flor = 0.6",
        "[watercolour]\nbleed = 0.2",
        "[[palette.groups]]\nname = 'g'\nhue_rnage = [1, 2]",
    ] {
        assert!(toml::from_str::<Style>(text).is_err(), "{text}");
    }
}

#[test]
fn rule_invalid_ranges_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.toml");
    std::fs::write(&path, "[palette]\nl_floor = 0.9\nl_ceiling = 0.7").unwrap();
    let err = format!("{:#}", Config::load(Some(&path), None, None).unwrap_err());
    assert!(
        err.contains("l_floor") && err.contains("l_ceiling"),
        "{err}"
    );
}

#[test]
fn builtin_styles_are_the_shipped_files() {
    // The binary carries every shipped style (so `--style impressionist` and the default work
    // from anywhere); each must equal its file in `styles/`, and none may be missing.
    use pastelplash::config::DEFAULT_STYLE;
    let names = Style::builtin_names();
    assert!(
        names.contains(&DEFAULT_STYLE),
        "default {DEFAULT_STYLE} is not built in"
    );
    for path in styles() {
        let n = name(&path);
        assert!(
            names.contains(&n.as_str()),
            "styles/{n}.toml is not built in"
        );
        let mut a = Style::builtin(&n).unwrap();
        let mut b = Style::load(&path).unwrap();
        (a.raw, b.raw) = (None, None);
        assert_eq!(a, b, "built-in {n} differs from styles/{n}.toml");
    }
    let config = Config::load(Some(std::path::Path::new(DEFAULT_STYLE)), None, None).unwrap();
    assert_eq!(config.style.name, DEFAULT_STYLE);
}

proptest! {
    #![proptest_config(proptest_config())]

    #[test]
    fn rule_exposure_curve_is_monotone_and_restores_the_mean(
        lo in 0.0f32..0.4,
        width in 0.1f32..0.6,
        seed in 0u32..1000,
        lift in -0.08f32..0.08,
    ) {
        // The background exposure curve never reorders values (slope ≥ MIN_SLOPE everywhere,
        // black and white fixed) and, when the needed correction is within its bounds, brings the
        // mean lightness back to the source's.
        use pastelplash::exposure::{self, ToneCurve, MIN_SLOPE};
        let protect = [lo, (lo + width).min(1.0)];
        let (kmin, kmax) = ToneCurve::k_bounds(protect);
        for k in [kmin.max(-1.0), 0.0, kmax.min(1.0)] {
            let c = ToneCurve { k, protect };
            let mut prev = c.apply(0.0);
            prop_assert!(prev.abs() < 1e-6);
            for i in 1..=512 {
                let v = c.apply(i as f32 / 512.0);
                prop_assert!(v - prev >= MIN_SLOPE / 512.0 - 1e-5, "k {}: slope at {}", k, i);
                prev = v;
            }
            prop_assert!((prev - 1.0).abs() < 1e-5);
        }
        let src: Vec<[f32; 4]> = (0..400u32)
            .map(|i| {
                let v = 0.05 + 0.9 * ((i.wrapping_mul(2_654_435_761) ^ seed) % 1000) as f32 / 1000.0;
                [v, v * 0.9, v * 0.7, 1.0]
            })
            .collect();
        let target = exposure::mean_l(&src);
        let mut out: Vec<[f32; 4]> = src
            .iter()
            .map(|p| {
                let [l, a, b] = color::srgb_to_oklab([p[0], p[1], p[2]]);
                let rgb = color::oklab_to_srgb([(l + lift * (1.0 - l)).clamp(0.0, 1.0), a, b]);
                [rgb[0].clamp(0.0, 1.0), rgb[1].clamp(0.0, 1.0), rgb[2].clamp(0.0, 1.0), 1.0]
            })
            .collect();
        let curve = exposure::preserve_mean(&mut out, target, protect);
        if curve.k > kmin + 1e-4 && curve.k < kmax - 1e-4 {
            prop_assert!((exposure::mean_l(&out) - target).abs() < 2e-3,
                "mean {} vs target {}", exposure::mean_l(&out), target);
        }
    }
}
