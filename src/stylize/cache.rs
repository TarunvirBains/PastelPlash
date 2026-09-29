//! Per-stage caches: styles derived for moods and palette LUTs on the GPU. Every key includes a
//! fingerprint of what the entry is derived from (the resolved style or palette), so one stage
//! can serve several styles, stacks or configs without handing out stale entries.

use std::borrow::Cow;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::fmt::{self, Debug, Write as _};
use std::hash::Hasher;
use std::sync::{Arc, Mutex};

use anyhow::Result;

use super::plan::{LutKey, LutSpec};
use super::runner::Runner;
use crate::config::{Mood, Style};

/// A fingerprint of a value's full `Debug` form (every field, in order).
pub(super) fn fingerprint(value: &impl Debug) -> u64 {
    struct Feed(DefaultHasher);
    impl fmt::Write for Feed {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            self.0.write(s.as_bytes());
            Ok(())
        }
    }
    let mut feed = Feed(DefaultHasher::new());
    let _ = write!(feed, "{value:?}");
    feed.0.finish()
}

#[derive(Default)]
pub(super) struct Cache {
    /// The style's external `.cube` on the GPU.
    external_lut: Option<Arc<wgpu::Buffer>>,
    /// Generated palette LUTs by (palette fingerprint, treatment scale bits).
    luts: Mutex<HashMap<LutKey, Arc<wgpu::Buffer>>>,
    /// Styles derived for non-base moods, by (style fingerprint, mood key).
    moods: Mutex<HashMap<(u64, String), Arc<Style>>>,
}

impl Cache {
    pub fn new(external_lut: Option<Arc<wgpu::Buffer>>) -> Self {
        Self {
            external_lut,
            ..Self::default()
        }
    }

    /// The style for a mood: `style` itself for the base mood, otherwise derived once per
    /// (style, mood) and cached.
    pub fn style_for<'a>(&self, style: &'a Style, mood: &Mood) -> Result<Cow<'a, Style>> {
        if mood.is_base() {
            return Ok(Cow::Borrowed(style));
        }
        let key = (fingerprint(style), mood.key());
        if let Some(s) = self.moods.lock().unwrap().get(&key) {
            return Ok(Cow::Owned((**s).clone()));
        }
        let derived = Arc::new(style.for_mood(mood)?);
        self.moods.lock().unwrap().insert(key, derived.clone());
        Ok(Cow::Owned((*derived).clone()))
    }

    /// The GPU buffer for a planned LUT (generated LUTs are baked once and cached).
    pub fn lut(&self, spec: &LutSpec, runner: &Runner) -> Arc<wgpu::Buffer> {
        let key = match spec {
            LutSpec::External(_) => {
                return self.external_lut.clone().expect("external LUT is loaded");
            }
            LutSpec::Palette { key, .. } => key,
        };
        if let Some(buf) = self.luts.lock().unwrap().get(key) {
            return buf.clone();
        }
        // Baked outside the lock: baking runs on rayon, and a worker that steals another file's
        // job while waiting would block on the lock it holds itself.
        let buf = Arc::new(runner.upload_lut(&spec.bake()));
        self.luts.lock().unwrap().entry(*key).or_insert(buf).clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprints_tell_styles_apart() {
        let a = crate::config::Palette::default();
        let mut b = a.clone();
        assert_eq!(fingerprint(&a), fingerprint(&b));
        b.l_floor = 0.2;
        assert_ne!(fingerprint(&a), fingerprint(&b));
    }
}
