//! Shared helpers for the rule and snapshot tests: the style contract, style/target discovery,
//! procedural test images and a cached GPU stage per style.

#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use pastelplash::color;
use pastelplash::config::{Category, Config, Mood};
use pastelplash::image::{Image, SourceColor, SourceFormat};
use pastelplash::pipeline::{FileContext, Stage};
use pastelplash::stylize::Stylize;
use serde::Deserialize;

pub fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

// ------------------------------------------------------------------ contract

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contract {
    pub tolerance: Tolerance,
    pub palette: PaletteRules,
    pub accents: AccentRules,
    pub vivid: VividRules,
    pub technique: TechniqueRules,
    pub target: TargetRules,
    pub actor: ActorRules,
    pub identity: IdentityRules,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityRules {
    pub max_mean_delta_e: f32,
    pub max_group_hue_shift: f32,
    /// Larger bounds for named opt-in styles.
    #[serde(default)]
    pub styles: std::collections::BTreeMap<String, f32>,
}

impl IdentityRules {
    /// The mean-color ΔE bound for a style (by file stem).
    pub fn bound(&self, style: &str) -> f32 {
        self.styles
            .get(style)
            .copied()
            .unwrap_or(self.max_mean_delta_e)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tolerance {
    pub lightness: f32,
    pub chroma: f32,
    pub outliers: f32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaletteRules {
    pub min_strength: f32,
    pub max_strength: f32,
    pub max_floor_margin: f32,
    pub min_l: f32,
    pub dark_l: f32,
    pub dark_min_chroma: f32,
    pub mud_l: f32,
    pub mud_hue: [f32; 2],
    pub mud_max_chroma: f32,
    pub retention_min_source_chroma: f32,
    pub retention_ratio: f32,
    pub retention_floor: f32,
    pub max_hue_shift: f32,
}

impl PaletteRules {
    /// The least chroma a source of chroma `c` may come out with.
    pub fn retained(&self, c: f32) -> f32 {
        (self.retention_ratio * c).min(self.retention_floor)
    }

    /// True for a dull brownish dark ("mud").
    pub fn is_mud(&self, [l, c, h]: [f32; 3]) -> bool {
        l < self.mud_l && c >= 0.012 && c < self.mud_max_chroma && in_hue_range(h, self.mud_hue)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccentRules {
    pub max_fraction: f32,
    pub hue: [f32; 2],
    pub min_l: f32,
    pub max_chroma: f32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VividRules {
    pub max_amount: f32,
    pub max_chroma: f32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TechniqueRules {
    pub max_kuwahara_radius: f32,
    pub max_edge_darkening: f32,
    pub max_granulation: f32,
    pub max_paper_grain: f32,
    pub max_paper_tint: f32,
    pub max_stroke_strength: f32,
    pub max_smear: f32,
    pub max_temperature_chroma: f32,
    pub max_edge_width: f32,
    pub value_min_effect: f32,
    pub value_mean_tolerance: f32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetRules {
    pub max_actor_ceiling: f32,
    pub max_actor_warm_cool: f32,
    pub max_actor_shadow_tint: f32,
    pub max_actor_delight: f32,
    pub max_actor_lift: f32,
    pub max_actor_hue: f32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActorRules {
    pub retention_ratio: f32,
    pub max_lightness_shift: f32,
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
                moods.push(Mood {
                    name: name.clone(),
                    strength,
                    dark_greens: None,
                });
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

pub fn contract() -> &'static Contract {
    static C: OnceLock<Contract> = OnceLock::new();
    C.get_or_init(|| {
        let text = std::fs::read_to_string(repo().join("rules.toml")).expect("rules.toml");
        toml::from_str(&text).expect("rules.toml parses")
    })
}

/// True if `h` lies in the (possibly wrapping) hue range.
pub fn in_hue_range(h: f32, [from, to]: [f32; 2]) -> bool {
    (h - from).rem_euclid(360.0) <= (to - from).rem_euclid(360.0)
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

// ------------------------------------------------------------------ images

pub fn image(w: u32, h: u32, f: impl Fn(u32, u32) -> [f32; 4]) -> Image {
    Image {
        width: w,
        height: h,
        pixels: (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| f(x, y))
            .collect(),
        source: SourceFormat {
            color: SourceColor::Rgba,
            bit_depth: 8,
            has_alpha: true,
        },
        source_scale: None,
        tint_safe: None,
    }
}

/// Deterministic hash noise in 0..1.
pub fn noise(x: u32, y: u32, seed: u32) -> f32 {
    let mut v =
        x.wrapping_mul(0x9E37_79B9) ^ y.wrapping_mul(0x85EB_CA6B) ^ seed.wrapping_mul(0xC2B2_AE35);
    v ^= v >> 15;
    v = v.wrapping_mul(0x2C1B_3C6D);
    v ^= v >> 12;
    v = v.wrapping_mul(0x297A_2D39);
    v ^= v >> 15;
    (v & 0xFFFF) as f32 / 65535.0
}

/// Smooth periodic value noise (period = image size / cells), 0..1.
pub fn smooth_noise(x: f32, y: f32, cells: u32, size: u32, seed: u32) -> f32 {
    let fx = x / size as f32 * cells as f32;
    let fy = y / size as f32 * cells as f32;
    let (ix, iy) = (fx.floor() as i64, fy.floor() as i64);
    let (tx, ty) = (fx - ix as f32, fy - iy as f32);
    let (tx, ty) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
    let at = |i: i64, j: i64| {
        noise(
            i.rem_euclid(cells as i64) as u32,
            j.rem_euclid(cells as i64) as u32,
            seed,
        )
    };
    let a = at(ix, iy) + (at(ix + 1, iy) - at(ix, iy)) * tx;
    let b = at(ix, iy + 1) + (at(ix + 1, iy + 1) - at(ix, iy + 1)) * tx;
    a + (b - a) * ty
}

pub fn from_oklch(l: f32, c: f32, h: f32) -> [f32; 3] {
    color::oklch_to_srgb_gamut([l, c, h])
}

/// Dark, textured foliage: leaf blobs in dark and mid greens with gaps, tiling.
pub fn dark_foliage(size: u32, seed: u32) -> Image {
    image(size, size, |x, y| {
        let (fx, fy) = (x as f32, y as f32);
        let blob = smooth_noise(fx, fy, 8, size, seed);
        let detail = noise(x, y, seed + 1);
        let l = if blob > 0.55 {
            0.35 + 0.2 * detail
        } else {
            0.12 + 0.12 * detail
        };
        let h = 120.0 + 30.0 * smooth_noise(fx, fy, 4, size, seed + 2);
        let [r, g, b] = from_oklch(l, 0.09 + 0.04 * detail, h);
        [r, g, b, 1.0]
    })
}

/// Saturated darks of many hues (reds, blues, purples, browns) with noise.
pub fn dark_hues(size: u32, seed: u32) -> Image {
    image(size, size, |x, y| {
        let h = x as f32 / size as f32 * 360.0;
        let l = 0.1 + 0.5 * y as f32 / size as f32 + 0.05 * noise(x, y, seed);
        let [r, g, b] = from_oklch(l, 0.15, h);
        [r, g, b, 1.0]
    })
}

pub fn grayscale(size: u32, seed: u32) -> Image {
    image(size, size, |x, y| {
        let v =
            0.15 + 0.6 * smooth_noise(x as f32, y as f32, 6, size, seed) + 0.1 * noise(x, y, seed);
        [v, v, v, 1.0]
    })
}

/// Near-black neutrals with slight noise (crushed shadows).
pub fn grayscale_dark(size: u32, seed: u32) -> Image {
    image(size, size, |x, y| {
        let v =
            0.02 + 0.1 * smooth_noise(x as f32, y as f32, 6, size, seed) + 0.02 * noise(x, y, seed);
        // A faint warm cast keeps it out of tint-safe detection (real crushed shadows).
        let [r, g, b] = from_oklch(color::srgb_to_oklab([v, v, v])[0], 0.05, 60.0);
        [r, g, b, 1.0]
    })
}

/// Dull dark browns: dirt and grime, the raw material of "mud".
pub fn dull_browns(size: u32, seed: u32) -> Image {
    image(size, size, |x, y| {
        let l =
            0.18 + 0.2 * smooth_noise(x as f32, y as f32, 5, size, seed) + 0.04 * noise(x, y, seed);
        let [r, g, b] = from_oklch(
            l,
            0.03 + 0.03 * noise(x, y, seed + 1),
            55.0 + 30.0 * noise(x, y, seed + 2),
        );
        [r, g, b, 1.0]
    })
}

/// Mid-value foliage (typical lit grass/leaves), tiling.
pub fn mid_foliage(size: u32, seed: u32) -> Image {
    image(size, size, |x, y| {
        let (fx, fy) = (x as f32, y as f32);
        let l = 0.4 + 0.25 * smooth_noise(fx, fy, 8, size, seed) + 0.06 * noise(x, y, seed);
        let [r, g, b] = from_oklch(
            l,
            0.1,
            125.0 + 15.0 * smooth_noise(fx, fy, 4, size, seed + 1),
        );
        [r, g, b, 1.0]
    })
}

/// Stone blocks with mortar and photographic grit (fine light/dark noise).
pub fn gritty_blocks(size: u32, seed: u32) -> Image {
    image(size, size, |x, y| {
        let mortar = y % 32 < 4 || (x + if (y / 32) % 2 == 1 { 24 } else { 0 }) % 48 < 4;
        let base = if mortar { 0.35 } else { 0.62 };
        let l = base + 0.18 * (noise(x, y, seed) - 0.5);
        let [r, g, b] = from_oklch(l, 0.05, 75.0);
        [r, g, b, 1.0]
    })
}

/// Pale peach skin, like an actor's hand or face texture.
pub fn pale_skin(size: u32, seed: u32) -> Image {
    image(size, size, |x, y| {
        let l = 0.84
            + 0.06 * smooth_noise(x as f32, y as f32, 4, size, seed)
            + 0.01 * noise(x, y, seed);
        let [r, g, b] = from_oklch(l, 0.07, 70.0);
        [r, g, b, 1.0]
    })
}

/// A leafy cutout: an opaque disc of mid green on fully transparent black texels.
pub fn cutout(size: u32, seed: u32) -> Image {
    let c = size as f32 / 2.0;
    image(size, size, |x, y| {
        let d = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt();
        if d < size as f32 * 0.35 {
            let [r, g, b] = from_oklch(0.6 + 0.05 * noise(x, y, seed), 0.1, 135.0);
            [r, g, b, 1.0]
        } else {
            [0.0, 0.0, 0.0, 0.0]
        }
    })
}

/// A hard vertical step between two flat, finely noisy regions.
pub fn step_edge(size: u32, seed: u32) -> Image {
    image(size, size, |x, y| {
        let n = 0.04 * (noise(x, y, seed) - 0.5);
        let l = if x < size / 2 { 0.4 } else { 0.8 } + n;
        let [r, g, b] = from_oklch(l, 0.06, 80.0);
        [r, g, b, 1.0]
    })
}

/// A seamlessly tiling colorful pattern.
pub fn tiling(size: u32, seed: u32) -> Image {
    image(size, size, |x, y| {
        let (fx, fy) = (x as f32, y as f32);
        let l = 0.3 + 0.5 * smooth_noise(fx, fy, 6, size, seed);
        let h = 360.0 * smooth_noise(fx, fy, 3, size, seed + 7);
        let [r, g, b] = from_oklch(l, 0.1, h);
        [r, g, b, 1.0]
    })
}

pub fn lch(p: [f32; 4]) -> [f32; 3] {
    color::oklab_to_oklch(color::srgb_to_oklab([p[0], p[1], p[2]]))
}

// ------------------------------------------------------------------ GPU

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
    };
    stage.apply(&mut out, &ctx).unwrap();
    Some(out)
}
