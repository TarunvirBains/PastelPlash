//! `[palette]`: the OKLCH palette mapping and its hue groups, tints and earth warmth.

use serde::Deserialize;

use anyhow::Result;

use crate::config::{check, non_negative, unit};

/// A hue group of the palette: a hue range (feathered at its ends) with its own adjustments.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HueGroup {
    pub name: String,
    /// OKLCH hue range `[from, to]` in degrees; may wrap (e.g. `[345, 40]`).
    pub hue_range: [f32; 2],
    /// Hue rotation in degrees, applied first.
    pub hue_shift: f32,
    /// Harmonization target hue; `None` = no pull.
    pub hue_center: Option<f32>,
    /// Fraction of the way from the shifted hue to `hue_center`.
    pub hue_pull: f32,
    /// Added to the tone-curve lightness.
    pub l_offset: f32,
    /// Lightness floor for the group. The group's value range is compressed (not clamped) into
    /// `[l_floor, l_ceiling]`, so relative value order survives.
    pub l_floor: f32,
    pub c_scale: f32,
    pub c_cap: Option<f32>,
    /// Chroma floor for clearly colored sources of this group ("pastel is not gray"),
    /// typically the reference palette's median chroma for the group.
    pub c_min: f32,
    /// Informational (from the reference analysis); not used by the mapping.
    pub l_target_median: Option<f32>,
}

