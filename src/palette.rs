//! The parameterized OKLCH palette mapping (`[palette]` in the style), baked into a [`Lut3d`].
//!
//! Per input color, in OKLCH (input `L, C, h`):
//!
//! 1. **Tone curve** `L1 = l_curve(L)` (monotone, piecewise-linear).
//! 2. **Hue groups**, blended by feathered weights over their hue ranges (hard group boundaries
//!    posterize): `h += hue_shift`, then `h += hue_pull · (hue_center − h)`; `L2 = L1 + l_offset`,
//!    then the group's range below a knee is *compressed* (not clamped) up onto `l_floor`;
//!    `C' = softcap(C · c_scale · chroma_scale, c_cap)`.
//! 3. **Pigment harmonization** toward a limited hue set (continuous between pigments).
//! 4. **Vivid colors**: strongly chromatic sources may exceed the caps (chroma only).
//! 5. **Neutral path** for `C` below `neutral_c` (feathered): tone curve only, a/b lerped toward
//!    `neutral_tint`.
//! 6. **Shadow tint** on originally dark texels.
//! 7. Clamp L to `[l_floor, l_ceiling]`, cap C at `chroma_cap`, gamut-map by reducing chroma.
//!
//! `strength` s moves every parameter from the identity (0) to the configured look (1) and
//! extrapolates beyond it (lighter, softer, more harmonized) without leaving the gamut:
//! the tone curve is composed with itself (monotone), floors move in log-headroom space toward
//! `L_MAX`, shifts scale by s, pulls by
//! `1 + 2(s − 1)` (≤ 0.8), chroma scales as `x^s` (boosts stop at s = 1), caps tighten by `1/√s`,
//! tints by s.
//!
//! Each LUT entry also stores the lightness floor that applied to it, so the shader can keep
//! watercolor darkening from undercutting the palette.

use crate::color::{self, hue_diff};
use crate::config::{HueGroup, Palette};
use crate::lut::Lut3d;

/// Lightness asymptote for extrapolation.
const L_MAX: f32 = 0.985;

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Lightness target `to` (configured for input `from`) at strength `s`, extrapolated in
/// log-headroom space so it approaches but never passes `L_MAX`.
fn extrapolate_l(from: f32, to: f32, s: f32) -> f32 {
    if s <= 1.0 {
        return lerp(from, to, s);
    }
    let a = (L_MAX - from.min(L_MAX - 1e-3)).ln();
    let b = (L_MAX - to.min(L_MAX - 1e-3)).ln();
    L_MAX - (a + s * (b - a)).exp()
}

/// Piecewise-linear curve lookup (points sorted by input; clamped at the ends).
fn curve(points: &[[f32; 2]], x: f32) -> f32 {
    match points {
        [] => x,
        [p] => p[1],
        _ => {
            if x <= points[0][0] {
                return points[0][1];
            }
            for w in points.windows(2) {
                let ([x0, y0], [x1, y1]) = (w[0], w[1]);
                if x <= x1 {
                    let t = if x1 > x0 { (x - x0) / (x1 - x0) } else { 1.0 };
                    return lerp(y0, y1, t);
                }
            }
            points[points.len() - 1][1]
        }
    }
}

/// The tone curve applied "`s` times": `s` in 0..1 blends identity → curve, 1..2 blends curve →
/// curve∘curve, and so on. Blends and compositions of monotone curves stay monotone, so
/// extrapolating never reverses value order.
fn curve_power(points: &[[f32; 2]], x: f32, s: f32) -> f32 {
    let s = s.clamp(0.0, 8.0);
    let whole = s.floor() as u32;
    let mut v = x;
    for _ in 0..whole {
        v = curve(points, v);
    }
    let frac = s - whole as f32;
    if frac > 0.0 {
        v = lerp(v, curve(points, v), frac);
    }
    v
}

