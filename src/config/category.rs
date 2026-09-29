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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_parse_from_cli_strings() {
        assert_eq!("Actor".parse::<Category>(), Ok(Category::Actor));
        assert!("actors".parse::<Category>().is_err());
    }
}
