//! Style layers and how they compose.
//!
//! A style is a stack of TOML layers: a palette base, overlays such as the impressionist
//! brushwork, then the file itself. `extends` in a style file names the layers below it (a path
//! or a list of paths, relative to the file), merged in order at full strength. Moods are
//! ordinary tables in that merge (`[moods.<name>]`; each mood is the union of what every layer
//! declares for it) and are blended over the resolved style at their strength.
//!
//! [`merge`] at strength `s` blends every value `over` sets toward it: numbers (and number
//! arrays of equal length, such as tone curves) are interpolated, tables and arrays of tables
//! are blended entry by entry, and anything else switches at `s = 0.5`. Keys only in `over`
//! appear from `s = 0.5` on. Blending in parameter space keeps a partial mood a valid style (a
//! blend of two monotone tone curves on the same inputs is monotone).

use std::path::Path;

use anyhow::{Context, Result};
use toml::{Table, Value};

fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Integer(i) => Some(*i as f64),
        Value::Float(f) => Some(*f),
        _ => None,
    }
}

/// Blends `over` into `base` at strength `s` (see the module docs).
fn blend(base: &Value, over: &Value, s: f64) -> Value {
    match (base, over) {
        (Value::Table(b), Value::Table(o)) => Value::Table(merge(b, o, s)),
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

/// Merges layer `over` into `base` at strength `s` (1 for a layer of a style, the mood's
/// strength for a mood). Keys only in `over` appear from `s = 0.5` on (there is no base value
/// to interpolate from, so styles should set every key a mood overrides).
pub fn merge(base: &Table, over: &Table, s: f64) -> Table {
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

/// An ordered stack of style layers: the first is the base, each later one an overlay merged
/// over it at full strength (e.g. a palette layer, then a brushwork layer, then a file's own
/// settings).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StyleStack {
    layers: Vec<Table>,
}

impl StyleStack {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a layer on top.
    pub fn push(&mut self, layer: Table) {
        self.layers.push(layer);
    }

    pub fn layers(&self) -> &[Table] {
        &self.layers
    }

    /// The merged table (empty for an empty stack).
    pub fn resolve(&self) -> Table {
        let mut layers = self.layers.iter();
        let Some(first) = layers.next() else {
            return Table::new();
        };
        layers.fold(first.clone(), |acc, layer| merge(&acc, layer, 1.0))
    }
}

/// Reads a style file's TOML with its `extends` chain resolved, reading files through `read`
/// (the filesystem, or the built-in styles).
pub(crate) fn load(path: &Path, read: &dyn Fn(&Path) -> Result<String>) -> Result<Table> {
    load_at(path, 0, read)
}

fn load_at(path: &Path, depth: usize, read: &dyn Fn(&Path) -> Result<String>) -> Result<Table> {
    anyhow::ensure!(
        depth < 8,
        "style `extends` chain too deep at {}",
        path.display()
    );
    let text = read(path)?;
    let mut raw: Table =
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    let Some(base) = raw.remove("extends") else {
        return Ok(raw);
    };
    // A path, or a list of paths: the first is the base, each later one an overlay merged
    // over it in order (e.g. a palette base plus a brushwork overlay), then this file.
    let bases: Vec<&str> = match &base {
        Value::String(s) => vec![s.as_str()],
        Value::Array(a) => a.iter().filter_map(|v| v.as_str()).collect(),
        _ => Vec::new(),
    };
    let paths_ok = match &base {
        Value::Array(a) => !a.is_empty() && a.iter().all(|v| v.is_str()),
        v => v.is_str(),
    };
    anyhow::ensure!(
        paths_ok,
        "{}: `extends` must be a path or a non-empty list of paths",
        path.display()
    );
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut stack = StyleStack::new();
    for b in bases {
        stack.push(load_at(&dir.join(b), depth + 1, read)?);
    }
    stack.push(raw);
    Ok(stack.resolve())
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
        let half = merge(&base, &over, 0.5);
        assert_eq!(half["a"].as_float(), Some(0.5));
        assert_eq!(half["n"].as_integer(), Some(15));
        assert_eq!(
            half["curve"].as_array().unwrap()[0].as_array().unwrap()[1].as_float(),
            Some(0.4)
        );
        assert_eq!(half["x"]["b"].as_float(), Some(0.5));
        assert_eq!(half["x"]["c"].as_str(), Some("keep"));
        assert_eq!(merge(&base, &over, 0.0), base);
        let full = merge(&base, &over, 1.0);
        assert_eq!(full["a"].as_float(), Some(0.0));
    }

    #[test]
    fn non_numbers_switch_at_half() {
        let base = t("s = 'a'\nlist = [1, 2]");
        let over = t("s = 'b'\nlist = [1, 2, 3]");
        assert_eq!(merge(&base, &over, 0.4)["s"].as_str(), Some("a"));
        assert_eq!(merge(&base, &over, 0.6)["s"].as_str(), Some("b"));
        assert_eq!(
            merge(&base, &over, 0.6)["list"].as_array().unwrap().len(),
            3
        );
    }

    #[test]
    fn a_stack_folds_its_layers_in_order() {
        let mut stack = StyleStack::new();
        assert_eq!(stack.resolve(), Table::new());
        stack.push(t("a = 1.0\nb = 1.0"));
        stack.push(t("b = 2.0\nc = 2.0"));
        stack.push(t("c = 3.0"));
        let r = stack.resolve();
        assert_eq!(
            (r["a"].as_float(), r["b"].as_float(), r["c"].as_float()),
            (Some(1.0), Some(2.0), Some(3.0))
        );
    }
}
