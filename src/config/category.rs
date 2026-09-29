//! Texture categories: what kind of surface a file is, which decides its treatment.

use serde::Deserialize;

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
    /// Effects (glows, fire, sparkles, particles, shadow blobs): the gray is the effect's
    /// intensity and falloff, often drawn additively. Left untouched.
    Effect,
    /// Copied through untouched.
    Skip,
    /// Fluids (see `crate::fluid`): soft light over depth, never cut into outlined cells.
    /// Water: pulled gently toward the style's reference water tone.
    Water,
    /// Emissive molten rock: keeps its glow and heat colors in every mood.
    Lava,
    /// Other liquids (poison, swamp, organic fluids): fluid treatment, their own color.
    Liquid,
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
            "effect" => Self::Effect,
            "skip" => Self::Skip,
            "water" => Self::Water,
            "lava" => Self::Lava,
            "liquid" => Self::Liquid,
            _ => {
                return Err(format!(
                    "unknown category {s:?} (actor, world, skybox, background, ui, effect, skip, water, \
                     lava, liquid)"
                ));
            }
        })
    }
}

/// Category policy: the rules every driver and stage share, in one place.
impl Category {
    /// Whether the texture is restyled at all. UI (until it gets its own treatment), effects
    /// (their gray is light intensity, not paint) and skip are copied through untouched.
    pub fn is_stylized(self) -> bool {
        !matches!(self, Self::Ui | Self::Effect | Self::Skip)
    }

    /// Whether the texture may tile. Pre-rendered backgrounds are whole pictures: they never
    /// wrap, whatever their edges say.
    pub fn may_tile(self) -> bool {
        self != Self::Background
    }

    /// Whether soft value grouping may apply: world and background textures only (never
    /// actors, which the cel shader bands, nor fluids, whose caustics it cuts into cells, nor UI).
    pub fn may_group(self) -> bool {
        matches!(self, Self::World | Self::Background)
    }

    /// Whether this is a fluid material (water, lava, other liquids).
    pub fn is_fluid(self) -> bool {
        matches!(self, Self::Water | Self::Lava | Self::Liquid)
    }

    /// Emissive materials keep their own light: no mood (no moonlight cast, no night exposure)
    /// ever reaches them.
    pub fn is_emissive(self) -> bool {
        self == Self::Lava
    }

    /// Whether fluid detection may turn a texture of this category into a fluid (world geometry
    /// only; anything else needs a pack-map `[[fluids]]` rule).
    pub fn may_be_detected_fluid(self) -> bool {
        self == Self::World
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_parse_from_cli_strings() {
        assert_eq!("Actor".parse::<Category>(), Ok(Category::Actor));
        assert_eq!("lava".parse::<Category>(), Ok(Category::Lava));
        assert!("actors".parse::<Category>().is_err());
    }

    #[test]
    fn category_policy() {
        use Category::*;
        let all = [
            Actor, World, Skybox, Background, Ui, Effect, Skip, Water, Lava, Liquid,
        ];
        let pick = |f: fn(Category) -> bool| all.into_iter().filter(|&c| f(c)).collect::<Vec<_>>();
        assert_eq!(
            pick(Category::is_stylized),
            [Actor, World, Skybox, Background, Water, Lava, Liquid]
        );
        assert_eq!(pick(Category::may_group), [World, Background]);
        assert_eq!(pick(Category::is_fluid), [Water, Lava, Liquid]);
        assert_eq!(pick(Category::is_emissive), [Lava]);
        assert_eq!(pick(Category::may_be_detected_fluid), [World]);
        assert!(
            !Background.may_tile() && World.may_tile() && Skybox.may_tile() && Water.may_tile()
        );
    }
}