/// Weight of a hue inside a (possibly wrapping) range, with a smooth `feather`-wide ramp
/// centred on each end.
fn group_weight(range: [f32; 2], feather: f32, h: f32) -> f32 {
    let [from, to] = range;
    let span = (to - from).rem_euclid(360.0);
    let span = if span == 0.0 { 360.0 } else { span };
    if span >= 360.0 {
        return 1.0;
    }
    let f = feather.max(1e-3);
    // Position of h relative to `from`, centred on the range middle so wrapping is unambiguous.
    let mid = from + span / 2.0;
    let d = hue_diff(mid, h); // -180..180, 0 at the middle
    let x = d + span / 2.0; // 0 at `from`, span at `to`
    smoothstep(-f / 2.0, f / 2.0, x) * (1.0 - smoothstep(span - f / 2.0, span + f / 2.0, x))
}

/// Monotone floor: compresses `[lo, knee]` up into `[floor, knee]` (values above the knee are
/// untouched), so darks are lifted without losing their order. `knee` sits at `knee_frac` of the
/// way from `floor` to `ceiling`.
fn compress(l: f32, lo: f32, floor: f32, ceiling: f32, knee_frac: f32) -> f32 {
    let knee = floor + (ceiling - floor) * knee_frac.clamp(0.0, 1.0);
    if floor > lo && l < knee && knee > lo {
        floor + (knee - floor) * ((l - lo) / (knee - lo)).max(0.0)
    } else {
        l
    }
}

/// Signed pull toward the pigment set: Gaussian-weighted average of the differences to each
/// pigment hue, so it is continuous everywhere.
fn pigment_pull(pigments: &[f32], spread: f32, hue: f32) -> f32 {
    let spread = spread.max(1.0);
    let (mut sum, mut wsum) = (0.0, 0.0);
    for &p in pigments {
        let d = hue_diff(hue, p);
        let w = (-(d / spread).powi(2)).exp();
        sum += w * d;
        wsum += w;
    }
    if wsum > 1e-6 { sum / wsum } else { 0.0 }
}

/// Soft chroma cap: linear below 60% of the cap, asymptotic to it above.
fn soft_cap(c: f32, cap: f32) -> f32 {
    if cap <= 0.0 {
        return 0.0;
    }
    let knee = 0.6 * cap;
    if c <= knee {
        c
    } else {
        knee + (cap - knee) * ((c - knee) / (cap - knee)).tanh()
    }
}

/// Group parameters blended by weight (`None` if no group covers the hue).
#[derive(Default)]
struct Blend {
    weight: f32,
    shift: f32,
    offset: f32,
    floor: f32,
    c_scale: f32,
    c_cap: f32,
}

/// The mapping for one palette config, ready to evaluate or bake.
pub struct Mapping<'a> {
    pub palette: &'a Palette,
    /// Scales the lightness lift (target `floor_scale`, e.g. < 1 for dark areas).
    pub lift_scale: f32,
    /// Scales the shadow tint (target `shadow_tint`).
    pub shadow_scale: f32,
}

