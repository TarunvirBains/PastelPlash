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
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetRules {
    pub max_actor_ceiling: f32,
    pub max_actor_warm_cool: f32,
    pub max_actor_shadow_tint: f32,
    pub max_actor_cast: f32,
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
pub struct ActorRules {
    pub retention_ratio: f32,
    pub max_lightness_shift: f32,
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
