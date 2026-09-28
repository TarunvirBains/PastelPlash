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
//! value blended toward the mood's value: numbers (and number arrays of equal length, such as
//! tone curves) are interpolated, tables and arrays of tables are blended entry by entry, and
//! anything else switches at `s = 0.5`. Blending in parameter space keeps a partial mood a valid
//! style (a blend of two monotone tone curves on the same inputs is monotone).

use std::fmt;
use std::str::FromStr;

use serde::Deserialize;
use toml::{Table, Value};

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

fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Integer(i) => Some(*i as f64),
        Value::Float(f) => Some(*f),
        _ => None,
    }
}

/// Blends `over` into `base` at strength `s` (see the module docs).
pub fn blend(base: &Value, over: &Value, s: f64) -> Value {
    match (base, over) {
        (Value::Table(b), Value::Table(o)) => Value::Table(blend_table(b, o, s)),
        (Value::Array(b), Value::Array(o)) if b.len() == o.len() => {
            Value::Array(b.iter().zip(o).map(|(b, o)| blend(b, o, s)).collect())
        }
        (Value::Integer(b), Value::Integer(o)) => {
            Value::Integer((*b as f64 + s * (*o - *b) as f64).round() as i64)
        }
        _ => match (number(base), number(over)) {
            (Some(b), Some(o)) => Value::Float(b + s * (o - b)),
            _ if s >= 0.5 => over.clone(),
            _ => base.clone(),
        },
    }
}

/// Table version of [`blend`]. Keys only in `over` appear from `s = 0.5` on (there is no base
/// value to interpolate from, so styles should set every key a mood overrides).
pub fn blend_table(base: &Table, over: &Table, s: f64) -> Table {
    let mut out = base.clone();
    for (k, o) in over {
        match base.get(k) {
            Some(b) => {
                out.insert(k.clone(), blend(b, o, s));
            }
            None if s >= 0.5 => {
                out.insert(k.clone(), o.clone());
            }
            None => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> Table {
        toml::from_str(s).unwrap()
    }

    #[test]
    fn numbers_and_curves_interpolate() {
        let base = t("a = 1.0\nn = 10\ncurve = [[0, 0.5], [1, 1.0]]\n[x]\nb = 0.0\nc = 'keep'");
        let over = t("a = 0.0\nn = 20\ncurve = [[0, 0.3], [1, 0.8]]\n[x]\nb = 1.0");
        let half = blend_table(&base, &over, 0.5);
        assert_eq!(half["a"].as_float(), Some(0.5));
        assert_eq!(half["n"].as_integer(), Some(15));
        assert_eq!(
            half["curve"].as_array().unwrap()[0].as_array().unwrap()[1].as_float(),
            Some(0.4)
        );
        assert_eq!(half["x"]["b"].as_float(), Some(0.5));
        assert_eq!(half["x"]["c"].as_str(), Some("keep"));
        assert_eq!(blend_table(&base, &over, 0.0), base);
        let full = blend_table(&base, &over, 1.0);
        assert_eq!(full["a"].as_float(), Some(0.0));
    }

    #[test]
    fn non_numbers_switch_at_half() {
        let base = t("s = 'a'\nlist = [1, 2]");
        let over = t("s = 'b'\nlist = [1, 2, 3]");
        assert_eq!(blend_table(&base, &over, 0.4)["s"].as_str(), Some("a"));
        assert_eq!(blend_table(&base, &over, 0.6)["s"].as_str(), Some("b"));
        assert_eq!(
            blend_table(&base, &over, 0.6)["list"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
    }

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
