//! The pack-map layer: which file is what (pure data: path globs to categories, moods, marks).

use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::Category;
use crate::mood::Mood;

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
    /// Brushwork rules by path glob, first match wins: multiply the stroke strength of matching
    /// files (props like chests and pots take more visible strokes than faces and skin).
    pub brushwork: Vec<BrushworkRule>,
    /// Path globs of files never value-grouped (signs, lettering, symbols the detector misses).
    pub no_grouping: Vec<String>,
    /// Path globs of files that never get the large-scale abstraction pass (signs whose thin
    /// painted borders and lettering must stay).
    pub no_abstraction: Vec<String>,
    /// Path globs of files that never turn terracotta (area opt-out).
    pub no_terracotta: Vec<String>,
    /// When not empty, only files matching one of these globs may turn terracotta (area opt-in).
    pub terracotta_only: Vec<String>,
    /// Detect fluids (water, lava) among world textures by their look (`crate::fluid`).
    pub detect_fluids: bool,
    /// Fluid rules by path glob, first match wins: confirm or override the detector, and give
    /// an area its own water reference tone.
    pub fluids: Vec<FluidRule>,
}

/// What a fluid rule says a file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FluidRuleKind {
    Water,
    Lava,
    Liquid,
    /// Never a fluid, whatever the detector says.
    None,
}

/// Confirms or overrides fluid detection for matching files, and may override the water
/// reference tone of the style (`palette.water`) for them.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FluidRule {
    pub glob: String,
    /// The file's material; unset leaves it to the detector.
    pub kind: Option<FluidRuleKind>,
    /// Reference water hue (OKLCH degrees) for this area.
    pub water_hue: Option<f32>,
    /// Reference water chroma range for this area.
    pub water_chroma: Option<[f32; 2]>,
    /// Pull toward the reference for this area (0..1).
    pub water_pull: Option<f32>,
    /// Reference water body lightness for this area.
    pub water_lightness: Option<f32>,
}

/// Scales the paint-mark size (the painting Kuwahara radius) of matching files, e.g. for
/// textures that tile many times, whose texture-space marks shrink in world space.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarksRule {
    pub glob: String,
    pub scale: f32,
}

/// Multiplies the brushstroke strength of matching files (at most [`MAX_BRUSHWORK`]).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrushworkRule {
    pub glob: String,
    pub strength: f32,
}

/// The largest brushwork multiplier a pack map may set.
pub const MAX_BRUSHWORK: f32 = 4.0;

