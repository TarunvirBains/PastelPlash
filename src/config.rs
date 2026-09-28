//! The three configuration layers (see `PLAN.md`): style, target and pack map.
//!
//! Every field has a default, so each file may be partial or absent. Defaults are neutral: an
//! empty style and target leave textures unchanged. Relative paths inside a file are resolved
//! against that file's folder.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;
use serde::de::DeserializeOwned;

pub use crate::mood::Mood;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    /// Relit by the target's actor lighting.
    Actor,
    /// Static world geometry.
    World,
    Skybox,
    /// Pre-rendered scene images (painted backdrops): no tiling assumptions, no de-lighting.
    Background,
    Ui,
    /// Copied through untouched.
    Skip,
}

impl std::str::FromStr for Category {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        Ok(match s.to_ascii_lowercase().as_str() {
            "actor" => Self::Actor,
            "world" => Self::World,
            "skybox" => Self::Skybox,
            "background" => Self::Background,
            "ui" => Self::Ui,
            "skip" => Self::Skip,
            _ => {
                return Err(format!(
                    "unknown category {s:?} (actor, world, skybox, background, ui, skip)"
                ));
            }
        })
    }
}

/// What it should look like.
///
/// Sizes given "at the reference size" are in texels for a texture whose geometric-mean side
/// (`sqrt(w·h)`) equals [`Scale::reference_size`], and scale with resolution so a 256 px and a
/// 4096 px texture get a comparable painterly scale.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Style {
    pub name: String,
    /// 3D `.cube` palette LUT; replaces the generated `[palette]` mapping when set.
    pub lut: Option<PathBuf>,
    pub scale: Scale,
    pub tiling: Tiling,
    pub delight: Delight,
    pub kuwahara: Kuwahara,
    pub palette: Palette,
    pub temperature: Temperature,
    pub strokes: Strokes,
    pub value_contrast: ValueContrast,
    pub contrast: Contrast,
    pub abstraction: Abstraction,
    pub marks: Marks,
    pub watercolor: Watercolor,
    /// Named moods: partial overrides of this style (see `src/mood.rs`). The style itself is
    /// the `base` mood.
    pub moods: BTreeMap<String, toml::Table>,
    /// The file's TOML, kept to derive moods from.
    #[serde(skip)]
    pub raw: Option<toml::Table>,
}

/// How style sizes (in "reference texels") become texels of a given image.
///
/// When the image's **source scale** (HD texels per original texel, from an adapter, the pack
/// map or [`Scale::source_scale`]) is known, the factor is `source_scale /
/// reference_source_scale`, so brush sizes stay consistent in world space. Otherwise it falls
/// back to image size: `(sqrt(w·h) / reference_size) ^ exponent`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Scale {
    /// Geometric-mean side length at which the style's texel sizes apply as written.
    pub reference_size: f32,
    /// Size-relative fallback exponent; 1 = fully proportional.
    pub exponent: f32,
    /// Source scale at which the style's texel sizes apply as written.
    pub reference_source_scale: f32,
    /// Default source scale for images whose adapter and pack map give none.
    pub source_scale: Option<f32>,
}

impl Default for Scale {
    fn default() -> Self {
        Self {
            reference_size: 1024.0,
            exponent: 1.0,
            reference_source_scale: 16.0,
            source_scale: None,
        }
    }
}

/// Directional brushstrokes: noise integrated along the structure-tensor flow (LIC), modulating
/// lightness and chroma slightly. Never moves edges.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Strokes {
    /// Lightness modulation amplitude (OKLab L); 0 disables.
    pub strength: f32,
    /// Relative chroma modulation.
    pub chroma: f32,
    /// Bristle/stroke width in reference texels.
    pub width: f32,
    /// Stroke half-length in reference texels.
    pub length: f32,
    /// 0..1: how far the painted wash is pulled toward the source color averaged along the
    /// flow, bringing detail back as streaks that follow form (not across edges).
    pub smear: f32,
}

impl Default for Strokes {
    fn default() -> Self {
        Self {
            strength: 0.0,
            chroma: 0.15,
            width: 2.0,
            length: 10.0,
            smear: 0.0,
        }
    }
}

/// Local value-contrast compression: each texel's lightness is pulled toward edge-aware local
/// means at three scales, so a surface sits in a narrower lightness range (as in Skyward Sword)
/// and detail moves from light/dark into hue and brushwork. Scaled per category by the target's
/// `value_contrast`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ValueContrast {
    /// 0..1 compression of deviations from the fine-scale mean (grime, pores, grain).
    pub fine: f32,
    /// 0..1 compression of fine-vs-mid means (small shapes).
    pub mid: f32,
    /// 0..1 compression of mid-vs-coarse means (large shapes; keep low so blocks vs. mortar stay
    /// readable).
    pub coarse: f32,
    /// Chroma gain per unit of lightness removed, so detail migrates into color.
    pub chroma: f32,
    /// Radii of the three scales in reference texels.
    pub radius_fine: f32,
    pub radius_mid: f32,
    pub radius_coarse: f32,
    /// Lightness difference (OKLab L) beyond which a neighbor counts as across an edge.
    pub range: f32,
}

