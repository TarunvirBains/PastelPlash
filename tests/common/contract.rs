//! The style contract (`rules.toml`): the hard limits the rule tests read.

use std::sync::OnceLock;

use serde::Deserialize;

use super::repo;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contract {
    pub tolerance: Tolerance,
    pub palette: PaletteRules,
    pub accents: AccentRules,
    pub vivid: VividRules,
    pub technique: TechniqueRules,
    pub target: TargetRules,
    pub actor: ActorRules,
    pub identity: IdentityRules,
    pub fluid: FluidRules,
    pub resolution: ResolutionRules,
    /// Per-mood allowances (keyed by mood name).
    #[serde(default)]
    pub moods: std::collections::BTreeMap<String, MoodRules>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoodRules {
    pub dark_max_hue_shift: Option<f32>,
    pub max_cool_bias: Option<f32>,
    /// The mood may have a moonlight cast dimming down to this exposure (none if unset).
    pub min_exposure: Option<f32>,
    pub near_black_l: Option<f32>,
    pub near_neutral_c: Option<f32>,
    pub near_black_max_lift: Option<f32>,
    pub dark_chroma_per_l: Option<f32>,
    pub cast_max_chroma: Option<f32>,
    pub max_tiling_multiplier: Option<f32>,
    pub black_l: Option<f32>,
    pub black_max_chroma: Option<f32>,
    pub black_max_cast: Option<f32>,
    pub black_warm_slack: Option<f32>,
    pub black_fade_l: Option<f32>,
}

impl Contract {
    /// The dark-hue-shift bound for a style label like "watercolor [nocturne:0.50] World".
    pub fn dark_max_hue_shift(&self, label: &str) -> f32 {
        self.moods
            .iter()
            .find(|(m, _)| label.contains(&format!("[{m}")))
            .and_then(|(_, r)| r.dark_max_hue_shift)
            .unwrap_or(self.palette.dark_max_hue_shift)
    }

    /// Source lightness below which a case's mood may fade near-black chroma (see
    /// `moods.<name>.black_fade_l`; 0 for the base and unlisted moods), by label like
    /// "watercolor [nocturne:0.50] World".
    pub fn black_fade_l(&self, label: &str) -> f32 {
        self.moods
            .iter()
            .find(|(m, _)| label.contains(&format!("[{m}")))
            .and_then(|(_, r)| r.black_fade_l)
            .unwrap_or(0.0)
    }

    /// The contract's rules for a mood name, if it lists any.
    pub fn mood(&self, mood: &str) -> Option<&MoodRules> {
        self.moods.get(mood)
    }

    /// The cool-bias bound for a mood name ("base" and unlisted moods: 0).
    pub fn max_cool_bias(&self, mood: &str) -> f32 {
        self.moods
            .get(mood)
            .and_then(|r| r.max_cool_bias)
            .unwrap_or(0.0)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityRules {
    pub max_mean_delta_e: f32,
    pub max_group_hue_shift: f32,
    pub coarse_max_color: f32,
    pub coarse_max_lightness: f32,
    pub coarse_min_pattern_corr: f32,
    pub coarse_min_pattern_range: f32,
    pub family_share_max_change: f32,
    pub family_min_separation: f32,
    pub moss_hue: [f32; 2],
    pub moss_max_warm_shift: f32,
    pub new_hue_min_chroma: f32,
    pub new_hue_max_gap: f32,
    pub background_max_mean_l: f32,
    pub background_min_pattern_range: f32,
    /// Larger bounds for named opt-in styles.
    #[serde(default)]
    pub styles: std::collections::BTreeMap<String, f32>,
    #[serde(default)]
    pub coarse_lightness_styles: std::collections::BTreeMap<String, f32>,
}

impl IdentityRules {
    /// The coarse (chroma, lightness) bounds for a style.
    pub fn coarse_bounds(&self, style: &str) -> (f32, f32) {
        (
            self.coarse_max_color,
            self.coarse_lightness_styles
                .get(style)
                .copied()
                .unwrap_or(self.coarse_max_lightness),
        )
    }

    /// The mean-color ΔE bound for a style (by file stem).
    pub fn bound(&self, style: &str) -> f32 {
        self.styles
            .get(style)
            .copied()
            .unwrap_or(self.max_mean_delta_e)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tolerance {
    pub lightness: f32,
    pub chroma: f32,
    pub outliers: f32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaletteRules {
    pub min_strength: f32,
    pub max_strength: f32,
    pub max_floor_margin: f32,
    pub min_l: f32,
    pub dark_l: f32,
    pub dark_min_chroma: f32,
    /// Categories (names as in the pack map) the colored-darks rule does not apply to.
    pub colored_darks_exempt: Vec<String>,
    pub mud_relative: Vec<String>,
    pub mud_relative_margin: f32,
    pub mud_relative_hue: f32,
    pub mud_relative_max_dc: f32,
    pub mud_relative_max_dh: f32,
    pub mud_l: f32,
    pub mud_hue: [f32; 2],
    pub mud_max_chroma: f32,
    pub retention_min_source_chroma: f32,
    pub retention_ratio: f32,
    pub retention_floor: f32,
    pub max_hue_shift: f32,
    pub dark_max_hue_shift: f32,
}

impl PaletteRules {
    /// The least chroma a source of chroma `c` may come out with.
    pub fn retained(&self, c: f32) -> f32 {
        (self.retention_ratio * c).min(self.retention_floor)
    }

    /// True for a dull brownish dark ("mud").
    /// Whether the colored-darks rule applies to this category.
    pub fn darks_colored(&self, cat: pastelplash::config::Category) -> bool {
        let name = format!("{cat:?}").to_lowercase();
        !self.colored_darks_exempt.contains(&name)
    }

    /// Whether mapping `src` to `out` (OKLCH) introduces brown mud for this category. For the
    /// `mud_relative` categories (actors) an output in the mud band is allowed when the source
    /// was already there (widened band) or the output is the source's own color.
    pub fn introduces_mud(
        &self,
        cat: pastelplash::config::Category,
        src: [f32; 3],
        out: [f32; 3],
    ) -> bool {
        if !self.is_mud(out) {
            return false;
        }
        let name = format!("{cat:?}").to_lowercase();
        if !self.mud_relative.contains(&name) {
            return true;
        }
        let [l, c, h] = src;
        let m = self.mud_relative_margin;
        let [h0, h1] = self.mud_hue;
        let widened = l < self.mud_l + m
            && c >= 0.012 - m
            && c < self.mud_max_chroma + m
            && in_hue_range(h, [h0 - self.mud_relative_hue, h1 + self.mud_relative_hue]);
        let own = (out[1] - c).abs() <= self.mud_relative_max_dc
            && pastelplash::color::hue_diff(h, out[2]).abs() <= self.mud_relative_max_dh;
        !(widened || own)
    }

    pub fn is_mud(&self, [l, c, h]: [f32; 3]) -> bool {
        l < self.mud_l && c >= 0.012 && c < self.mud_max_chroma && in_hue_range(h, self.mud_hue)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccentRules {
    pub max_fraction: f32,
    pub hue: [f32; 2],
    pub min_l: f32,
    pub max_chroma: f32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VividRules {
    pub max_amount: f32,
    pub max_chroma: f32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TechniqueRules {
    pub max_kuwahara_radius: f32,
    pub max_edge_darkening: f32,
    pub max_granulation: f32,
    pub max_paper_grain: f32,
    pub max_paper_tint: f32,
    pub max_stroke_strength: f32,
    pub max_smear: f32,
    pub max_temperature_chroma: f32,
    pub max_edge_width: f32,
    pub value_min_effect: f32,
    pub value_mean_tolerance: f32,
    pub adaptive_min_effect: f32,
    pub small_object_min_contrast: f32,
    pub text_min_contrast: f32,
    pub split_max_hue_change: f32,
    pub split_min_separation: f32,
    pub speck_max_contrast: f32,
    pub ink_max_darkening: f32,
    pub thin_min_contrast: f32,
    pub thin_min_value_contrast: f32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetRules {
    pub max_actor_ceiling: f32,
    pub max_actor_warm_cool: f32,
    pub max_actor_shadow_tint: f32,
    pub max_actor_cast: f32,
    pub max_actor_accent: f32,
    pub max_actor_delight: f32,
    pub max_actor_lift: f32,
    pub max_actor_hue: f32,
    pub max_background_grouping: f32,
    pub max_background_accent: f32,
    pub max_background_warm_cool: f32,
    pub max_background_palette: f32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FluidRules {
    pub max_wet_edges: f32,
    pub max_granulation: f32,
    pub max_value_contrast: f32,
    pub max_abstraction: f32,
    pub max_accent: f32,
    pub max_radius_scale: f32,
    pub highlight_min_contrast: f32,
    pub depth_max_grit: f32,
    pub outline_drop: f32,
    pub max_outline_share: f32,
    pub max_mean_l: f32,
    pub body_min_lean: f32,
    pub highlight_max_drop: f32,
    pub tint_safe_max_darkening: f32,
    pub lava_max_darkening: f32,
    pub lava_min_chroma_retention: f32,
    pub lava_max_hue_shift: f32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActorRules {
    pub retention_ratio: f32,
    pub max_lightness_shift: f32,
    pub max_patch_l: f32,
    pub max_brushwork: f32,
    pub min_mark_energy: f32,
    pub max_local_hue_change: f32,
    pub cool_dark_hue: [f32; 2],
    pub cool_dark_family: [f32; 2],
    pub max_cool_dark_hue_shift: f32,
    pub max_tint_safe_gray: f32,
    pub max_tint_safe_l: f32,
    pub tint_safe_min_structure: f32,
    pub max_tint_safe_strokes: f32,
}

/// The contract, parsed once.
pub fn contract() -> &'static Contract {
    static C: OnceLock<Contract> = OnceLock::new();
    C.get_or_init(|| {
        let text = std::fs::read_to_string(repo().join("rules.toml")).expect("rules.toml");
        toml::from_str(&text).expect("rules.toml parses")
    })
}

/// True if `h` lies in the (possibly wrapping) hue range.
pub fn in_hue_range(h: f32, [from, to]: [f32; 2]) -> bool {
    (h - from).rem_euclid(360.0) <= (to - from).rem_euclid(360.0)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolutionRules {
    pub min_floor: u32,
    pub max_factor: u32,
    pub max_blockiness: f32,
    pub max_coarse_delta_e: f32,
}
