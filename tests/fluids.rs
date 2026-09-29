//! Fluid detection on procedural textures (CPU): caustic water (colored and engine-tinted gray)
//! and lava are recognized by their look; ordinary materials are not.

mod common;

use common::*;
use pastelplash::fluid::{FluidKind, detect};

#[test]
fn caustic_water_is_detected_colored_or_gray() {
    for (label, chroma) in [("jade", 0.06), ("gray", 0.0)] {
        let (img, _) = caustic_water(256, 3, chroma);
        let d = detect(&img);
        assert_eq!(
            d.kind,
            Some(FluidKind::Water),
            "{label} caustic water: {d:#?}"
        );
    }
}

#[test]
fn lava_is_detected() {
    let (img, _) = lava(256, 5);
    let d = detect(&img);
    assert_eq!(d.kind, Some(FluidKind::Lava), "{d:#?}");
}

#[test]
fn ordinary_materials_are_not_fluids() {
    for (label, img) in [
        ("bark", bark(256, 1)),
        ("dark brown bark", dark_brown_bark(256, 2)),
        ("foliage", dark_foliage(256, 3)),
        ("mid foliage", mid_foliage(256, 4)),
        ("grooved wood", grooved_wood(256, 5)),
        ("moss on wood", moss_on_wood(256, 6)),
        ("gritty blocks", gritty_blocks(256, 7)),
        ("tiling", tiling(256, 8)),
        ("grayscale", grayscale(256, 9)),
        ("dark hues", dark_hues(256, 10)),
        ("dull browns", dull_browns(256, 11)),
        ("sign", sign(256, 12)),
        ("cutout", cutout(256, 13)),
    ] {
        let d = detect(&img);
        assert_eq!(d.kind, None, "{label}: {d:#?}");
    }
}

#[test]
fn detection_does_not_depend_on_resolution() {
    let (small, _) = caustic_water(256, 7, 0.05);
    let big = pastelplash::image::Image {
        width: 1024,
        height: 1024,
        pixels: (0..1024 * 1024)
            .map(|i| small.pixels[((i / 1024) / 4) * 256 + (i % 1024) / 4])
            .collect(),
        ..small.clone()
    };
    assert_eq!(detect(&small).kind, detect(&big).kind);
    assert_eq!(
        detect(&big).kind,
        Some(FluidKind::Water),
        "{:#?}",
        detect(&big)
    );
}