impl Default for ValueContrast {
    fn default() -> Self {
        Self {
            fine: 0.0,
            mid: 0.0,
            coarse: 0.0,
            chroma: 0.0,
            radius_fine: 3.0,
            radius_mid: 12.0,
            radius_coarse: 40.0,
            range: 0.12,
        }
    }
}

/// Adaptive contrast for busy, high-contrast textures (bark, cliffs): the texture's own
/// value spread (median OKLab L std over mid-scale windows) is measured, and if it is above
/// `trigger_spread` the value compression is raised at every scale — including the groove scale —
/// toward `target_spread` (the spread measured on the reference game's comparable surfaces).
/// Groove shapes stay; their light/dark amplitude shrinks. Textures below the trigger (the
/// ground) are untouched by this.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Contrast {
    /// 0..1: how far toward the target; 0 disables.
    pub strength: f32,
    /// Goal spread (median L std over mid-scale windows).
    pub target_spread: f32,
    /// Only textures whose spread exceeds this (feathered ±15%) adapt.
    pub trigger_spread: f32,
}

/// Design-like abstraction for busy, photographic textures (the same trigger as [`Contrast`]):
/// a large-radius Kuwahara pass first simplifies the texture into big shapes; brushstrokes then
/// follow the coarse structure and paint the simplified wash instead of the photo; wet edges only
/// outline large regions; bright grayish glare calms into the surface color; texels keep their
/// source chroma. Shape-based textures below the trigger (vines, ground) are unaffected.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Abstraction {
    /// 0..1 overall amount on fully busy textures; 0 disables.
    pub strength: f32,
    /// Radius of the large-scale Kuwahara pass, in reference texels.
    pub radius: f32,
    /// Structure-tensor smoothing multiplier (strokes follow the coarse structure).
    pub flow_scale: f32,
    /// Stroke length multiplier.
    pub stroke_scale: f32,
    /// Coarse step for large-region edges, as a multiple of the wet-edge width.
    pub edge_scale: f32,
    /// 0..1 reduction of the remaining wet edges.
    pub edge_soften: f32,
    /// 0..1 calming of bright grayish highlight patches, and their neighborhood radius
    /// (reference texels).
    pub highlight_calm: f32,
    pub highlight_radius: f32,
    /// 0..1: output chroma kept at least this share of the source texel's chroma.
    pub chroma_retain: f32,
    /// Lower bound for the abstraction radius as a fraction of the texture's size (highlight
    /// radius: twice this), so low-resolution textures stretched over big surfaces still get
    /// big shapes.
    pub min_frac: f32,
}

impl Default for Abstraction {
    fn default() -> Self {
        Self {
            min_frac: 0.03,
            strength: 0.0,
            radius: 20.0,
            flow_scale: 3.0,
            stroke_scale: 1.8,
            edge_scale: 4.0,
            edge_soften: 0.5,
            highlight_calm: 0.8,
            highlight_radius: 12.0,
            chroma_retain: 0.9,
        }
    }
}

impl Default for Contrast {
    fn default() -> Self {
        Self {
            strength: 0.0,
            target_spread: 0.03,
            trigger_spread: 0.08,
        }
    }
}

/// Warm/cool contrast: residual low-frequency shading becomes hue temperature (lit → warm,
/// shade → cool). Scaled per category by the target's `warm_cool` (0 for relit actors).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Temperature {
    /// OKLab chroma added at full temperature; 0 disables.
    pub chroma: f32,
    pub warm_hue: f32,
    pub cool_hue: f32,
    /// Temperature per stop of shading (log2 of blurred luminance vs. mean).
    pub sensitivity: f32,
}

