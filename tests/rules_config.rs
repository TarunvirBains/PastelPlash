//! Style rules checked on configuration and palette math (CPU only; runs everywhere).
//!
//! Every style in `styles/` and every target in `targets/` is checked against the bounds in
//! `rules.toml`. See docs/RULES.md for what each rule protects and why.

mod common;

use common::*;
use pastelplash::color;
use pastelplash::config::{Category, Config, Style};
use pastelplash::lut::Lut3d;
use pastelplash::palette::Mapping;
use proptest::prelude::*;

fn each_style(mut f: impl FnMut(&str, &Style)) {
    for path in styles() {
        let config = Config::load(Some(&path), None, None)
            .unwrap_or_else(|e| panic!("{}: {e:#}", path.display()));
        f(&name(&path), &config.style);
    }
}

fn green_group_floor(style: &Style) -> f32 {
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

#[test]
fn rule_styles_stay_within_the_contract() {
    let c = contract();
    each_style(|name, s| {
        let p = &s.palette;
        assert!(
            p.enabled || s.lut.is_some(),
            "{name}: styles must set a palette"
        );
        assert!(
            (c.palette.min_strength..=c.palette.max_strength).contains(&p.strength),
            "{name}: palette.strength {} outside {}..={}",
            p.strength,
            c.palette.min_strength,
            c.palette.max_strength
        );
        assert!(
            p.l_floor >= c.palette.min_floor,
            "{name}: palette.l_floor {} < {}",
            p.l_floor,
            c.palette.min_floor
        );
        for g in &p.groups {
            assert!(
                g.l_floor >= c.palette.min_floor,
                "{name}: group {} l_floor {} < {}",
                g.name,
                g.l_floor,
                c.palette.min_floor
            );
        }
        let gf = green_group_floor(s);
        assert!(
            gf >= c.palette.min_green_floor,
            "{name}: green floor {gf} < {}",
            c.palette.min_green_floor
        );
        assert!(
            p.l_ceiling <= c.palette.max_ceiling,
            "{name}: l_ceiling {}",
            p.l_ceiling
        );
        assert!(
            p.chroma_cap <= c.palette.max_chroma_cap,
            "{name}: chroma_cap {}",
            p.chroma_cap
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
            "{name}: accent_chroma {}",
            p.accent_chroma
        );
        // Vivid colors: bounded.
        assert!(p.vivid <= c.vivid.max_amount, "{name}: vivid {}", p.vivid);
        assert!(
            p.vivid_max_chroma <= c.vivid.max_chroma,
            "{name}: vivid_max_chroma {}",
            p.vivid_max_chroma
        );
        // Technique.
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
            s.strokes.strength <= t.max_stroke_strength,
            "{name}: strokes.strength"
        );
        assert!(
            s.temperature.chroma <= t.max_temperature_chroma,
            "{name}: temperature.chroma"
        );
    });
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
            "{n}: actor warm_cool {}",
            actor.warm_cool
        );
        assert!(
            actor.shadow_tint <= c.max_actor_shadow_tint,
            "{n}: actor shadow_tint {}",
            actor.shadow_tint
        );
    }
}

// ------------------------------------------------------------------ palette math

/// The palette mapping a style uses for a category of the default target.
fn mapping(style: &Style, category: Category) -> Mapping<'_> {
    let target = Config::load(None, Some(&default_target()), None)
        .unwrap()
        .target;
    let tr = target.treatment(category);
    Mapping {
        palette: &style.palette,
        lift_scale: tr.floor_scale,
        shadow_scale: tr.shadow_tint,
    }
}

/// The LUT a style bakes for a category of the default target (what the GPU samples).
fn lut(style: &Style, category: Category) -> Lut3d {
    mapping(style, category).bake()
}

/// Each style's world LUT, baked once.
fn luts() -> &'static [(String, Style, Lut3d)] {
    static LUTS: std::sync::OnceLock<Vec<(String, Style, Lut3d)>> = std::sync::OnceLock::new();
    LUTS.get_or_init(|| {
        styles()
            .into_iter()
            .map(|p| {
                let style = Config::load(Some(&p), None, None).unwrap().style;
                let l = lut(&style, Category::World);
                (name(&p), style, l)
            })
            .collect()
    })
}

fn mapped_lch(lut: &Lut3d, rgb: [f32; 3]) -> [f32; 3] {
    let o = lut.sample(rgb);
    lch([o[0], o[1], o[2], 1.0])
}

#[test]
fn rule_palette_output_is_in_gamut_and_finite() {
    for (name, _, lut) in luts().iter() {
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
    each_style(|name, s| {
        let mut s = s.clone();
        s.palette.strength = 0.0;
        let lut = lut(&s, Category::World);
        let identity = Lut3d::identity(lut.size);
        for (a, b) in lut.data.iter().zip(&identity.data) {
            for k in 0..3 {
                assert!((a[k] - b[k]).abs() < 2e-3, "{name}: {a:?} vs {b:?}");
            }
        }
    });
}

#[test]
fn rule_value_order_preserved() {
    // Lightness ramps (with the chroma a shaded surface has) must map monotonically, so value
    // structure stays readable. Checked on the exact mapping; LUT fidelity is checked below.
    let tol = contract().tolerance.lightness;
    for (name, style, _) in luts().iter() {
        let m = mapping(style, Category::World);
        for h in (0..360).step_by(15) {
            for c in [0.0, 0.03, 0.08] {
                let mut prev = -1.0;
                for i in 0..=80 {
                    let l = i as f32 / 80.0;
                    let rgb = from_oklch(l, c * l.min(1.0 - l) * 4.0, h as f32);
                    let o = m.map(rgb);
                    let out = lch(o)[0];
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
    for (name, style, lut) in luts().iter() {
        let m = mapping(style, Category::World);
        for i in 0..4000u32 {
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

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    #[test]
    fn rule_no_dark_greens(l in 0.0f32..1.0, c in 0.0f32..0.3, h in 0.0f32..360.0) {
        let k = contract();
        let rgb = from_oklch(l, c, h);
        for (name, style, lut) in luts().iter() {
            let [lo, co, ho] = mapped_lch(lut, rgb);
            if co >= k.palette.green_min_chroma && in_hue_range(ho, k.palette.green_hue) {
                let floor = green_group_floor(style);
                prop_assert!(lo >= floor - k.tolerance.lightness,
                    "{}: {:?} -> L {} C {} h {} below green floor {}", name, rgb, lo, co, ho, floor);
            }
        }
    }

    #[test]
    fn rule_all_hue_pastel_floor(r in 0.0f32..1.0, g in 0.0f32..1.0, b in 0.0f32..1.0) {
        let k = contract();
        for (name, style, lut) in luts().iter() {
            let lo = mapped_lch(lut, [r, g, b])[0];
            let floor = style.palette.l_floor.max(k.palette.min_floor);
            prop_assert!(lo >= floor - k.tolerance.lightness,
                "{}: {:?} -> L {} below floor {}", name, [r, g, b], lo, floor);
        }
    }

    #[test]
    fn rule_chroma_is_capped(r in 0.0f32..1.0, g in 0.0f32..1.0, b in 0.0f32..1.0) {
        let k = contract();
        for (name, style, lut) in luts().iter() {
            let co = mapped_lch(lut, [r, g, b])[1];
            let cap = style.palette.chroma_cap.max(style.palette.vivid_max_chroma);
            prop_assert!(co <= cap + k.tolerance.chroma, "{}: {:?} -> C {} above {}", name, [r, g, b], co, cap);
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
