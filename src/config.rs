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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    /// Relit by the target's actor lighting.
    Actor,
    /// Static world geometry.
    World,
    Skybox,
    Ui,
    /// Copied through untouched.
    Skip,
}

/// What it should look like.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Style {
    pub name: String,
    /// 3D `.cube` palette LUT.
    pub lut: Option<PathBuf>,
    pub kuwahara: Kuwahara,
    pub watercolor: Watercolor,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Kuwahara {
    /// Filter radius in texels; 0 disables the filter.
    pub radius: u32,
    /// Blend between original (0) and filtered (1).
    pub strength: f32,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Watercolor {
    pub edge_darkening: f32,
    pub granulation: f32,
    pub paper_grain: f32,
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

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Treatment {
    /// How strongly to remove baked shading, 0..=1.
    pub delight: f32,
    /// Maximum OKLCH lightness, for textures the renderer brightens further.
    pub lightness_ceiling: Option<f32>,
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
}

impl Default for Pack {
    fn default() -> Self {
        Self {
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
            style: load_or_default(style)?,
            target: load_or_default(target)?,
            pack: load_or_default(pack)?,
        };
        if let Some(base) = style.and_then(Path::parent) {
            resolve(&mut config.style.lut, base);
        }
        if let Some(base) = pack.and_then(Path::parent) {
            resolve(&mut config.pack.list, base);
        }
        Ok(config)
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
        assert_eq!(style.kuwahara.radius, 6);
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