impl Default for Temperature {
    fn default() -> Self {
        Self {
            chroma: 0.0,
            warm_hue: 70.0,
            cool_hue: 285.0,
            sensitivity: 1.5,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Tiling {
    /// An axis wraps when its seam discontinuity is at most this multiple of the average
    /// neighbor difference inside the image.
    pub threshold: f32,
}

impl Default for Tiling {
    fn default() -> Self {
        Self { threshold: 2.5 }
    }
}

/// Removal of baked low-frequency shading/AO: linear color is divided by a large-radius blurred
/// luminance (relative to the image mean), raised to `strength × target delight`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Delight {
    /// Global multiplier on the target's per-category `delight`; 0 disables.
    pub strength: f32,
    /// Blur sigma as a fraction of the image's geometric-mean side.
    pub radius: f32,
    pub min_gain: f32,
    pub max_gain: f32,
}

impl Default for Delight {
    fn default() -> Self {
        Self {
            strength: 0.0,
            radius: 0.06,
            min_gain: 0.6,
            max_gain: 2.0,
        }
    }
}

/// Anisotropic Kuwahara filter with polynomial sector weights (Kyprianidis et al.).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Kuwahara {
    /// Filter radius in texels at the reference size; 0 disables the filter.
    pub radius: f32,
    /// Blend between the de-lit input (0) and the filtered result (1).
    pub strength: f32,
    /// Sector selection sharpness `q`; higher keeps edges crisper.
    pub sharpness: f32,
    /// Sector variance scale; higher prefers flat sectors more strongly.
    pub hardness: f32,
    /// Anisotropy tuning `α`; larger keeps ellipses rounder.
    pub anisotropy: f32,
    /// Polynomial weight zero crossing (radians-ish, ~0.58 ≈ 8-sector overlap).
    pub zero_crossing: f32,
    /// Structure-tensor smoothing sigma in texels at the reference size.
    pub tensor_sigma: f32,
    /// Hard cap on the scaled radius (cost grows with its square).
    pub max_radius: f32,
    /// Floor on the scaled radius so small textures still get painted.
    pub min_radius: f32,
}

impl Default for Kuwahara {
    fn default() -> Self {
        Self {
            radius: 0.0,
            strength: 1.0,
            sharpness: 8.0,
            hardness: 8.0,
            anisotropy: 1.0,
            zero_crossing: 0.58,
            tensor_sigma: 2.0,
            max_radius: 24.0,
            min_radius: 2.0,
        }
    }
}

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

/// Watercolor finish; every strength defaults to 0 (off).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Watercolor {
    /// Wet-edge darkening: maximum OKLab L drop where pigment pools on the darker side of a
    /// painted boundary; 0 disables.
    pub edge_darkening: f32,
    /// Rim darkening relative to the wash depth (`1 - L`), capped by `edge_darkening`.
    pub edge_relative: f32,
    /// Lightness step across a boundary at which rims start; higher rims fewer boundaries.
    pub edge_threshold: f32,
    /// Lightening on the lighter side of rimmed boundaries (OKLab L).
    pub edge_feather: f32,
    /// Rim half-width in reference texels.
    pub edge_width: f32,
    /// Color bleeding across soft boundaries (edge-aware; strong edges block it).
    pub bleed: f32,
    /// Bleed reach in texels at the reference size.
    pub bleed_radius: f32,
    /// OKLab distance at which a boundary stops the bleed.
    pub bleed_range: f32,
    /// Pigment granulation amplitude (OKLab L at full wash depth); 0 disables.
    pub granulation: f32,
    /// Granulation noise cell size in texels at the reference size.
    pub granulation_scale: f32,
    /// Share of granulation that settles in the source texture's own valleys (vs. noise).
    pub granulation_valley: f32,
    /// Paper tooth amplitude (OKLab L); 0 disables.
    pub paper_grain: f32,
    /// How far highlight colors mix toward the paper color (0..1 at full highlight); keep it
    /// small, or light colors turn chalky.
    pub paper_tint: f32,
    /// Paper texture cell size in texels at the reference size.
    pub paper_scale: f32,
    /// OKLab lightness above which the paper starts to show.
    pub paper_highlight: f32,
    /// Paper color, gamma sRGB.
    pub paper_color: [f32; 3],
    /// How far watercolor darkening may undercut the palette's lightness floor (OKLab L).
    pub floor_margin: f32,
    pub seed: u32,
}

impl Default for Watercolor {
    fn default() -> Self {
        Self {
            edge_darkening: 0.0,
            edge_relative: 0.5,
            edge_threshold: 0.02,
            edge_feather: 0.0,
            edge_width: 1.5,
            bleed: 0.0,
            bleed_radius: 4.0,
            bleed_range: 0.06,
            granulation: 0.0,
            granulation_scale: 3.0,
            granulation_valley: 0.6,
            paper_grain: 0.0,
            paper_tint: 0.0,
            paper_scale: 1.5,
            paper_highlight: 0.75,
            paper_color: [0.98, 0.965, 0.93],
            floor_margin: 0.06,
            seed: 0,
        }
    }
}

/// How the game will render it.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Target {
    pub name: String,
    /// Per-category treatment; categories not listed get [`Treatment::default`].
    pub categories: BTreeMap<Category, Treatment>,
}