/// Assigns a mood (and optionally allows or denies dark greens) to matching files.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoodRule {
    pub glob: String,
    pub mood: String,
    #[serde(default = "one")]
    pub strength: f32,
    pub dark_greens: Option<bool>,
    /// The area's cast (a name from [`crate::mood::CASTS`], e.g. `"midnight-purple"`),
    /// overriding the mood's cast hue.
    pub cast: Option<String>,
    /// The area's full-strength cast strength, overriding the mood's.
    pub cast_strength: Option<f32>,
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
            brushwork: Vec::new(),
            no_grouping: Vec::new(),
            no_abstraction: Vec::new(),
            no_terracotta: Vec::new(),
            terracotta_only: Vec::new(),
            detect_fluids: true,
            fluids: Vec::new(),
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

    /// Brushwork multiplier by the first matching brushwork rule, else 1.
    pub fn brushwork_for(&self, path: &Path) -> f32 {
        let p = path.to_string_lossy().replace('\\', "/");
        self.brushwork
            .iter()
            .find(|r| glob_match(&r.glob, &p))
            .map_or(1.0, |r| r.strength)
    }

    /// False if a `no_grouping` glob matches the file.
    pub fn grouping_allowed(&self, path: &Path) -> bool {
        let p = path.to_string_lossy().replace('\\', "/");
        !self.no_grouping.iter().any(|g| glob_match(g, &p))
    }

    /// Whether the area opt-in and opt-out lists let the file turn terracotta.
    pub fn terracotta_allowed(&self, path: &Path) -> bool {
        let p = path.to_string_lossy().replace('\\', "/");
        !self.no_terracotta.iter().any(|g| glob_match(g, &p))
            && (self.terracotta_only.is_empty()
                || self.terracotta_only.iter().any(|g| glob_match(g, &p)))
    }

    /// False if a `no_abstraction` glob matches the file.
    pub fn abstraction_allowed(&self, path: &Path) -> bool {
        let p = path.to_string_lossy().replace('\\', "/");
        !self.no_abstraction.iter().any(|g| glob_match(g, &p))
    }

    /// Mood by the first matching mood rule, else the base mood.
    pub fn mood_for(&self, path: &Path) -> Mood {
        let p = path.to_string_lossy().replace('\\', "/");
        self.moods
            .iter()
            .find(|r| glob_match(&r.glob, &p))
            .map_or_else(Mood::default, |r| Mood {
                dark_greens: r.dark_greens,
                cast_hue: r.cast.as_deref().and_then(crate::mood::cast_hue),
                cast_strength: r.cast_strength,
                ..Mood::new(&r.mood, r.strength)
            })
    }

    /// The first fluid rule matching the file.
    pub fn fluid_rule_for(&self, path: &Path) -> Option<&FluidRule> {
        let p = path.to_string_lossy().replace('\\', "/");
        self.fluids.iter().find(|r| glob_match(&r.glob, &p))
    }

    /// Rejects unknown cast names and out-of-range strengths.
    pub fn validate(&self) -> anyhow::Result<()> {
        for r in &self.brushwork {
            anyhow::ensure!(
                (0.0..=MAX_BRUSHWORK).contains(&r.strength),
                "brushwork {:?}: strength = {} must be within 0..={MAX_BRUSHWORK}",
                r.glob,
                r.strength
            );
        }
        for r in &self.fluids {
            if let Some(p) = r.water_pull {
                anyhow::ensure!(
                    (0.0..=1.0).contains(&p),
                    "fluids {:?}: water_pull = {p} must be within 0..=1",
                    r.glob
                );
            }
            if let Some([lo, hi]) = r.water_chroma {
                anyhow::ensure!(
                    0.0 <= lo && lo <= hi,
                    "fluids {:?}: water_chroma = [{lo}, {hi}] must be increasing and >= 0",
                    r.glob
                );
            }
        }
        for r in &self.moods {
            if let Some(c) = &r.cast {
                anyhow::ensure!(
                    crate::mood::cast_hue(c).is_some(),
                    "moods {:?}: unknown cast {c:?} (known: {})",
                    r.glob,
                    crate::mood::CASTS
                        .iter()
                        .map(|(n, _)| *n)
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            for (what, v) in [
                ("strength", Some(r.strength)),
                ("cast_strength", r.cast_strength),
            ] {
                if let Some(v) = v {
                    anyhow::ensure!(
                        (0.0..=1.0).contains(&v),
                        "moods {:?}: {what} = {v} must be within 0..=1",
                        r.glob
                    );
                }
            }
        }
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn pack_casts_resolve_by_name_and_unknown_names_are_rejected() {
        let pack: Pack = toml::from_str(
            "[[moods]]\nglob = 'a/**'\nmood = 'nocturne'\ncast = 'midnight-purple'\n\
             cast_strength = 0.8",
        )
        .unwrap();
        pack.validate().unwrap();
        let m = pack.mood_for(Path::new("a/x"));
        assert_eq!(
            (m.cast_hue, m.cast_strength),
            (crate::mood::cast_hue("midnight-purple"), Some(0.8))
        );
        let bad: Pack =
            toml::from_str("[[moods]]\nglob = 'a/**'\nmood = 'nocturne'\ncast = 'mauve'").unwrap();
        let err = bad.validate().unwrap_err().to_string();
        assert!(err.contains("mauve") && err.contains("indigo"), "{err}");
    }

    #[test]
    fn fluid_rules_first_match_wins_and_are_validated() {
        let pack: Pack = toml::from_str(
            "[[fluids]]\nglob = 'a/pool*'\nkind = 'none'\n\
             [[fluids]]\nglob = 'a/**'\nkind = 'water'\nwater_hue = 300.0\nwater_pull = 0.3",
        )
        .unwrap();
        pack.validate().unwrap();
        assert!(pack.detect_fluids);
        let r = pack.fluid_rule_for(Path::new("a/pool1")).unwrap();
        assert_eq!(r.kind, Some(FluidRuleKind::None));
        let r = pack.fluid_rule_for(Path::new("a/b/c")).unwrap();
        assert_eq!(
            (r.kind, r.water_hue),
            (Some(FluidRuleKind::Water), Some(300.0))
        );
        assert!(pack.fluid_rule_for(Path::new("b/c")).is_none());
        let bad: Pack =
            toml::from_str("[[fluids]]\nglob = 'a/**'\nwater_chroma = [0.06, 0.02]").unwrap();
        assert!(bad.validate().is_err());
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
}
