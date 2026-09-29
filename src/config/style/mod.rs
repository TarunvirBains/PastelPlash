//! The style layer: what it should look like. One module per section.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use super::{builtin, layers};

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
mod terracotta;
mod tiling;
mod value;
mod watercolor;

pub use abstraction::Abstraction;
pub use delight::Delight;
pub use grouping::Grouping;
pub use kuwahara::Kuwahara;
pub use marks::Marks;
pub use palette::{Cast, HueGroup, Palette, Tint, Warmth, WaterTone};
pub use scale::Scale;
pub use strokes::Strokes;
pub use temperature::Temperature;
pub use terracotta::Terracotta;
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
    pub terracotta: Terracotta,
    /// Named moods: partial overrides of this style (see `src/mood.rs`). The style itself is
    /// the `base` mood.
    pub moods: BTreeMap<String, toml::Table>,
    /// The file's TOML, kept to derive moods from.
    #[serde(skip)]
    pub raw: Option<toml::Table>,
}

/// The value at a dotted key path of a table.
fn get_path<'a>(t: &'a toml::Table, path: &str) -> Option<&'a toml::Value> {
    let mut parts = path.split('.');
    let mut v = t.get(parts.next()?)?;
    for p in parts {
        v = v.as_table()?.get(p)?;
    }
    Some(v)
}

/// Sets the value at a dotted key path, creating tables on the way.
fn set_path(t: &mut toml::Table, path: &str, value: toml::Value) {
    let parts: Vec<&str> = path.split('.').collect();
    let mut cur = t;
    for p in &parts[..parts.len() - 1] {
        let next = cur
            .entry(p.to_string())
            .or_insert_with(|| toml::Value::Table(toml::Table::new()));
        if !next.is_table() {
            *next = toml::Value::Table(toml::Table::new());
        }
        cur = next.as_table_mut().unwrap();
    }
    cur.insert(parts[parts.len() - 1].to_string(), value);
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

    /// A stack named `a+b+…` (`--style ss-terracotta+impressionist`): the first part is a style
    /// (a file, or a built-in name), each later part a layer merged over it at full strength: a
    /// file, an overlay in `styles/overlays/` by name (`impressionist` names the
    /// `impressionist-brushwork` overlay), or a built-in style. A palette layer times a
    /// brushwork layer without a file for every combination.
    pub fn stacked(spec: &str) -> Result<Self> {
        let parts: Vec<&str> = spec.split('+').map(str::trim).collect();
        anyhow::ensure!(
            parts.len() >= 2 && parts.iter().all(|p| !p.is_empty()),
            "style stack {spec:?}: name two or more styles or layers joined by `+`"
        );
        let mut stack = layers::StyleStack::new();
        for (i, part) in parts.iter().enumerate() {
            let path = Path::new(part);
            let table = if path.is_file() {
                layers::load(path, &|p: &Path| {
                    fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))
                })?
            } else {
                let candidates: Vec<String> = if i == 0 {
                    vec![format!("{part}.toml")]
                } else {
                    vec![
                        format!("overlays/{part}.toml"),
                        format!("overlays/{part}-brushwork.toml"),
                        format!("{part}.toml"),
                    ]
                };
                let found = candidates
                    .iter()
                    .find(|c| builtin::read(Path::new(c)).is_ok())
                    .with_context(|| {
                        format!(
                            "style stack {spec:?}: {part:?} is no file, built-in style or overlay"
                        )
                    })?;
                layers::load(Path::new(found), &builtin::read)?
            };
            stack.push(table);
        }
        let mut raw = stack
            .resolve()
            .with_context(|| format!("merging the style stack {spec:?}"))?;
        raw.insert("name".into(), toml::Value::String(spec.to_string()));
        Self::from_table(raw).with_context(|| format!("parsing the style stack {spec:?}"))
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
            let mut over = over.clone();
            // `hold`: keys the mood sets at its full value whatever its strength (e.g. earth
            // warmth fully off in a nocturne), not blended.
            let hold: Vec<String> = match over.remove("hold") {
                Some(toml::Value::Array(a)) => a
                    .iter()
                    .map(|v| v.as_str().map(String::from))
                    .collect::<Option<_>>()
                    .with_context(|| format!("mood {:?}: `hold` lists key paths", mood.name))?,
                Some(_) => {
                    anyhow::bail!("mood {:?}: `hold` must be a list of key paths", mood.name)
                }
                None => Vec::new(),
            };
            if let Some(s) = mood.cast_strength {
                set_path(
                    &mut over,
                    "palette.cast.strength",
                    toml::Value::Float(s as f64),
                );
            }
            let mut table = layers::merge(raw, &over, mood.strength.min(1.0) as f64)
                .with_context(|| format!("mood {:?}", mood.name))?;
            for path in &hold {
                let v = get_path(&over, path).with_context(|| {
                    format!(
                        "mood {:?}: held key {path:?} is not set by the mood",
                        mood.name
                    )
                })?;
                set_path(&mut table, path, v.clone());
            }
            if let Some(h) = mood.cast_hue {
                set_path(&mut table, "palette.cast.hue", toml::Value::Float(h as f64));
            }
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
        // An area's own reference water tone (a pack-map fluid rule).
        let water = &mut style.palette.water;
        water.hue = mood.water_hue.unwrap_or(water.hue);
        water.chroma = mood.water_chroma.unwrap_or(water.chroma);
        water.pull = mood.water_pull.unwrap_or(water.pull);
        water.lightness = mood.water_lightness.unwrap_or(water.lightness);
        style.validate().with_context(|| format!("mood {mood}"))?;
        Ok(style)
    }

    /// Rejects settings that are out of range or contradict each other.
    pub fn validate(&self) -> Result<()> {
        // Section by section, in a fixed order (the first failure is reported).
        self.palette.validate()?;
        self.kuwahara.validate()?;
        self.scale.validate()?;
        self.watercolor.validate()?;
        self.strokes.validate()?;
        self.temperature.validate()?;
        self.delight.validate()?;
        self.terracotta.validate()?;
        self.grouping.validate()
    }
}

#[cfg(test)]
mod stack_tests {
    use super::*;

    fn same(a: &Style, b: &Style) {
        let strip = |s: &Style| {
            let mut s = s.clone();
            s.name.clear();
            if let Some(r) = s.raw.as_mut() {
                r.remove("name");
            }
            s
        };
        assert_eq!(strip(a), strip(b));
    }

    #[test]
    fn a_stack_equals_the_style_file_of_the_same_layers() {
        same(
            &Style::stacked("watercolor+impressionist").unwrap(),
            &Style::builtin("impressionist").unwrap(),
        );
        same(
            &Style::stacked("ss-baseline+impressionist").unwrap(),
            &Style::builtin("ss-impressionist").unwrap(),
        );
        same(
            &Style::stacked("ss-terracotta+impressionist").unwrap(),
            &Style::builtin("ss-terracotta-impressionist").unwrap(),
        );
        same(
            &Style::stacked("ss-baseline+terracotta").unwrap(),
            &Style::builtin("ss-terracotta").unwrap(),
        );
        assert_eq!(
            Style::stacked("ss-baseline+terracotta").unwrap().name,
            "ss-baseline+terracotta"
        );
        assert!(Style::stacked("ss-baseline+nope").is_err());
        assert!(Style::stacked("ss-baseline+").is_err());
    }
}
