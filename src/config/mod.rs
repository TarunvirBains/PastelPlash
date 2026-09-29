//! The three configuration layers (see `PLAN.md`): style, target and pack map.
//!
//! Every field has a default, so each file may be partial or absent. Defaults are neutral: an
//! empty style and target leave textures unchanged. Relative paths inside a file are resolved
//! against that file's folder.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;

pub use crate::mood::Mood;

mod builtin;
mod category;
pub mod layers;
mod pack;
mod style;
mod target;

pub use builtin::DEFAULT_STYLE;
pub use category::Category;
pub use pack::{
    BrushworkRule, FluidRule, FluidRuleKind, MarksRule, MoodRule, Pack, Rule, glob_match,
};
pub use style::{
    Abstraction, Cast, Contrast, Delight, Grouping, HueGroup, Kuwahara, Marks, Palette, Scale,
    Strokes, Style, Temperature, Tiling, Tint, ValueContrast, Warmth, WaterTone, Watercolor,
};
pub use target::{Exposure, Target, Treatment};

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
                // A bare name that is not a file is a built-in style (`--style impressionist`).
                Some(path)
                    if !path.exists()
                        && path.components().count() == 1
                        && path.extension().is_none() =>
                {
                    Style::builtin(&path.to_string_lossy())?
                }
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
                    let mood = Mood::new(name, strength);
                    config.style.for_mood(&mood).with_context(invalid)?;
                }
            }
        }
        if let Some(path) = pack {
            config
                .pack
                .validate()
                .with_context(|| format!("invalid pack map {}", path.display()))?;
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
    fn moods_derive_from_the_base_style() {
        let style = Style::parse(
            "name = 's'\n[palette]\nenabled = true\nl_floor = 0.6\nl_ceiling = 0.95\n\
             [moods.nocturne.palette]\nl_floor = 0.3\nl_ceiling = 0.85",
        )
        .unwrap();
        let mood = |name: &str, strength| Mood::new(name, strength);
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
    fn held_mood_keys_apply_at_any_strength_and_casts_override_per_texture() {
        let style = Style::parse(
            "name = 's'\n[palette]\nenabled = true\nl_floor = 0.2\n\
             warmth = { strength = 0.4 }\ncast = { strength = 0.0, hue = 275.0 }\n\
             [moods.nocturne]\nhold = ['palette.warmth.strength']\n\
             [moods.nocturne.palette]\nl_floor = 0.1\nwarmth = { strength = 0.0 }\n\
             cast = { strength = 1.0 }",
        )
        .unwrap();
        let half = style.for_mood(&Mood::new("nocturne", 0.5)).unwrap();
        assert_eq!(
            half.palette.warmth.strength, 0.0,
            "held: off at half strength"
        );
        assert!(
            (half.palette.l_floor - 0.15).abs() < 1e-6,
            "not held: blended"
        );
        assert!((half.palette.cast.strength - 0.5).abs() < 1e-6);
        let over = Mood {
            cast_hue: Some(305.0),
            cast_strength: Some(0.5),
            ..Mood::new("nocturne", 0.5)
        };
        let m = style.for_mood(&over).unwrap();
        assert_eq!(m.palette.cast.hue, 305.0, "the area's hue, not blended");
        assert!(
            (m.palette.cast.strength - 0.25).abs() < 1e-6,
            "the area's strength, blended"
        );
        let bad = Style::parse(
            "[moods.x]\nhold = ['palette.l_floor']\n[moods.x.palette]\nenabled = true",
        )
        .unwrap();
        assert!(
            bad.for_mood(&Mood::new("x", 1.0)).is_err(),
            "held key the mood doesn't set"
        );
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
    fn extends_list_layers_overlays_in_order() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("base.toml"),
            "name = 'base'\n[palette]\nl_floor = 0.3\n[strokes]\nstrength = 0.01\nwidth = 4.0",
        )
        .unwrap();
        fs::create_dir(dir.path().join("overlays")).unwrap();
        fs::write(
            dir.path().join("overlays/brush.toml"),
            "[strokes]\nstrength = 0.02\n[kuwahara]\nradius = 10.0",
        )
        .unwrap();
        let child = dir.path().join("child.toml");
        fs::write(
            &child,
            "extends = ['base.toml', 'overlays/brush.toml']\nname = 'child'\n[kuwahara]\nradius = 12.0",
        )
        .unwrap();
        let s = Config::load(Some(&child), None, None).unwrap().style;
        assert_eq!(s.name, "child");
        assert_eq!(s.palette.l_floor, 0.3);
        assert_eq!((s.strokes.strength, s.strokes.width), (0.02, 4.0));
        assert_eq!(s.kuwahara.radius, 12.0);
        fs::write(&child, "extends = []\n").unwrap();
        assert!(Config::load(Some(&child), None, None).is_err());
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
    fn typos_are_rejected() {
        assert!(toml::from_str::<Style>("[kuwahara]\nradios = 3").is_err());
        assert!(toml::from_str::<Pack>("[[rules]]\nglob = 'a'\ncategory = 'actors'").is_err());
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
