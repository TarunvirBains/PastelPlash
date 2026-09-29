//! The style layer: what it should look like. One module per section.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use super::{builtin, layers};
use super::{check, non_negative, unit};
use crate::mood::Mood;

mod abstraction;
mod delight;
mod grouping;
mod kuwahara;
mod marks;
mod palette;
mod scale;
mod strokes;
mod temperature;
mod tiling;
mod value;
mod watercolor;

pub use abstraction::Abstraction;
pub use delight::Delight;
pub use grouping::Grouping;
pub use kuwahara::Kuwahara;
pub use marks::Marks;
pub use palette::{HueGroup, Palette, Tint, Warmth};
pub use scale::Scale;
pub use strokes::Strokes;
pub use temperature::Temperature;
pub use tiling::Tiling;
pub use value::{Contrast, ValueContrast};
pub use watercolor::Watercolor;

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
    pub grouping: Grouping,
    pub marks: Marks,
    pub watercolor: Watercolor,
    /// Named moods: partial overrides of this style (see `src/mood.rs`). The style itself is
    /// the `base` mood.
    pub moods: BTreeMap<String, toml::Table>,
    /// The file's TOML, kept to derive moods from.
    #[serde(skip)]
    pub raw: Option<toml::Table>,
}

impl Style {
    /// Loads a style file, following `extends` (a path or a list of paths relative to the file):
    /// the file's settings are merged over the layers it extends (see [`super::layers`]).
    pub fn load(path: &Path) -> Result<Self> {
        let raw = layers::load(path, &|p: &Path| {
            fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))
        })?;
        Self::from_table(raw).with_context(|| format!("parsing {}", path.display()))
    }

    /// Names of the built-in styles (`styles/*.toml` shipped in the binary).
    pub fn builtin_names() -> Vec<&'static str> {
        builtin::names()
    }

    /// A built-in style by name (e.g. `impressionist`), with its `extends` chain resolved from
    /// the built-in files.
    pub fn builtin(name: &str) -> Result<Self> {
        let path = PathBuf::from(format!("{name}.toml"));
        let raw = layers::load(&path, &builtin::read)?;
        Self::from_table(raw).with_context(|| format!("parsing built-in style {name}"))
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
            let mut table = layers::merge(raw, over, mood.strength.min(1.0) as f64);
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
        let g = &self.grouping;
        unit("grouping.strength", g.strength)?;
        unit("grouping.color", g.color)?;
        unit("grouping.explained", g.explained)?;
        unit("grouping.skip_explained", g.skip_explained)?;
        check((2..=4).contains(&g.max_masses), || {
            format!(
                "grouping.max_masses = {} must be within 2..=4",
                g.max_masses
            )
        })?;
        check(g.softness > 0.0 && g.range > 0.0 && g.radius >= 0.0, || {
            "grouping.softness and grouping.range must be > 0, grouping.radius >= 0".into()
        })?;
        check(g.salient[0] < g.salient[1], || {
            "grouping.salient must be an increasing range".into()
        })?;
        Ok(())
    }
}
