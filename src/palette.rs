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

pub(crate) fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
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

/// Weight of the earth warmth for a source hue and chroma: zero at and outside the band edges,
/// fading in over `feather` degrees inside the band, times a chroma fade-in.
pub fn warmth_weight(w: &crate::config::Warmth, h: f32, c: f32) -> f32 {
    let [from, to] = w.band;
    let span = (to - from).rem_euclid(360.0);
    let x = (h - from).rem_euclid(360.0);
    if x >= span || span <= 0.0 {
        return 0.0;
    }
    let f = w.feather.clamp(1e-3, span / 2.0);
    let edge = smoothstep(0.0, f, x) * (1.0 - smoothstep(span - f, span, x));
    edge * smoothstep(
        w.min_chroma[0],
        w.min_chroma[1].max(w.min_chroma[0] + 1e-4),
        c,
    )
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
    let knee = 0.8 * cap;
    if c <= knee {
        c
    } else {
        knee + (cap - knee) * ((c - knee) / (cap - knee)).tanh()
    }
}

/// The effective strength of the palette's moonlight cast (0 = off) for a category whose
/// treatment scales it by `scale` (the target's `cast`).
pub fn cast_strength(p: &Palette, scale: f32) -> f32 {
    (p.cast.strength * p.strength.min(1.0) * scale).clamp(0.0, 1.0)
}

/// Lightness `l` as the palette's moonlight cast dims it (scaled down above the palette floor;
/// `l` itself when the cast is off).
pub fn cast_exposure(p: &Palette, scale: f32, l: f32) -> f32 {
    let s = cast_strength(p, scale);
    let pivot = p.l_floor;
    if s <= 0.0 || l <= pivot {
        return l;
    }
    pivot + (l - pivot) * (1.0 - s * (1.0 - p.cast.exposure))
}

/// Weight of hue `h` inside the band `[from, to]`, feathered by `f` degrees outside it.
fn band_weight([from, to]: [f32; 2], f: f32, h: f32) -> f32 {
    let span = (to - from).rem_euclid(360.0);
    let x = (h - from + 180.0).rem_euclid(360.0) - 180.0; // position relative to `from`
    smoothstep(-f, 0.0, x) * (1.0 - smoothstep(span, span + f, x))
}