impl Default for HueGroup {
    fn default() -> Self {
        Self {
            name: String::new(),
            hue_range: [0.0, 360.0],
            hue_shift: 0.0,
            hue_center: None,
            hue_pull: 0.0,
            l_offset: 0.0,
            l_floor: 0.0,
            c_scale: 1.0,
            c_cap: None,
            c_min: 0.0,
            l_target_median: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Tint {
    pub hue: f32,
    pub chroma: f32,
    /// 0..1 lerp of a/b toward the tint vector.
    pub amount: f32,
    /// Shadow tints only: weight falls from 1 at input L 0 to 0 at this input L.
    pub below_input_l: f32,
    /// Shadow tints only: share of the tint applied to clearly colored sources (0 = neutral
    /// darks only, so dark browns stay brown and dark greens stay green).
    pub colored: f32,
}

impl Default for Tint {
    fn default() -> Self {
        Self {
            hue: 0.0,
            chroma: 0.0,
            amount: 0.0,
            below_input_l: 0.25,
            colored: 1.0,
        }
    }
}

/// Targeted earth warmth: sources whose hue lies in `band` (earth, olive, khaki) are pulled
/// toward a warm golden-tan hue, weighted by their chroma (near-neutral stone barely moves).
/// Hues outside the band are untouched exactly; hues inside never leave it (the target lies
/// inside the band and the pull is a fraction < 1 of the way there).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Warmth {
    /// 0..0.9: fraction of the way to `hue` at full weight; 0 disables.
    pub strength: f32,
    /// Source OKLCH hue band `[from, to]`, degrees.
    pub band: [f32; 2],
    /// Width in degrees of the fade-in at each band edge (zero weight at the edges).
    pub feather: f32,
    /// Target hue (SS lit earth, golden tan).
    pub hue: f32,
    /// Target chroma (SS lit earth median); colors below it gain up to `chroma_boost` of the gap.
    pub chroma: f32,
    pub chroma_boost: f32,
    /// Source chroma range over which the weight fades in (near-neutrals barely move).
    pub min_chroma: [f32; 2],
    /// Lightness lift at full weight (OKLab L); usually 0.
    pub lift: f32,
}

impl Default for Warmth {
    fn default() -> Self {
        Self {
            strength: 0.0,
            band: [50.0, 115.0],
            feather: 10.0,
            hue: 68.0,
            chroma: 0.079,
            chroma_boost: 0.0,
            min_chroma: [0.02, 0.06],
            lift: 0.0,
        }
    }
}

/// A shared moonlight cast (a mood's device, off by default): every color shifts the same way —
/// darker, proportionally less saturated, and nudged by one shared a/b vector toward `hue` — so
/// hue *differences* between materials survive (moss stays greener than wood). Near-neutral
/// darks take only a muted cast, their chroma capped in proportion to their lightness.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Cast {
    /// 0..1; 0 disables.
    pub strength: f32,
    /// OKLCH hue of the cast (e.g. 275 indigo, 250 midnight blue, 305 midnight purple).
    pub hue: f32,
    /// OKLab chroma of the shared cast vector at full strength (muted below `tint_full_l`).
    pub tint: f32,
    /// Output lightness at and above which the cast vector is at full size.
    pub tint_full_l: f32,
    /// Exposure: lightness above the palette floor is scaled by this at full strength (< 1
    /// dims the area; value order is kept).
    pub exposure: f32,
    /// Chroma multiplier at full strength (proportionally less saturated).
    pub chroma: f32,
    /// Near-neutral sources: output chroma at most `dark_cap` × output lightness.
    pub dark_cap: f32,
    /// Darks (below `palette.dark_below`) keep at least this chroma, topped up along the cast
    /// hue (a muted midnight, not a dull gray or mud).
    pub dark_min: f32,
    /// Clearly colored darks keep at least this chroma along their (cast-shifted) hue: colored
    /// shadows, never dull brown mud.
    pub dark_chroma: f32,
    /// OKLCH hue band of warm (earth, olive) darks, which keep `dark_chroma` too, even when
    /// near-neutral: a dull warm dark reads as mud.
    pub warm_band: [f32; 2],
}

impl Default for Cast {
    fn default() -> Self {
        Self {
            strength: 0.0,
            hue: 275.0,
            tint: 0.02,
            tint_full_l: 0.45,
            exposure: 1.0,
            chroma: 1.0,
            dark_cap: 0.12,
            dark_min: 0.018,
            dark_chroma: 0.038,
            warm_band: [35.0, 105.0],
        }
    }
}

/// OKLCH palette mapping, baked into a 3D LUT (see `src/palette.rs` for the exact order).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Palette {
    /// Generate the palette LUT (ignored when the style sets `lut`).
    pub enabled: bool,
    /// 0 = identity, 1 = the configured look, > 1 extrapolates further toward pastel.
    pub strength: f32,
    pub lut_size: u32,
    /// Monotone piecewise-linear tone curve `[[in, out], …]` on OKLCH L; empty = identity.
    pub l_curve: Vec<[f32; 2]>,
    /// Global lightness clamp.
    pub l_floor: f32,
    pub l_ceiling: f32,
    /// Where group floors stop compressing, as a fraction of `[l_floor, l_ceiling]` of the
    /// group: values below map monotonically into `[group floor, knee]`, values above are kept.
    pub floor_knee: f32,
    pub chroma_scale: f32,
    pub chroma_cap: f32,
    /// Extra chroma per unit of lightness lift (`C *= 1 + chroma_lift · ΔL`), so lifted colors
    /// stay clean instead of chalky.
    pub chroma_lift: f32,
    /// Chroma floor for darks that have any hue (no brown or olive mud): deep colored shadows
    /// along the source's own hue. Fades in below `dark_below` (output OKLCH L).
    pub dark_chroma: f32,
    pub dark_below: f32,
    /// Optional cool bias on lifted darks: 0..1 of the way their hue rotates toward
    /// `dark_cool_hue`, chroma kept (0 = off; darks keep their source hue).
    pub dark_cool_bias: f32,
    pub dark_cool_hue: f32,
    /// Targeted earth warmth (scaled per category by the target's `warmth`).
    pub warmth: Warmth,
    /// Shared moonlight cast (moods such as nocturne).
    pub cast: Cast,
    /// Chroma below which a color takes the neutral path (feathered over ±50%).
    pub neutral_c: f32,
    pub neutral_tint: Tint,
    /// Tint for originally dark texels (scaled per category by the target's `shadow_tint`).
    pub shadow_tint: Tint,
    /// Width in degrees of the smooth blend at hue-group boundaries.
    pub hue_feather: f32,
    pub groups: Vec<HueGroup>,
    /// Limited pigment set (OKLCH hues) that hues are pulled toward, after the groups.
    pub pigments: Vec<f32>,
    /// 0..1 pull toward the nearest pigments.
    pub harmonize: f32,
    /// Hue distance (degrees) over which a pigment attracts.
    pub pigment_spread: f32,
    /// Images whose 99th-percentile OKLab chroma is below this are tint-safe (lightness only).
    pub tint_safe_chroma: f32,
    /// 0..1: how far high-chroma source colors may exceed the chroma caps.
    pub vivid: f32,
    /// Source OKLab chroma where vividness starts.
    pub vivid_threshold: f32,
    /// Chroma cap for fully vivid colors.
    pub vivid_max_chroma: f32,
    /// Optional hue ranges `[from, to]` (degrees, may wrap) that may be vivid; empty = all.
    pub vivid_hues: Vec<[f32; 2]>,
    /// Fraction of texels, by high-frequency darkness (crevices, gaps: source L below its local
    /// mean), that drop below the floor as colored accents. Low-frequency shading never counts.
    pub accent_fraction: f32,
    /// Local-mean radius for the high-frequency measure, in reference texels.
    pub accent_radius: f32,
    /// OKLCH lightness of the deepest accents.
    pub accent_min_l: f32,
    pub accent_hue: f32,
    pub accent_chroma: f32,
    /// Width of the accent threshold's soft transition, as a fraction of `accent_fraction`.
    pub accent_softness: f32,
    /// Minimum darkness relative to the surroundings (OKLab L) for any accent, so flat or
    /// noise-only textures get none.
    pub accent_min_depth: f32,
    /// Allow hue groups in `green_hue` their own (possibly low) floors. When false (a pack-map
    /// rule can force this per texture), those groups are floored at `light_green_floor`.
    pub dark_greens: bool,
    /// OKLCH hue range of the green groups `dark_greens` applies to.
    pub green_hue: [f32; 2],
    pub light_green_floor: f32,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            enabled: false,
            strength: 1.0,
            lut_size: 33,
            l_curve: Vec::new(),
            l_floor: 0.0,
            l_ceiling: 1.0,
            floor_knee: 0.5,
            chroma_scale: 1.0,
            chroma_cap: 0.4,
            chroma_lift: 0.0,
            dark_chroma: 0.0,
            dark_below: 0.45,
            dark_cool_bias: 0.0,
            dark_cool_hue: 255.0,
            warmth: Warmth::default(),
            cast: Cast::default(),
            neutral_c: 0.02,
            neutral_tint: Tint::default(),
            shadow_tint: Tint::default(),
            hue_feather: 10.0,
            groups: Vec::new(),
            pigments: Vec::new(),
            harmonize: 0.0,
            pigment_spread: 25.0,
            tint_safe_chroma: 0.03,
            vivid: 0.0,
            vivid_threshold: 0.15,
            vivid_max_chroma: 0.18,
            vivid_hues: Vec::new(),
            accent_fraction: 0.0,
            accent_radius: 4.0,
            accent_min_l: 0.42,
            accent_hue: 280.0,
            accent_chroma: 0.1,
            accent_softness: 0.6,
            accent_min_depth: 0.03,
            dark_greens: true,
            green_hue: [110.0, 175.0],
            light_green_floor: 0.0,
        }
    }
}

