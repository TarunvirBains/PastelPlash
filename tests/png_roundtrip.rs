//! Decode → encode must be lossless for every PNG variant.

use std::fs;
use std::io::Cursor;
use std::path::Path;

use pastelplash::image::SourceColor;
use pastelplash::png_io;
use png::{BitDepth, ColorType, Transformations};

const W: u32 = 7; // odd, so low-bit rows end mid-byte
const H: u32 = 5;

/// Deterministic bytes covering the whole 0..=255 range.
fn noise(len: usize, seed: u32) -> Vec<u8> {
    let mut x = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x >> 24) as u8
        })
        .collect()
}

fn make_png(color: ColorType, depth: BitDepth, trns: bool) -> Vec<u8> {
    let bits = color.samples() * depth as usize;
    let row = (W as usize * bits).div_ceil(8);
    let data = noise(row * H as usize, color as u32 * 31 + depth as u32);

    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, W, H);
    encoder.set_color(color);
    encoder.set_depth(depth);
    if color == ColorType::Indexed {
        let entries = 1usize << depth as u8;
        encoder.set_palette(noise(entries * 3, 7));
        if trns {
            encoder.set_trns(noise(entries / 2 + 1, 9));
        }
    } else if trns {
        // A single transparent color key, taken from the first pixel so it is actually used.
        let key_len = if color == ColorType::Rgb { 6 } else { 2 };
        let key: Vec<u8> = match depth {
            BitDepth::Sixteen => data[..key_len].to_vec(),
            BitDepth::Eight => data[..key_len / 2].iter().flat_map(|&b| [0, b]).collect(),
            _ => vec![0, data[0] >> (8 - depth as u8)],
        };
        encoder.set_trns(key);
    }
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&data).unwrap();
    writer.finish().unwrap();
    out
}

fn decode_with(bytes: &[u8], transform: Transformations) -> (ColorType, BitDepth, Vec<u8>) {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(transform);
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    reader.next_frame(&mut buf).unwrap();
    let (color, depth) = reader.output_color_type();
    (color, depth, buf)
}

fn roundtrip(dir: &Path, color: ColorType, depth: BitDepth, trns: bool) {
    let name = format!("{color:?}_{}_{trns}", depth as u8);
    let input = dir.join(format!("{name}.png"));
    let output = dir.join(format!("{name}.out.png"));
    let original = make_png(color, depth, trns);
    fs::write(&input, &original).unwrap();

    let image = png_io::read(&input).unwrap();
    assert_eq!(image.pixels.len(), (W * H) as usize);
    assert_eq!(image.source.bit_depth, depth as u8, "{name}");
    assert_eq!(
        image.source.has_alpha,
        trns || matches!(color, ColorType::GrayscaleAlpha | ColorType::Rgba),
        "{name}"
    );
    png_io::write(&image, &output).unwrap();
    let written = fs::read(&output).unwrap();

    // Pixel values must match exactly once both files are expanded the same way.
    let expected = decode_with(&original, Transformations::EXPAND);
    let actual = decode_with(&written, Transformations::EXPAND);
    assert_eq!(expected, actual, "{name}: pixels differ");

    // The file itself keeps color model and depth (palettes become RGB, low-bit gray becomes 8-bit).
    let (out_color, out_depth, _) = decode_with(&written, Transformations::IDENTITY);
    assert_eq!(
        (out_color, out_depth),
        (expected.0, expected.1),
        "{name}: format changed"
    );
    if !trns && color != ColorType::Indexed && depth as u8 >= 8 {
        assert_eq!(
            decode_with(&original, Transformations::IDENTITY),
            decode_with(&written, Transformations::IDENTITY)
        );
    }
}

#[test]
fn every_variant_roundtrips_losslessly() {
    use BitDepth::*;
    use ColorType::*;
    let dir = tempfile::tempdir().unwrap();
    let cases: &[(ColorType, &[BitDepth], bool)] = &[
        (Grayscale, &[One, Two, Four, Eight, Sixteen], false),
        (Grayscale, &[One, Four, Eight, Sixteen], true),
        (GrayscaleAlpha, &[Eight, Sixteen], false),
        (Rgb, &[Eight, Sixteen], false),
        (Rgb, &[Eight, Sixteen], true),
        (Rgba, &[Eight, Sixteen], false),
        (Indexed, &[One, Two, Four, Eight], false),
        (Indexed, &[Two, Eight], true),
    ];
    for &(color, depths, trns) in cases {
        for &depth in depths {
            roundtrip(dir.path(), color, depth, trns);
        }
    }
}

#[test]
fn gray_and_palette_are_recorded() {
    let gray = png_io::decode(Cursor::new(make_png(
        ColorType::Grayscale,
        BitDepth::Sixteen,
        false,
    )))
    .unwrap();
    assert_eq!(gray.source.color, SourceColor::Gray);
    assert!(
        gray.pixels
            .iter()
            .all(|p| p[0] == p[1] && p[1] == p[2] && p[3] == 1.0)
    );

    let indexed = png_io::decode(Cursor::new(make_png(
        ColorType::Indexed,
        BitDepth::Four,
        true,
    )))
    .unwrap();
    assert_eq!(indexed.source.color, SourceColor::Indexed);
    assert!(indexed.source.has_alpha);
}

#[test]
fn corrupt_file_is_an_error() {
    let mut bytes = make_png(ColorType::Rgb, BitDepth::Eight, false);
    bytes.truncate(bytes.len() / 2);
    assert!(png_io::decode(Cursor::new(bytes)).is_err());
}