/// The shared moonlight cast (`palette.cast`, a mood's device) on a palette-mapped OKLCH color.
/// Applied per texel after the palette LUT (in `finish`; this is the same math for the CPU rule
/// tests), because its dark handling switches with the source's chroma and hue, which a baked
/// LUT cannot interpolate faithfully. `[l_src, c_src]` are the source texel's OKLab lightness
/// and chroma; `scale` is the category's `cast` treatment.
///
/// Exposure scales lightness down above the palette floor (value order and the dark floor stay);
/// chroma scales proportionally; one shared a/b vector toward the cast hue is added (muted in
/// the darks), so every color shifts the same way and hue differences survive. Darks: near-neutral
/// sources are capped at `dark_cap × L` (a muted midnight, never ink) and topped up to `dark_min`
/// along the cast; warm darks (hue in `warm_band`) and clearly colored darks keep at least
/// `dark_chroma` (deep colored shadows, never brown mud); colored sources keep the retention share
/// of their chroma.
pub fn apply_cast(
    p: &Palette,
    scale: f32,
    [l_src, c_src]: [f32; 2],
    [ll, cc, hh]: [f32; 3],
) -> [f32; 3] {
    let k = &p.cast;
    // The mood's dark handling applies whenever it has a cast (it replaces the palette's own
    // dark floors); exposure, desaturation and the cast vector scale with the category.
    if cast_strength(p, 1.0) <= 0.0 {
        return [ll, cc, hh];
    }
    let s = cast_strength(p, scale);
    let l2 = cast_exposure(p, scale, ll);
    let dark = 1.0 - smoothstep(p.dark_below - 0.03, p.dark_below + 0.05, l2);
    let mud_dark = 1.0 - smoothstep(p.dark_below + 0.02, p.dark_below + 0.1, l2);
    // Near-neutral by lightness-relative chroma, like the palette (a near-black navy is navy).
    let c_rel = c_src * (0.55 / (l_src.max(0.0) + 0.05)).max(1.0);
    let neutral = 1.0 - smoothstep(0.012, 0.03, c_rel);
    // Every color: its own chroma scaled, plus the one shared cast vector.
    let c2 = cc * (1.0 - s * (1.0 - k.chroma));
    let tint = s * k.tint * (l2 / k.tint_full_l.max(1e-3)).min(1.0);
    let (sh, ch) = hh.to_radians().sin_cos();
    let (sk, ck) = k.hue.to_radians().sin_cos();
    let (an, bn) = (c2 * ch + tint * ck, c2 * sh + tint * sk);
    // Clearly colored sources keep most of their color, and colored darks a deep colored shadow.
    let keep = smoothstep(0.03, 0.05, c_src) * (0.65 * c_src).min(0.052);
    let cn = an
        .hypot(bn)
        .max(keep)
        .max(k.dark_chroma * mud_dark * (1.0 - neutral));
    let len = an.hypot(bn).max(1e-9);
    let (an, bn) = (an / len * cn, bn / len * cn);
    // Near-neutral darks: a muted midnight along the cast, chroma at most dark_cap × L.
    let cm = (k.dark_cap * l2).max(k.dark_min);
    let w = neutral * dark;
    let (a, b) = (an + (cm * ck - an) * w, bn + (cm * sk - bn) * w);
    let mut c3 = cn + (cm - cn) * w;
    let h3 = if a.hypot(b) > 1e-9 {
        b.atan2(a).to_degrees().rem_euclid(360.0)
    } else {
        k.hue
    };
    // Warm darks (earth, olive) never dull: at least dark_chroma (no mud); every dark at least
    // dark_min.
    let warm = band_weight(k.warm_band, 5.0, h3);
    c3 = c3
        .max(k.dark_chroma * mud_dark * warm)
        .max(k.dark_min * dark);
    [l2, c3, h3]
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
    c_min: f32,
}

/// The mapping for one palette config, ready to evaluate or bake.
pub struct Mapping<'a> {
    pub palette: &'a Palette,
    /// Scales the lightness lift (target `floor_scale`, e.g. < 1 for dark areas).
    pub lift_scale: f32,
    /// Scales the shadow tint (target `shadow_tint`).
    pub shadow_scale: f32,
    /// Scales hue shifts, pulls and harmonization (target `hue`).
    pub hue_scale: f32,
    /// Scales the earth warmth (target `warmth`).
    pub warmth_scale: f32,
}

impl<'a> Mapping<'a> {
    /// The mapping for a palette under a category's treatment.
    pub fn new(palette: &'a Palette, tr: &crate::config::Treatment) -> Self {
        Self {
            warmth_scale: tr.warmth,
            palette,
            lift_scale: tr.floor_scale,
            shadow_scale: tr.shadow_tint,
            hue_scale: tr.hue,
        }
    }
}

