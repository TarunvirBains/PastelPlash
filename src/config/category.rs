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
            _ => {
                return Err(format!(
                    "unknown category {s:?} (actor, world, skybox, background, ui, effect, skip)"
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
    /// actors, which the cel shader bands, nor UI).
    pub fn may_group(self) -> bool {
        matches!(self, Self::World | Self::Background)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_parse_from_cli_strings() {
        assert_eq!("Actor".parse::<Category>(), Ok(Category::Actor));
        assert!("actors".parse::<Category>().is_err());
    }

    #[test]
    fn category_policy() {
        use Category::*;
        let all = [Actor, World, Skybox, Background, Ui, Effect, Skip];
        let pick = |f: fn(Category) -> bool| all.into_iter().filter(|&c| f(c)).collect::<Vec<_>>();
        assert_eq!(
            pick(Category::is_stylized),
            [Actor, World, Skybox, Background]
        );
        assert_eq!(pick(Category::may_group), [World, Background]);
        assert!(!Background.may_tile() && World.may_tile() && Skybox.may_tile());
    }
}