impl Mapping<'_> {
    fn groups(&self, h: f32) -> (Blend, Vec<(f32, &HueGroup)>) {
        let p = self.palette;
        let weighted: Vec<(f32, &HueGroup)> = p
            .groups
            .iter()
            .map(|g| (group_weight(g.hue_range, p.hue_feather, h), g))
            .filter(|(w, _)| *w > 0.0)
            .collect();
        let total: f32 = weighted.iter().map(|(w, _)| w).sum();
        let mut b = Blend::default();
        if total <= 0.0 {
            b.c_scale = 1.0;
            b.c_cap = f32::INFINITY;
            return (b, weighted);
        }
        let norm = total.max(1.0);
        for (w, g) in &weighted {
            let w = w / norm;
            b.weight += w;
            b.shift += w * g.hue_shift;
            b.offset += w * g.l_offset;
            b.floor += w * g.l_floor;
            b.c_scale += w * g.c_scale;
            b.c_cap += w * g.c_cap.unwrap_or(1.0);
        }
        // Uncovered share behaves like an identity group.
        let rest = 1.0 - b.weight;
        b.c_scale += rest;
        b.c_cap += rest;
        (b, weighted)
    }

    /// Maps one gamma-sRGB color; returns `[r, g, b, floor]` (the OKLab lightness floor that
    /// applied).
    pub fn map(&self, rgb: [f32; 3]) -> [f32; 4] {
        let p = self.palette;
        let s = p.strength.max(0.0);
        let [l, c, h] = color::oklab_to_oklch(color::srgb_to_oklab(rgb));
        let l = l.clamp(0.0, 1.0);

        let ceiling = if s <= 1.0 {
            lerp(1.0, p.l_ceiling, s)
        } else {
            p.l_ceiling
        };
        let global_floor = extrapolate_l(0.0, p.l_floor, s).min(ceiling);
        // 1. Tone curve (each point extrapolated from the identity).
        let l1 = curve_power(&p.l_curve, l, s);
        let l1_at_zero = curve_power(&p.l_curve, 0.0, s);

        // 2. Hue groups.
        let (g, weighted) = self.groups(h);
        let mut h2 = h + (g.shift * s).clamp(-120.0, 120.0);
        let pull_scale = if s <= 1.0 { s } else { 1.0 + 2.0 * (s - 1.0) };
        let mut pull = 0.0;
        let norm: f32 = weighted.iter().map(|(w, _)| w).sum::<f32>().max(1.0);
        for (w, grp) in &weighted {
            if let Some(center) = grp.hue_center {
                let amount = (grp.hue_pull * pull_scale).min(0.8);
                pull += w / norm * amount * hue_diff(h2, center);
            }
        }
        h2 += pull;
        // Lightness: one monotone mapping shared by the chromatic and neutral paths; the group's
        // offset and floor fade in with chroma, so near-grays don't jump between two curves.
        let nw = 1.0 - smoothstep(p.neutral_c * 0.5, p.neutral_c * 1.5, c);
        let cw = 1.0 - nw;
        let offset = g.offset * s * cw;
        let l2 = (l1 + offset).min(ceiling);
        let lo = l1_at_zero + offset;
        let neutral_floor = global_floor.min(ceiling - 0.05);
        let group_floor = extrapolate_l(0.0, g.floor, s)
            .max(global_floor)
            .min(ceiling - 0.05);
        let floor = lerp(neutral_floor, group_floor, cw);
        let l3 = compress(l2, lo, floor, ceiling, p.floor_knee);

        // 3. Pigment harmonization.
        let harmonize = (p.harmonize * s).min(1.0);
        if harmonize > 0.0 && !p.pigments.is_empty() {
            h2 += harmonize * pigment_pull(&p.pigments, p.pigment_spread, h2);
        }

        // Chroma with soft caps (tighter when extrapolating).
        let tighten = s.max(1.0).sqrt();
        let global_cap = if s <= 1.0 {
            lerp(1.0, p.chroma_cap, s)
        } else {
            p.chroma_cap / tighten
        };
        let mut cap = g.c_cap.min(p.chroma_cap) / tighten;
        let mut scale = (g.c_scale * p.chroma_scale).max(0.0);
        // 4. Vivid colors keep more chroma (never more lightness).
        let mut vivid_cap = global_cap;
        if p.vivid > 0.0 {
            let in_range = p.vivid_hues.is_empty()
                || p.vivid_hues.iter().any(|&[from, to]| {
                    (h - from).rem_euclid(360.0) <= (to - from).rem_euclid(360.0)
                });
            if in_range {
                let v =
                    p.vivid.min(1.0) * smoothstep(p.vivid_threshold, p.vivid_threshold + 0.06, c);
                cap = lerp(cap, p.vivid_max_chroma.max(cap), v);
                scale = lerp(scale, scale.max(1.0), v);
                vivid_cap = lerp(global_cap, p.vivid_max_chroma.max(global_cap), v);
            }
        }
        // Chroma boosts are never extrapolated past the configured look (pastel means softer).
        let c1 = c * if scale > 1.0 {
            scale.powf(s.min(1.0))
        } else {
            scale.powf(s)
        };
        let c2 = lerp(c1, soft_cap(c1, cap), s.min(1.0));
        let chromatic = color::oklch_to_oklab([l3, c2, h2]);

        // 5. Neutral path.
        let nt = &p.neutral_tint;
        let n_amount = (nt.amount * s).min(1.0);
        let n_vec = color::oklch_to_oklab([0.0, nt.chroma, nt.hue]);
        let neutral = [
            l3,
            lerp(c * h.to_radians().cos(), n_vec[1], n_amount),
            lerp(c * h.to_radians().sin(), n_vec[2], n_amount),
        ];
        let mut lab = [0, 1, 2].map(|k| lerp(chromatic[k], neutral[k], nw));
        // Lowest lightness this hue can map to (the guard for later darkening).
        let floor = floor.max(lo);

        // 6. Shadow tint on originally dark texels.
        let st = &p.shadow_tint;
        let st_w = (st.amount * s).min(1.0)
            * self.shadow_scale
            * (1.0 - smoothstep(0.0, st.below_input_l.max(1e-3), l));
        if st_w > 0.0 {
            let v = color::oklch_to_oklab([0.0, st.chroma, st.hue]);
            lab[1] = lerp(lab[1], v[1], st_w);
            lab[2] = lerp(lab[2], v[2], st_w);
        }

        // Scale the lightness lift for this category.
        lab[0] = l + (lab[0] - l) * self.lift_scale;

        // 7. Clamps and gamut.
        let [ll, cc, hh] = color::oklab_to_oklch(lab);
        let lowest = global_floor
            .min(l + (global_floor - l) * self.lift_scale)
            .min(ceiling);
        let ll = ll.clamp(lowest, ceiling);
        let cc = cc.min(vivid_cap);
        let [r, g, b] = color::oklch_to_srgb_gamut([ll, cc, hh]);
        let floor = (l + (floor - l) * self.lift_scale).min(ll);
        [r, g, b, floor]
    }

    pub fn bake(&self) -> Lut3d {
        Lut3d::bake(self.palette.lut_size.clamp(2, 129) as usize, |rgb| {
            self.map(rgb)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Style;

    fn skyward() -> Palette {
        let style: Style =
            toml::from_str(include_str!("../styles/skyward-watercolor.toml")).unwrap();
        style.palette
    }

    fn mapping(p: &Palette) -> Mapping<'_> {
        Mapping {
            palette: p,
            lift_scale: 1.0,
            shadow_scale: 1.0,
        }
    }

    fn lch(rgb: [f32; 4]) -> [f32; 3] {
        color::oklab_to_oklch(color::srgb_to_oklab([rgb[0], rgb[1], rgb[2]]))
    }

    #[test]
    fn zero_strength_is_identity() {
        let p = Palette {
            strength: 0.0,
            ..skyward()
        };
        for rgb in [
            [0.1, 0.5, 0.2],
            [0.9, 0.3, 0.1],
            [0.5, 0.5, 0.5],
            [0.0, 0.0, 0.0],
        ] {
            let out = mapping(&p).map(rgb);
            for k in 0..3 {
                assert!((out[k] - rgb[k]).abs() < 2e-3, "{rgb:?} -> {out:?}");
            }
        }
    }

    #[test]
    fn no_dark_greens_and_value_order_is_kept() {
        let p = skyward();
        let m = mapping(&p);
        let greens = [
            [0.02, 0.08, 0.01],
            [0.05, 0.2, 0.03],
            [0.1, 0.3, 0.05],
            [0.2, 0.45, 0.1],
            [0.5, 0.8, 0.3],
        ];
        let mut prev = 0.0;
        for rgb in greens {
            let [l, _, h] = lch(m.map(rgb));
            assert!(l >= 0.72, "{rgb:?} -> L {l}");
            assert!(l > prev, "order lost at {rgb:?}");
            assert!((100.0..175.0).contains(&h), "{rgb:?} -> hue {h}");
            prev = l;
        }
    }

    #[test]
    fn every_hue_is_pastel() {
        let p = skyward();
        let m = mapping(&p);
        for rgb in [
            [0.3, 0.02, 0.02],
            [0.02, 0.02, 0.3],
            [0.2, 0.02, 0.25],
            [0.25, 0.12, 0.04],
        ] {
            let [l, c, _] = lch(m.map(rgb));
            assert!(l >= 0.5, "{rgb:?} -> L {l}");
            assert!(c <= 0.16, "{rgb:?} -> C {c}");
        }
    }

    #[test]
    fn curve_is_piecewise_linear() {
        let pts = [[0.0, 0.5], [0.5, 0.7], [1.0, 1.0]];
        assert_eq!(curve(&pts, 0.25), 0.6);
        assert_eq!(curve(&pts, -1.0), 0.5);
        assert_eq!(curve(&pts, 2.0), 1.0);
        assert_eq!(curve(&[], 0.3), 0.3);
    }

    #[test]
    fn group_weights_feather_and_wrap() {
        assert_eq!(group_weight([345.0, 40.0], 10.0, 10.0), 1.0);
        assert_eq!(group_weight([345.0, 40.0], 10.0, 100.0), 0.0);
        assert!((group_weight([345.0, 40.0], 10.0, 345.0) - 0.5).abs() < 1e-4);
        // Adjacent groups sum to one across the boundary.
        for h in [36.0, 38.0, 40.0, 42.0, 44.0] {
            let sum = group_weight([345.0, 40.0], 10.0, h) + group_weight([40.0, 85.0], 10.0, h);
            assert!((sum - 1.0).abs() < 1e-4, "{h}: {sum}");
        }
    }

    #[test]
    fn mapping_is_continuous_across_hue() {
        let p = skyward();
        let m = mapping(&p);
        let mut prev = m.map(color::oklch_to_srgb_gamut([0.5, 0.08, 0.0]));
        for i in 1..=720 {
            let rgb = color::oklch_to_srgb_gamut([0.5, 0.08, i as f32 * 0.5]);
            let out = m.map(rgb);
            let d: f32 = (0..3).map(|k| (out[k] - prev[k]).abs()).sum();
            assert!(d < 0.06, "jump at hue {}: {d}", i as f32 * 0.5);
            prev = out;
        }
    }

    #[test]
    fn pigment_pull_is_continuous_and_attracts() {
        let pigments = [60.0, 120.0];
        assert!(pigment_pull(&pigments, 25.0, 70.0) < 0.0);
        assert!(pigment_pull(&pigments, 25.0, 110.0) > 0.0);
        assert!(pigment_pull(&pigments, 25.0, 90.0).abs() < 1e-3);
    }

    #[test]
    fn vivid_colors_keep_more_chroma() {
        let chroma = |p: &Palette| lch(mapping(p).map([0.9, 0.1, 0.1]))[1];
        let plain = Palette {
            vivid: 0.0,
            ..skyward()
        };
        let vivid = Palette {
            vivid: 1.0,
            vivid_hues: Vec::new(),
            ..skyward()
        };
        let other_hues = Palette {
            vivid_hues: vec![[100.0, 200.0]],
            ..vivid.clone()
        };
        assert!(chroma(&vivid) > chroma(&plain) + 0.01);
        assert!((chroma(&other_hues) - chroma(&plain)).abs() < 1e-4);
    }

    #[test]
    fn extrapolation_stays_in_gamut_and_gets_lighter() {
        let base = skyward();
        let more = Palette {
            strength: 2.0,
            ..skyward()
        };
        let lut = mapping(&more).bake();
        assert!(
            lut.data
                .iter()
                .all(|v| v.iter().all(|c| c.is_finite() && (0.0..=1.0).contains(c)))
        );
        let dark = [0.2, 0.15, 0.1];
        assert!(lch(mapping(&more).map(dark))[0] > lch(mapping(&base).map(dark))[0]);
    }
}