impl Mapping<'_> {
    /// A group's floor; green groups are held at `light_green_floor` when dark greens are
    /// denied.
    fn group_floor(&self, g: &HueGroup) -> f32 {
        let p = self.palette;
        let [from, to] = g.hue_range;
        let mid = from + (to - from).rem_euclid(360.0) / 2.0;
        let [gf, gt] = p.green_hue;
        let green = (mid - gf).rem_euclid(360.0) <= (gt - gf).rem_euclid(360.0);
        if !p.dark_greens && green {
            g.l_floor.max(p.light_green_floor)
        } else {
            g.l_floor
        }
    }

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
            b.floor += w * self.group_floor(g);
            b.c_scale += w * g.c_scale;
            b.c_cap += w * g.c_cap.unwrap_or(1.0);
            b.c_min += w * g.c_min;
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
        // Lightness-relative chroma for "is this neutral?" decisions: very dark texels have tiny
        // absolute chroma even when clearly hued (a near-black navy is still navy), so chroma is
        // judged as if the color were at mid lightness. Equals `c` from L 0.5 up.
        let c_rel = c * (0.55 / (l + 0.05)).max(1.0);

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
        let hue_scale = self.hue_scale.max(0.0);
        let mut h2 = h + (g.shift * s * hue_scale).clamp(-120.0, 120.0);
        let pull_scale = hue_scale * if s <= 1.0 { s } else { 1.0 + 2.0 * (s - 1.0) };
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
        let nw = 1.0 - smoothstep(p.neutral_c * 0.5, p.neutral_c * 1.5, c_rel);
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
        let harmonize = (p.harmonize * s * hue_scale).min(1.0);
        if harmonize > 0.0 && !p.pigments.is_empty() {
            h2 += harmonize * pigment_pull(&p.pigments, p.pigment_spread, h2);
        }

        // Chroma: scale, soft cap, and a per-group chroma floor for clearly colored sources.
        // Extrapolating past strength 1 lightens; it never grays (caps and scales stop at 1).
        let global_cap = lerp(1.0, p.chroma_cap, s.min(1.0));
        let mut cap = g.c_cap.min(p.chroma_cap);
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
        let c1 = c * scale.powf(s.min(1.0));
        let c1 = c1 * (1.0 + p.chroma_lift * s.min(1.0) * (l3 - l).max(0.0));
        let mut c2 = lerp(c1, soft_cap(c1, cap), s.min(1.0));
        // "Pastel is not gray": colored sources keep at least the group's reference chroma.
        // The floor never more than doubles a source's chroma, so a faint cast (gray curtain
        // folds with a hint of blue) isn't amplified into colored stripes.
        let colored = smoothstep(p.neutral_c, p.neutral_c * 2.5, c) * s.min(1.0);
        c2 = c2.max((g.c_min * colored).min(2.0 * c));
        // Targeted earth warmth (weight 0 exactly outside the source band).
        let wm = &p.warmth;
        let ww = warmth_weight(wm, h, c)
            * (wm.strength * self.warmth_scale * s.min(1.0)).clamp(0.0, 0.9);
        let mut l3 = l3;
        if ww > 0.0 {
            h2 += ww * hue_diff(h2, wm.hue);
            if c2 < wm.chroma {
                c2 += (wm.chroma - c2) * (wm.chroma_boost * ww).min(1.0);
            }
            l3 = (l3 + wm.lift * ww).min(ceiling);
        }
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
        let colored_src = smoothstep(0.02, 0.04, c_rel);
        let st_w = (st.amount * s).min(1.0)
            * self.shadow_scale
            * lerp(1.0, st.colored.clamp(0.0, 1.0), colored_src)
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
        let mut cc = cc.min(vivid_cap);
        let mut hh = hh;
        // Darks are colored, never black or mud: below `dark_below`, chroma reaches at least
        // `dark_chroma` — along the source's own hue when it is clearly colored, along the shadow
        // tint's hue when it is (near-)neutral, rotating between them along the shortest arc so
        // no mix ever passes through gray.
        if p.dark_chroma > 0.0 {
            // Full strength until just below `dark_below`, fading out just above it.
            let dark = 1.0 - smoothstep(p.dark_below - 0.03, p.dark_below + 0.05, ll);
            let want = p.dark_chroma * dark * s.min(1.0);
            if cc < want {
                // Keep the source's own hue when it is clearly hued; near-neutrals (a faint
                // cast, e.g. a white curtain's fold shadows) take the shadow tint's hue, so the
                // lift never turns a faint cast into colored stripes. (Rotating between the two
                // would pass through unrelated hues: halfway between umber and blue is green.)
                if c_rel < 0.03 {
                    hh = p.shadow_tint.hue;
                }
                cc = want;
            }
            // Optional cool bias (off by default: darks keep their source hue).
            // A hue rotation toward the cool hue, chroma kept (adding a vector would cancel warm
            // chroma into gray mud).
            if p.dark_cool_bias > 0.0 {
                // Hues nearly opposite the cool target have no well-defined "toward" direction
                // (the shortest arc flips); they keep their hue instead of splitting two ways.
                let d = hue_diff(hh, p.dark_cool_hue);
                let t = (p.dark_cool_bias * dark * s.min(1.0)).clamp(0.0, 1.0)
                    * (1.0 - smoothstep(110.0, 160.0, d.abs()));
                hh += t * d;
            }
        }
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

    fn default_palette() -> Palette {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("styles/watercolor.toml");
        Style::load(&path).unwrap().palette
    }

    fn mapping(p: &Palette) -> Mapping<'_> {
        Mapping {
            palette: p,
            lift_scale: 1.0,
            shadow_scale: 1.0,
            hue_scale: 1.0,
            warmth_scale: 1.0,
        }
    }

    fn lch(rgb: [f32; 4]) -> [f32; 3] {
        color::oklab_to_oklch(color::srgb_to_oklab([rgb[0], rgb[1], rgb[2]]))
    }

    #[test]
    fn zero_strength_is_identity() {
        let p = Palette {
            strength: 0.0,
            ..default_palette()
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
        let mut p = default_palette();
        p.dark_greens = false;
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
            assert!(l >= p.light_green_floor - 1e-3, "{rgb:?} -> L {l}");
            assert!(l > prev, "order lost at {rgb:?}");
            assert!((100.0..175.0).contains(&h), "{rgb:?} -> hue {h}");
            prev = l;
        }
    }

    #[test]
    fn crushed_darks_of_every_hue_are_lifted() {
        let p = default_palette();
        let m = mapping(&p);
        for rgb in [
            [0.3, 0.02, 0.02],
            [0.02, 0.02, 0.3],
            [0.2, 0.02, 0.25],
            [0.25, 0.12, 0.04],
        ] {
            let [l, c, _] = lch(m.map(rgb));
            assert!(l >= p.l_floor - 1e-3, "{rgb:?} -> L {l}");
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
        let p = default_palette();
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
    fn denying_dark_greens_lifts_only_green_groups() {
        let mut p = default_palette();
        for g in &mut p.groups {
            g.l_floor = 0.3;
        }
        p.l_floor = 0.3;
        p.light_green_floor = 0.8;
        let dark_green = [0.05, 0.2, 0.03];
        let dark_red = [0.25, 0.03, 0.03];
        let allowed = lch(mapping(&p).map(dark_green))[0];
        let red_allowed = lch(mapping(&p).map(dark_red))[0];
        p.dark_greens = false;
        assert!(lch(mapping(&p).map(dark_green))[0] >= 0.8 - 1e-3);
        assert!(allowed < 0.8);
        assert!((lch(mapping(&p).map(dark_red))[0] - red_allowed).abs() < 1e-4);
    }

    #[test]
    fn vivid_colors_keep_more_chroma() {
        let chroma = |p: &Palette| lch(mapping(p).map([0.7, 0.1, 0.1]))[1];
        // A self-contained palette: one soft group that caps chroma well inside the gamut.
        let plain = Palette {
            enabled: true,
            vivid: 0.0,
            vivid_max_chroma: 0.18,
            groups: vec![crate::config::HueGroup {
                hue_range: [0.0, 360.0],
                c_scale: 0.5,
                c_cap: Some(0.06),
                ..Default::default()
            }],
            ..Palette::default()
        };
        let vivid = Palette {
            vivid: 1.0,
            vivid_hues: Vec::new(),
            ..plain.clone()
        };
        let other_hues = Palette {
            vivid_hues: vec![[100.0, 200.0]],
            ..vivid.clone()
        };
        assert!(
            chroma(&vivid) > chroma(&plain) + 0.01,
            "vivid {} vs plain {}",
            chroma(&vivid),
            chroma(&plain)
        );
        assert!((chroma(&other_hues) - chroma(&plain)).abs() < 1e-4);
    }

    #[test]
    fn extrapolation_stays_in_gamut_and_gets_lighter() {
        let base = default_palette();
        let more = Palette {
            strength: 2.0,
            ..default_palette()
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