impl Target {
    pub fn treatment(&self, category: Category) -> Treatment {
        self.categories.get(&category).cloned().unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Treatment {
    /// How strongly to remove baked shading, 0..=1 (multiplied by the style's delight strength).
    pub delight: f32,
    /// Maximum OKLCH lightness, for textures the renderer brightens further.
    pub lightness_ceiling: Option<f32>,
    /// Width (OKLab L) of the soft knee below the ceiling; only values within it are
    /// compressed.
    pub ceiling_knee: f32,
    /// Multiplies the palette's hue shifts, pulls and harmonization (e.g. < 1 to keep actors'
    /// own skin, hair and leather hues).
    pub hue: f32,
    /// Force tint-safe (lightness-only palette) on or off; unset = detect from chroma.
    pub tint_safe: Option<bool>,
    /// Scales the palette's lightness lift (e.g. < 1 for lava or dark dungeon areas).
    pub floor_scale: f32,
    /// Multiplies the style's Kuwahara radius.
    pub radius_scale: f32,
    /// Warm/cool temperature strength (0 for relit categories: the renderer decides lit/shade).
    pub warm_cool: f32,
    /// Multiplies the accent-dark fraction and depth.
    pub accent: f32,
    /// Multiplies brushstroke strength.
    pub strokes: f32,
    /// Multiplies brushstroke width and length.
    pub stroke_scale: f32,
    /// Multiplies the palette's shadow tint amount.
    pub shadow_tint: f32,
    /// Multiplies the paper tooth and paper tint.
    pub paper: f32,
    /// Multiplies the style's value-contrast compression.
    pub value_contrast: f32,
    /// Multiplies the design-like abstraction (0 for finished paintings such as pre-rendered
    /// backgrounds).
    pub abstraction: f32,
    /// Multiplies the palette's earth warmth.
    pub warmth: f32,
}

impl Default for Treatment {
    fn default() -> Self {
        Self {
            delight: 0.0,
            lightness_ceiling: None,
            ceiling_knee: 0.15,
            hue: 1.0,
            tint_safe: None,
            floor_scale: 1.0,
            radius_scale: 1.0,
            warm_cool: 0.0,
            accent: 1.0,
            strokes: 1.0,
            stroke_scale: 1.0,
            shadow_tint: 1.0,
            paper: 1.0,
            value_contrast: 1.0,
            warmth: 1.0,
            abstraction: 1.0,
        }
    }
}

/// Which file is what.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Pack {
    pub name: String,
    /// Path/filename glob rules, first match wins.
    pub rules: Vec<Rule>,
    /// CSV of `filename,category` for hand-sorted packs.
    pub list: Option<PathBuf>,
    /// Category for files nothing else classifies.
    pub default_category: Category,
    /// Filename-stem suffixes (case-insensitive) marking non-color maps, which are copied through.
    pub non_color_suffixes: Vec<String>,
    /// HD texels per original texel for this pack, when adapters don't supply it per image.
    pub source_scale: Option<f32>,
    /// Mood rules by path glob, first match wins; unmatched files get the base mood.
    pub moods: Vec<MoodRule>,
    /// Paint-mark size rules by path glob, first match wins; unmatched files get scale 1.
    pub marks: Vec<MarksRule>,
}

/// Scales the paint-mark size (the painting Kuwahara radius) of matching files, e.g. for
/// textures that tile many times, whose texture-space marks shrink in world space.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarksRule {
    pub glob: String,
    pub scale: f32,
}

/// Paint marks: the size of the soft dabs/blotches that fine color variation is simplified into.
///
/// Seamlessly tiling textures repeat many times across a surface, which shrinks their
/// texture-space marks in world space, so they get `tiling_multiplier` × the size — but only as
/// far as they are *speckled* (fine-scale lightness variation large relative to mid-scale, the
/// signature of photographic grit), so shape-based art (vines, leaves) keeps its shapes.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Marks {
    /// Mark radius in reference texels; replaces `kuwahara.radius` when set.
    pub size: Option<f32>,
    /// Mark-size multiplier for tiling, speckled textures (1 = off).
    pub tiling_multiplier: f32,
    /// Speckle ratio (fine / mid-scale L std) range over which the multiplier fades in.
    pub speckle: [f32; 2],
}

impl Default for Marks {
    fn default() -> Self {
        Self {
            size: None,
            tiling_multiplier: 1.0,
            speckle: [0.45, 0.6],
        }
    }
}

/// Assigns a mood (and optionally allows or denies dark greens) to matching files.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoodRule {
    pub glob: String,
    pub mood: String,
    #[serde(default = "one")]
    pub strength: f32,
    pub dark_greens: Option<bool>,
}

fn one() -> f32 {
    1.0
}

impl Default for Pack {
    fn default() -> Self {
        Self {
            source_scale: None,
            moods: Vec::new(),
            marks: Vec::new(),
            name: String::new(),
            rules: Vec::new(),
            list: None,
            default_category: Category::World,
            non_color_suffixes: ["_n", "_nrm", "_normal", "_spec", "_rough"]
                .map(String::from)
                .into(),
        }
    }
}

impl Pack {
    /// Category by the first matching rule, else [`Pack::default_category`].
    pub fn classify(&self, path: &Path) -> Category {
        let p = path.to_string_lossy().replace('\\', "/");
        self.rules
            .iter()
            .find(|r| glob_match(&r.glob, &p))
            .map_or(self.default_category, |r| r.category)
    }

