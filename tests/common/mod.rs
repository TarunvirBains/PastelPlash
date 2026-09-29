//! Shared helpers for the rule and snapshot tests: the style contract, style/target discovery,
//! procedural test images and a cached GPU stage per style.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use pastelplash::config::{Config, Mood};

mod contract;
mod gpu;
mod images;

#[allow(unused_imports)]
pub use contract::*;
#[allow(unused_imports)]
pub use gpu::*;
#[allow(unused_imports)]
pub use images::*;

pub fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every style in `styles/` in every mood it defines (base mood first), as loaded configs with
/// the default target: `(label, style path, config, mood)`.
pub fn style_moods() -> Vec<(String, PathBuf, Config, Mood)> {
    let mut out = Vec::new();
    for path in styles() {
        let config = load(&path, &default_target());
        let mut moods = vec![Mood::default()];
        for name in config.style.moods.keys() {
            for strength in [0.5, 1.0] {
                moods.push(Mood::new(name, strength));
            }
        }
        for mood in moods {
            out.push((
                format!("{} [{mood}]", name(&path)),
                path.clone(),
                config.clone(),
                mood,
            ));
        }
    }
    out
}

// ------------------------------------------------------------------ discovery

fn toml_files(dir: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(repo().join(dir))
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .collect();
    v.sort();
    assert!(!v.is_empty(), "no .toml files in {dir}/");
    v
}

/// Every style shipped in `styles/` (discovered, so new styles are covered automatically).
pub fn styles() -> Vec<PathBuf> {
    toml_files("styles")
}

pub fn targets() -> Vec<PathBuf> {
    toml_files("targets")
}

pub fn default_target() -> PathBuf {
    repo().join("targets/soh-celshade.toml")
}

pub fn name(path: &Path) -> String {
    path.file_stem().unwrap().to_string_lossy().into_owned()
}

pub fn load(style: &Path, target: &Path) -> Config {
    Config::load(Some(style), Some(target), None).unwrap()
}