impl Palette {
    pub fn validate(&self) -> Result<()> {
        let p = self;
        unit("palette.l_floor", p.l_floor)?;
        unit("palette.l_ceiling", p.l_ceiling)?;
        check(p.l_floor <= p.l_ceiling, || {
            format!(
                "palette.l_floor ({}) is above palette.l_ceiling ({})",
                p.l_floor, p.l_ceiling
            )
        })?;
        unit("palette.floor_knee", p.floor_knee)?;
        non_negative("palette.strength", p.strength)?;
        check((2..=129).contains(&p.lut_size), || {
            format!("palette.lut_size = {} must be within 2..=129", p.lut_size)
        })?;
        for w in p.l_curve.windows(2) {
            check(w[1][0] > w[0][0] && w[1][1] >= w[0][1], || {
                format!(
                    "palette.l_curve must be increasing: {:?} then {:?}",
                    w[0], w[1]
                )
            })?;
        }
        for pt in &p.l_curve {
            unit("palette.l_curve input", pt[0])?;
            unit("palette.l_curve output", pt[1])?;
        }
        for g in &p.groups {
            let name = format!("palette.groups[{}]", g.name);
            unit(&format!("{name}.l_floor"), g.l_floor)?;
            check(g.l_floor <= p.l_ceiling, || {
                format!(
                    "{name}.l_floor ({}) is above palette.l_ceiling ({})",
                    g.l_floor, p.l_ceiling
                )
            })?;
            non_negative(&format!("{name}.c_scale"), g.c_scale)?;
            unit(&format!("{name}.hue_pull"), g.hue_pull)?;
        }
        non_negative("palette.chroma_cap", p.chroma_cap)?;
        unit("palette.harmonize", p.harmonize)?;
        unit("palette.vivid", p.vivid)?;
        check((0.0..=0.5).contains(&p.accent_fraction), || {
            format!(
                "palette.accent_fraction = {} must be within 0..=0.5",
                p.accent_fraction
            )
        })?;
        unit("palette.accent_min_l", p.accent_min_l)?;
        unit("palette.accent_softness", p.accent_softness)?;
        let k = &p.cast;
        unit("palette.cast.strength", k.strength)?;
        unit("palette.cast.chroma", k.chroma)?;
        non_negative("palette.cast.tint", k.tint)?;
        non_negative("palette.cast.dark_cap", k.dark_cap)?;
        check(k.exposure > 0.0 && k.exposure <= 1.0, || {
            format!(
                "palette.cast.exposure = {} must be within (0, 1]",
                k.exposure
            )
        })?;
        check(k.tint_full_l > 0.0, || {
            "palette.cast.tint_full_l must be > 0".into()
        })?;
        Ok(())
    }
}