    /// Paint-mark scale by the first matching marks rule, else 1.
    pub fn marks_scale_for(&self, path: &Path) -> f32 {
        let p = path.to_string_lossy().replace('\\', "/");
        self.marks
            .iter()
            .find(|r| glob_match(&r.glob, &p))
            .map_or(1.0, |r| r.scale)
    }

    /// Mood by the first matching mood rule, else the base mood.
    pub fn mood_for(&self, path: &Path) -> Mood {
        let p = path.to_string_lossy().replace('\\', "/");
        self.moods
            .iter()
            .find(|r| glob_match(&r.glob, &p))
            .map_or_else(Mood::default, |r| Mood {
                name: r.mood.clone(),
                strength: r.strength,
                dark_greens: r.dark_greens,
            })
    }

    /// True if the file stem ends in one of [`Pack::non_color_suffixes`].
    pub fn is_non_color_map(&self, path: &Path) -> bool {
        let Some(stem) = path.file_stem() else {
            return false;
        };
        let stem = stem.to_string_lossy().to_lowercase();
        self.non_color_suffixes
            .iter()
            .any(|s| stem.ends_with(&s.to_lowercase()))
    }
}

/// Case-sensitive glob over `/`-separated paths: `*` and `?` stay within a segment, `**`
/// crosses segments.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    fn go(p: &[u8], s: &[u8]) -> bool {
        match p {
            [] => s.is_empty(),
            [b'*', b'*', rest @ ..] => {
                let rest = rest.strip_prefix(b"/").unwrap_or(rest);
                (0..=s.len()).any(|i| go(rest, &s[i..]))
            }
            [b'*', rest @ ..] => (0..=s.len())
                .take_while(|&i| i == 0 || s[i - 1] != b'/')
                .any(|i| go(rest, &s[i..])),
            [b'?', rest @ ..] => !s.is_empty() && s[0] != b'/' && go(rest, &s[1..]),
            [c, rest @ ..] => !s.is_empty() && s[0] == *c && go(rest, &s[1..]),
        }
    }
    go(pattern.as_bytes(), path.as_bytes())
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub glob: String,
    pub category: Category,
}

/// All three layers, loaded from optional files.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Config {
    pub style: Style,
    pub target: Target,
    pub pack: Pack,
}

impl Config {
    pub fn load(style: Option<&Path>, target: Option<&Path>, pack: Option<&Path>) -> Result<Self> {
        let mut config = Self {
            style: match style {
                Some(path) => Style::load(path)?,
                None => Style::default(),
            },
            target: load_or_default(target)?,
            pack: load_or_default(pack)?,
        };
        if let Some(base) = style.and_then(Path::parent) {
            resolve(&mut config.style.lut, base);
        }
        if let Some(base) = pack.and_then(Path::parent) {
            resolve(&mut config.pack.list, base);
        }
        if let Some(path) = style {
            let invalid = || format!("invalid style {}", path.display());
            config.style.validate().with_context(invalid)?;
            // Every mood must be valid in full and half blended with the base.
            for name in config.style.moods.keys() {
                for strength in [0.5, 1.0] {
                    let mood = Mood {
                        name: name.clone(),
                        strength,
                        dark_greens: None,
                    };
                    config.style.for_mood(&mood).with_context(invalid)?;
                }
            }
        }
        if let Some(path) = target {
            config
                .target
                .validate()
                .with_context(|| format!("invalid target {}", path.display()))?;
        }
        Ok(config)
    }
}

fn check(ok: bool, msg: impl FnOnce() -> String) -> Result<()> {
    if ok { Ok(()) } else { anyhow::bail!(msg()) }
}

fn unit(name: &str, v: f32) -> Result<()> {
    check((0.0..=1.0).contains(&v), || {
        format!("{name} = {v} must be within 0..=1")
    })
}

fn non_negative(name: &str, v: f32) -> Result<()> {
    check(v >= 0.0 && v.is_finite(), || {
        format!("{name} = {v} must be >= 0")
    })
}

impl Style {
    /// Loads a style file, following `extends` (a path relative to the file): the file's
    /// settings are deep-merged over the style it extends.
    pub fn load(path: &Path) -> Result<Self> {
        let raw = Self::load_raw(path, 0)?;
        Self::from_table(raw).with_context(|| format!("parsing {}", path.display()))
    }

    fn load_raw(path: &Path, depth: usize) -> Result<toml::Table> {
        anyhow::ensure!(
            depth < 8,
            "style `extends` chain too deep at {}",
            path.display()
        );
        let text =
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let mut raw: toml::Table =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        let Some(base) = raw.remove("extends") else {
            return Ok(raw);
        };
        let base = base
            .as_str()
            .with_context(|| format!("{}: `extends` must be a path", path.display()))?;
        let base_path = path.parent().unwrap_or(Path::new(".")).join(base);
        let base_raw = Self::load_raw(&base_path, depth + 1)?;
        Ok(crate::mood::blend_table(&base_raw, &raw, 1.0))
    }

