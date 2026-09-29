//! Moods: named variations of a style (e.g. `nocturne` for dark, haunting areas), selected per
//! texture by the pack map and blended with the base look by a strength.
//!
//! A style file defines moods as partial overrides of itself:
//!
//! ```toml
//! [moods.nocturne.palette]
//! l_floor = 0.36
//! ```
//!
//! The style as written is the `base` mood. A mood at strength `s` is the base with every overridden
//! value blended toward the mood's value (`config::layers::merge`).

use std::fmt;
use std::str::FromStr;

use serde::Deserialize;

/// The base mood: the style as written.
pub const BASE: &str = "base";

/// Named cast hues (OKLCH degrees) a pack map may give an area's mood (`cast = "indigo"`).
pub const CASTS: &[(&str, f32)] = &[
    ("blue-teal", 225.0),
    ("midnight-blue", 250.0),
    ("indigo", 275.0),
    ("midnight-purple", 305.0),
];

/// The hue of a named cast.
pub fn cast_hue(name: &str) -> Option<f32> {
    CASTS.iter().find(|(n, _)| *n == name).map(|(_, h)| *h)
}

/// Which mood a texture gets, and how strongly.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Mood {
    pub name: String,
    /// 0 = the base look, 1 = the full mood.
    pub strength: f32,
    /// Overrides the mood's `palette.dark_greens` (allow or deny dark greens for this texture).
    pub dark_greens: Option<bool>,
    /// Overrides the mood's cast hue (`palette.cast.hue`, OKLCH degrees) for this texture.
    pub cast_hue: Option<f32>,
    /// Overrides the mood's full-strength cast strength (`palette.cast.strength`, then blended
    /// by the mood's strength like any mood value).
    pub cast_strength: Option<f32>,
    /// Overrides the style's reference water tone for this texture (`palette.water`: hue,
    /// chroma range, pull), e.g. an area's own water color from a pack-map fluid rule.
    pub water_hue: Option<f32>,
    pub water_chroma: Option<[f32; 2]>,
    pub water_pull: Option<f32>,
    pub water_lightness: Option<f32>,
}

impl Default for Mood {
    fn default() -> Self {
        Self {
            name: BASE.into(),
            strength: 1.0,
            dark_greens: None,
            cast_hue: None,
            cast_strength: None,
            water_hue: None,
            water_chroma: None,
            water_pull: None,
            water_lightness: None,
        }
    }
}

impl Mood {
    /// A plain mood at a strength (no per-texture overrides).
    pub fn new(name: &str, strength: f32) -> Self {
        Self {
            name: name.into(),
            strength,
            ..Self::default()
        }
    }

    /// True if this is the base look with nothing overridden.
    pub fn is_base(&self) -> bool {
        (self.name == BASE || self.strength <= 0.0)
            && self.dark_greens.is_none()
            && self.water_hue.is_none()
            && self.water_chroma.is_none()
            && self.water_pull.is_none()
            && self.water_lightness.is_none()
    }

    /// A stable key for caches.
    pub fn key(&self) -> String {
        format!("{self}")
    }
}

impl fmt::Display for Mood {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)?;
        if self.strength < 1.0 {
            write!(f, ":{:.2}", self.strength)?;
        }
        match self.dark_greens {
            Some(true) => write!(f, "+dark-greens")?,
            Some(false) => write!(f, "-dark-greens")?,
            None => {}
        }
        if let Some(h) = self.cast_hue {
            write!(f, "+cast{h:.0}")?;
        }
        if let Some(s) = self.cast_strength {
            write!(f, "+cast@{s:.2}")?;
        }
        if let Some(h) = self.water_hue {
            write!(f, "+water{h:.0}")?;
        }
        if let Some([a, b]) = self.water_chroma {
            write!(f, "+waterC{a:.3}-{b:.3}")?;
        }
        if let Some(p) = self.water_pull {
            write!(f, "+water@{p:.2}")?;
        }
        if let Some(l) = self.water_lightness {
            write!(f, "+waterL{l:.3}")?;
        }
        Ok(())
    }
}

/// `name` or `name:strength`, e.g. `nocturne:0.6`.
impl FromStr for Mood {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        let (name, strength) = match s.split_once(':') {
            Some((n, v)) => (
                n,
                v.parse::<f32>()
                    .map_err(|_| format!("bad mood strength in {s:?}"))?,
            ),
            None => (s, 1.0),
        };
        if name.is_empty() || !(0.0..=1.0).contains(&strength) {
            return Err(format!("expected MOOD or MOOD:STRENGTH (0..=1), got {s:?}"));
        }
        Ok(Self::new(name, strength))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moods_parse_from_cli_strings() {
        let m: Mood = "nocturne:0.6".parse().unwrap();
        assert_eq!((m.name.as_str(), m.strength), ("nocturne", 0.6));
        assert_eq!("base".parse::<Mood>().unwrap(), Mood::default());
        assert!("nocturne:2".parse::<Mood>().is_err());
        assert!(Mood::default().is_base());
        assert_eq!(m.key(), "nocturne:0.60");
    }
}
