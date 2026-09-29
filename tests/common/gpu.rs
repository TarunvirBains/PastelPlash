//! The stylize stage on the GPU, created once per style (`None` without an adapter).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use pastelplash::config::{Category, Config, Mood};
use pastelplash::image::Image;
use pastelplash::pipeline::{FileContext, Stage};
use pastelplash::stylize::Stylize;

use super::{default_target, load};

/// The stylization stage for a style (created once per style; `None` without a GPU).
pub fn stylizer(style: &Path) -> Option<&'static Stylize> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Option<&'static Stylize>>>> = OnceLock::new();
    let mut cache = CACHE.get_or_init(Default::default).lock().unwrap();
    *cache.entry(style.to_path_buf()).or_insert_with(|| {
        let config = load(style, &default_target());
        match Stylize::new(&config) {
            Ok(s) => Some(Box::leak(Box::new(s))),
            Err(e) => {
                eprintln!("skipping GPU checks: no usable GPU adapter ({e:#})");
                None
            }
        }
    })
}

/// Runs the stage on a copy of `img` as `category` in the base mood; `None` without a GPU.
pub fn render(style: &Path, config: &Config, category: Category, img: &Image) -> Option<Image> {
    render_mood(style, config, category, &Mood::default(), img)
}

/// Runs the stage on a copy of `img` as `category` in `mood`; `None` if no GPU is available.
pub fn render_mood(
    style: &Path,
    config: &Config,
    category: Category,
    mood: &Mood,
    img: &Image,
) -> Option<Image> {
    let stage = stylizer(style)?;
    let mut out = img.clone();
    let ctx = FileContext {
        rel: Path::new("test.png"),
        category,
        mood: mood.clone(),
        config,
        upscale: 1.0,
    };
    stage.apply(&mut out, &ctx).unwrap();
    Some(out)
}