    /// Parses a style, keeping its TOML so moods can be derived from it.
    pub fn parse(text: &str) -> Result<Self> {
        Self::from_table(toml::from_str(text)?)
    }

    fn from_table(raw: toml::Table) -> Result<Self> {
        let mut style: Style = toml::Value::Table(raw.clone()).try_into()?;
        style.raw = Some(raw);
        Ok(style)
    }

    /// The style for a mood: the base itself for `base`, otherwise the base blended toward
    /// the mood's overrides by its strength. A mood's `dark_greens` setting is applied last.
    pub fn for_mood(&self, mood: &Mood) -> Result<Style> {
        let mut style = if mood.name == crate::mood::BASE || mood.strength <= 0.0 {
            self.clone()
        } else {
            let over = self.moods.get(&mood.name).with_context(|| {
                let known: Vec<&str> = self.moods.keys().map(String::as_str).collect();
                format!(
                    "style {:?} has no mood {:?} (it has: base{}{})",
                    self.name,
                    mood.name,
                    if known.is_empty() { "" } else { ", " },
                    known.join(", ")
                )
            })?;
            let raw = self
                .raw
                .as_ref()
                .context("style was not loaded from TOML, so it has no moods")?;
            let mut table = crate::mood::blend_table(raw, over, mood.strength.min(1.0) as f64);
            table.remove("moods");
            let mut derived: Style = toml::Value::Table(table)
                .try_into()
                .with_context(|| format!("mood {:?}", mood.name))?;
            derived.lut = self.lut.clone();
            derived.moods = self.moods.clone();
            derived.raw = self.raw.clone();
            derived
        };
        if let Some(dark) = mood.dark_greens {
            style.palette.dark_greens = dark;
        }
        style.validate().with_context(|| format!("mood {mood}"))?;
        Ok(style)
    }

    /// Rejects settings that are out of range or contradict each other.
    pub fn validate(&self) -> Result<()> {
        let p = &self.palette;
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
        let k = &self.kuwahara;
        non_negative("kuwahara.radius", k.radius)?;
        unit("kuwahara.strength", k.strength)?;
        check(k.min_radius <= k.max_radius, || {
            format!(
                "kuwahara.min_radius ({}) is above kuwahara.max_radius ({})",
                k.min_radius, k.max_radius
            )
        })?;
        check(k.anisotropy > 0.0, || {
            "kuwahara.anisotropy must be > 0".into()
        })?;
        check(self.scale.reference_size > 0.0, || {
            "scale.reference_size must be > 0".into()
        })?;
        let w = &self.watercolor;
        for (name, v) in [
            ("watercolor.edge_darkening", w.edge_darkening),
            ("watercolor.bleed", w.bleed),
            ("watercolor.granulation", w.granulation),
            ("watercolor.paper_grain", w.paper_grain),
            ("watercolor.floor_margin", w.floor_margin),
            ("strokes.strength", self.strokes.strength),
            ("temperature.chroma", self.temperature.chroma),
            ("delight.strength", self.delight.strength),
        ] {
            non_negative(name, v)?;
        }
        check(self.delight.min_gain <= self.delight.max_gain, || {
            "delight.min_gain is above delight.max_gain".into()
        })?;
        Ok(())
    }
}

impl Target {
    pub fn validate(&self) -> Result<()> {
        for (cat, t) in &self.categories {
            if let Some(c) = t.lightness_ceiling {
                check(c > 0.0 && c <= 1.0, || {
                    format!("categories.{cat:?}.lightness_ceiling = {c} must be within (0, 1]")
                })?;
            }
            unit(&format!("categories.{cat:?}.delight"), t.delight)?;
            non_negative(&format!("categories.{cat:?}.warm_cool"), t.warm_cool)?;
            non_negative(&format!("categories.{cat:?}.floor_scale"), t.floor_scale)?;
        }
        Ok(())
    }
}

pub fn load<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

fn load_or_default<T: DeserializeOwned + Default>(path: Option<&Path>) -> Result<T> {
    path.map_or_else(|| Ok(T::default()), load)
}

