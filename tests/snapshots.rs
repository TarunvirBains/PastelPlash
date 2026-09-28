//! Snapshot (golden) tests: regression alarms, not rules. Procedural, license-clean textures are
//! rendered with the default style and compared with committed goldens in `tests/golden/` using
//! a perceptual tolerance (GPU float results differ slightly across drivers).
//!
//! An intended change of look: `PASTELPLASH_BLESS=1 cargo test --test snapshots` rewrites the
//! goldens; review them and commit. Skips (passes with a message) without a GPU adapter.

mod common;

use common::*;
use pastelplash::color;
use pastelplash::config::Category;
use pastelplash::image::Image;
use pastelplash::png_io;

/// Mean OKLab distance allowed over the image.
const MEAN_DELTA_E: f32 = 0.01;
/// Share of texels allowed above `OUTLIER_DELTA_E`.
const OUTLIER_SHARE: f32 = 0.01;
const OUTLIER_DELTA_E: f32 = 0.05;

fn bricks(size: u32) -> Image {
    image(size, size, |x, y| {
        let row = y / 24;
        let bx = (x + if row % 2 == 1 { 20 } else { 0 }) % 40;
        let mortar = y % 24 < 3 || bx < 3;
        let v = if mortar {
            0.25 + 0.05 * noise(x, y, 1)
        } else {
            0.45 + 0.2 * smooth_noise(x as f32, y as f32, 12, size, 2) + 0.08 * noise(x, y, 3)
        };
        [v, v, v, 1.0]
    })
}

fn sky(size: u32) -> Image {
    image(size, size, |x, y| {
        let t = y as f32 / size as f32;
        let cloud = smooth_noise(x as f32, y as f32, 5, size, 4);
        let l = 0.55 + 0.25 * t + 0.25 * (cloud - 0.5).max(0.0);
        let [r, g, b] = from_oklch(l, 0.08 * (1.0 - cloud), 300.0 - 250.0 * t);
        [r, g, b, 1.0]
    })
}

fn cases() -> Vec<(&'static str, Category, Image)> {
    vec![
        ("foliage", Category::World, dark_foliage(192, 3)),
        ("bricks", Category::World, bricks(192)),
        ("sky", Category::Skybox, sky(192)),
        ("cutout_actor", Category::Actor, cutout(128, 2)),
        ("hues_actor", Category::Actor, dark_hues(128, 6)),
    ]
}

fn delta_e(a: [f32; 4], b: [f32; 4]) -> f32 {
    let (x, y) = (
        color::srgb_to_oklab([a[0], a[1], a[2]]),
        color::srgb_to_oklab([b[0], b[1], b[2]]),
    );
    let d = ((x[0] - y[0]).powi(2) + (x[1] - y[1]).powi(2) + (x[2] - y[2]).powi(2)).sqrt();
    // Fully transparent texels only need matching alpha.
    if a[3] == 0.0 && b[3] == 0.0 {
        0.0
    } else {
        d + (a[3] - b[3]).abs()
    }
}

#[test]
fn snapshots_match_goldens() {
    let style = repo().join("styles/skyward-watercolor.toml");
    let config = load(&style, &default_target());
    let bless = std::env::var_os("PASTELPLASH_BLESS").is_some();
    let dir = repo().join("tests/golden");
    let mut failures = Vec::new();
    for (name, category, img) in cases() {
        let Some(mut out) = render(&style, &config, category, &img) else {
            return;
        };
        out.source.bit_depth = 8;
        let path = dir.join(format!("{name}.png"));
        if bless {
            std::fs::create_dir_all(&dir).unwrap();
            png_io::write(&out, &path).unwrap();
            eprintln!("blessed {}", path.display());
            continue;
        }
        let golden = png_io::read(&path).unwrap_or_else(|e| {
            panic!("{e:#}\nmissing golden; run `PASTELPLASH_BLESS=1 cargo test --test snapshots`")
        });
        assert_eq!(
            (golden.width, golden.height),
            (out.width, out.height),
            "{name}: size"
        );
        let d: Vec<f32> = golden
            .pixels
            .iter()
            .zip(&out.pixels)
            .map(|(&a, &b)| delta_e(a, b))
            .collect();
        let mean = d.iter().sum::<f32>() / d.len() as f32;
        let outliers = d.iter().filter(|&&x| x > OUTLIER_DELTA_E).count() as f32 / d.len() as f32;
        if mean > MEAN_DELTA_E || outliers > OUTLIER_SHARE {
            failures.push(format!(
                "{name}: mean ΔE {mean:.4} (max {MEAN_DELTA_E}), outliers {:.2}% (max {:.2}%)",
                outliers * 100.0,
                OUTLIER_SHARE * 100.0
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "snapshots differ from tests/golden:\n  {}\nIf the new look is intended: PASTELPLASH_BLESS=1 cargo test --test snapshots",
        failures.join("\n  ")
    );
}
