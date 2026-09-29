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

/// Which mood a texture gets, and how strongly.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Mood {
    pub name: String,
    /// 0 = the base look, 1 = the full mood.
    pub strength: f32,
    /// Overrides the mood's `palette.dark_greens` (allow or deny dark greens for this texture).
    pub dark_greens: Option<bool>,
}

impl Default for Mood {
    fn default() -> Self {
        Self {
            name: BASE.into(),
            strength: 1.0,
            dark_greens: None,
        }
    }
}

impl Mood {
    /// True if this is the base look with nothing overridden.
    pub fn is_base(&self) -> bool {
        (self.name == BASE || self.strength <= 0.0) && self.dark_greens.is_none()
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
            Some(true) => write!(f, "+dark-greens"),
            Some(false) => write!(f, "-dark-greens"),
            None => Ok(()),
        }
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
        Ok(Self {
            name: name.into(),
            strength,
            dark_greens: None,
        })
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