fn resolve(path: &mut Option<PathBuf>, base: &Path) {
    if let Some(p) = path.as_mut().filter(|p| p.is_relative()) {
        *p = base.join(&*p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_files_give_defaults() {
        assert_eq!(toml::from_str::<Style>("").unwrap(), Style::default());
        assert_eq!(toml::from_str::<Target>("").unwrap(), Target::default());
        assert_eq!(toml::from_str::<Pack>("").unwrap(), Pack::default());
    }

    #[test]
    fn full_files_parse() {
        let style: Style = toml::from_str(
            "name = 'sky'\nlut = 'sky.cube'\n[kuwahara]\nradius = 6\nstrength = 0.8\n\
             [watercolor]\npaper_grain = 0.3",
        )
        .unwrap();
        assert_eq!(style.kuwahara.radius, 6.0);
        assert_eq!(style.kuwahara.sharpness, Kuwahara::default().sharpness);
        assert_eq!(style.watercolor.paper_grain, 0.3);

        let target: Target =
            toml::from_str("[categories.actor]\ndelight = 1.0\nlightness_ceiling = 0.85").unwrap();
        assert_eq!(
            target.treatment(Category::Actor).lightness_ceiling,
            Some(0.85)
        );
        assert_eq!(target.treatment(Category::World), Treatment::default());

        let pack: Pack = toml::from_str(
            "default_category = 'actor'\n\
             [[rules]]\nglob = 'objects/**'\ncategory = 'actor'\n\
             [[rules]]\nglob = 'ui/**'\ncategory = 'skip'",
        )
        .unwrap();
        assert_eq!(pack.rules.len(), 2);
        assert_eq!(pack.rules[1].category, Category::Skip);
        assert_eq!(pack.non_color_suffixes.len(), 5);
    }

    #[test]
    fn palette_tables_parse() {
        let style: Style = toml::from_str(
            "[palette]\nenabled = true\npigments = [60, 120.5]\nl_curve = [[0, 0.5], [1, 0.97]]\n\
             shadow_tint = { hue = 72, chroma = 0.03, amount = 0.5 }\n\
             [[palette.groups]]\nname = 'green'\nhue_range = [115, 170]\nl_floor = 0.78",
        )
        .unwrap();
        assert_eq!(style.palette.groups.len(), 1);
        assert_eq!(style.palette.groups[0].c_scale, 1.0);
        assert_eq!(style.palette.shadow_tint.below_input_l, 0.25);
        assert_eq!(style.palette.pigments, vec![60.0, 120.5]);
        assert!(toml::from_str::<Style>("[[palette.groups]]\nl_flor = 0.7").is_err());
    }

    #[test]
    fn categories_parse_from_cli_strings() {
        assert_eq!("Actor".parse::<Category>(), Ok(Category::Actor));
        assert!("actors".parse::<Category>().is_err());
    }

    #[test]
    fn contradictory_settings_are_rejected_with_clear_errors() {
        let style: Style = toml::from_str("[palette]\nl_floor = 0.9\nl_ceiling = 0.8").unwrap();
        let err = style.validate().unwrap_err().to_string();
        assert!(
            err.contains("l_floor") && err.contains("l_ceiling"),
            "{err}"
        );
        let style: Style =
            toml::from_str("[palette]\nl_curve = [[0, 0.5], [0.5, 0.4], [1, 1]]").unwrap();
        assert!(style.validate().is_err());
        let style: Style = toml::from_str("[kuwahara]\nradius = -1").unwrap();
        assert!(style.validate().is_err());
        let target: Target = toml::from_str("[categories.actor]\nlightness_ceiling = 1.5").unwrap();
        assert!(target.validate().is_err());
        assert!(Style::default().validate().is_ok());
        assert!(Target::default().validate().is_ok());
    }

    #[test]
    fn globs() {
        assert!(glob_match(
            "alt/scenes/**",
            "alt/scenes/shared/spot04_scene/x"
        ));
        assert!(glob_match(
            "alt/objects/object_link_boy/*",
            "alt/objects/object_link_boy/gTex"
        ));
        assert!(!glob_match(
            "alt/objects/object_link_boy/*",
            "alt/objects/object_link_boy/a/b"
        ));
        assert!(glob_match(
            "alt/**/*Eyes*",
            "alt/objects/object_link_boy/gLinkAdultEyesOpenTex"
        ));
        assert!(glob_match(
            "**/spot04_scene/**",
            "alt/scenes/nonmq/spot04_scene/t"
        ));
        assert!(!glob_match(
            "alt/textures/vr_*/**",
            "alt/textures/parameter_static/x"
        ));
    }

    #[test]
    fn pack_rules_classify_first_match_wins() {
        let pack: Pack = toml::from_str(
            "default_category = 'world'\n\
             [[rules]]\nglob = '**/*Eyes*'\ncategory = 'skip'\n\
             [[rules]]\nglob = 'alt/objects/**'\ncategory = 'actor'",
        )
        .unwrap();
        assert_eq!(
            pack.classify(Path::new("alt/objects/o/gEyesTex")),
            Category::Skip
        );
        assert_eq!(
            pack.classify(Path::new("alt/objects/o/gBodyTex")),
            Category::Actor
        );
        assert_eq!(pack.classify(Path::new("alt/scenes/s/t")), Category::World);
    }

    #[test]
    fn moods_derive_from_the_base_style() {
        let style = Style::parse(
            "name = 's'\n[palette]\nenabled = true\nl_floor = 0.6\nl_ceiling = 0.95\n\
             [moods.nocturne.palette]\nl_floor = 0.3\nl_ceiling = 0.85",
        )
        .unwrap();
        let mood = |name: &str, strength| Mood {
            name: name.into(),
            strength,
            dark_greens: None,
        };
        let full = style.for_mood(&mood("nocturne", 1.0)).unwrap();
        assert_eq!(full.palette.l_floor, 0.3);
        assert!(full.palette.enabled, "unset keys come from the base");
        let half = style.for_mood(&mood("nocturne", 0.5)).unwrap();
        assert!((half.palette.l_floor - 0.45).abs() < 1e-6);
        assert_eq!(style.for_mood(&Mood::default()).unwrap(), style);
        let err = style.for_mood(&mood("gloom", 1.0)).unwrap_err().to_string();
        assert!(err.contains("gloom") && err.contains("nocturne"), "{err}");
        let denied = style
            .for_mood(&Mood {
                dark_greens: Some(false),
                ..mood("nocturne", 1.0)
            })
            .unwrap();
        assert!(!denied.palette.dark_greens);
    }

    #[test]
    fn styles_extend_other_styles() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("base.toml"),
            "name = 'base'\n[palette]\nenabled = true\nl_floor = 0.6\n\
             [[palette.groups]]\nname = 'a'\nl_floor = 0.7\nc_scale = 0.5\n\
             [moods.nocturne.palette]\nl_floor = 0.3",
        )
        .unwrap();
        let child = dir.path().join("child.toml");
        fs::write(
            &child,
            "extends = 'base.toml'\nname = 'child'\n[palette]\nstrength = 1.3\n\
             [[palette.groups]]\nname = 'a'\nc_scale = 0.9",
        )
        .unwrap();
        let s = Config::load(Some(&child), None, None).unwrap().style;
        assert_eq!(s.name, "child");
        assert_eq!((s.palette.strength, s.palette.l_floor), (1.3, 0.6));
        assert_eq!(
            (s.palette.groups[0].l_floor, s.palette.groups[0].c_scale),
            (0.7, 0.9)
        );
        assert!(s.moods.contains_key("nocturne"), "moods are inherited");
    }

    #[test]
    fn invalid_moods_are_rejected_on_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.toml");
        fs::write(
            &path,
            "[palette]\nl_ceiling = 0.9\n[moods.nocturne.palette]\nl_floor = 0.95",
        )
        .unwrap();
        let err = format!("{:#}", Config::load(Some(&path), None, None).unwrap_err());
        assert!(err.contains("nocturne") && err.contains("l_floor"), "{err}");
    }

    #[test]
    fn pack_mood_rules_first_match_wins() {
        let pack: Pack = toml::from_str(
            "[[moods]]\nglob = 'alt/scenes/*/ydan_scene/*Moss*'\nmood = 'nocturne'\n\
             dark_greens = false\n\
             [[moods]]\nglob = 'alt/scenes/*/ydan_scene/**'\nmood = 'nocturne'\nstrength = 0.6",
        )
        .unwrap();
        let m = pack.mood_for(Path::new("alt/scenes/shared/ydan_scene/wall"));
        assert_eq!(
            (m.name.as_str(), m.strength, m.dark_greens),
            ("nocturne", 0.6, None)
        );
        let moss = pack.mood_for(Path::new("alt/scenes/nonmq/ydan_scene/gMossTex"));
        assert_eq!((moss.strength, moss.dark_greens), (1.0, Some(false)));
        assert!(
            pack.mood_for(Path::new("alt/scenes/shared/spot04_scene/x"))
                .is_base()
        );
    }

    #[test]
    fn typos_are_rejected() {
        assert!(toml::from_str::<Style>("[kuwahara]\nradios = 3").is_err());
        assert!(toml::from_str::<Pack>("[[rules]]\nglob = 'a'\ncategory = 'actors'").is_err());
    }

    #[test]
    fn non_color_maps_by_suffix() {
        let pack = Pack::default();
        assert!(pack.is_non_color_map(Path::new("rock_N.png")));
        assert!(pack.is_non_color_map(Path::new("dir/metal_rough.PNG")));
        assert!(!pack.is_non_color_map(Path::new("normal.png")));
        assert!(!pack.is_non_color_map(Path::new("grass.png")));
        let none = Pack {
            non_color_suffixes: vec![],
            ..Pack::default()
        };
        assert!(!none.is_non_color_map(Path::new("rock_n.png")));
    }

    #[test]
    fn relative_paths_resolve_against_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let style = dir.path().join("style.toml");
        fs::write(&style, "lut = 'luts/sky.cube'").unwrap();
        let config = Config::load(Some(&style), None, None).unwrap();
        assert_eq!(config.style.lut, Some(dir.path().join("luts/sky.cube")));
    }
}
