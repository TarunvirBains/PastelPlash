//! PNG decoding into [`Image`] and encoding back out.

use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Seek, Write};
use std::path::Path;

use anyhow::{Context, Result, bail};
use png::{BitDepth, ColorType, Transformations};

use crate::image::{Channels, Image, SourceColor, SourceFormat};

pub fn read(path: &Path) -> Result<Image> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    decode(BufReader::new(file)).with_context(|| format!("decoding {}", path.display()))
}

pub fn write(image: &Image, path: &Path) -> Result<()> {
    let file = File::create(path).with_context(|| format!("creating {}", path.display()))?;
    let mut out = BufWriter::new(file);
    encode(image, &mut out)
        .and_then(|()| Ok(out.flush()?))
        .with_context(|| format!("encoding {}", path.display()))
}

/// Decodes the first (or only) frame of a PNG, expanding every variant to RGBA.
pub fn decode(reader: impl BufRead + Seek) -> Result<Image> {
    let mut decoder = png::Decoder::new(reader);
    // Expand palettes and low-bit gray to 8-bit, tRNS to alpha; 16-bit stays 16-bit.
    decoder.set_transformations(Transformations::EXPAND);
    let mut reader = decoder.read_info()?;

    let info = reader.info();
    let color = match info.color_type {
        ColorType::Grayscale => SourceColor::Gray,
        ColorType::GrayscaleAlpha => SourceColor::GrayAlpha,
        ColorType::Rgb => SourceColor::Rgb,
        ColorType::Rgba => SourceColor::Rgba,
        ColorType::Indexed => SourceColor::Indexed,
    };
    let (width, height) = (info.width, info.height);
    let bit_depth = info.bit_depth as u8;

    let (out_color, out_depth) = reader.output_color_type();
    let has_alpha = matches!(out_color, ColorType::GrayscaleAlpha | ColorType::Rgba);
    let buffer_size = reader.output_buffer_size().context("image too large")?;
    let mut buf = vec![0; buffer_size];
    reader.next_frame(&mut buf)?;

    let samples: Vec<f32> = match out_depth {
        BitDepth::Eight => buf.iter().map(|&v| f32::from(v) / 255.0).collect(),
        BitDepth::Sixteen => buf
            .chunks_exact(2)
            .map(|b| f32::from(u16::from_be_bytes([b[0], b[1]])) / 65535.0)
            .collect(),
        other => bail!("unexpected decoded bit depth {other:?}"),
    };
    let pixels = match out_color {
        ColorType::Grayscale => samples.iter().map(|&v| [v, v, v, 1.0]).collect(),
        ColorType::GrayscaleAlpha => samples
            .chunks_exact(2)
            .map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        ColorType::Rgb => samples
            .chunks_exact(3)
            .map(|p| [p[0], p[1], p[2], 1.0])
            .collect(),
        ColorType::Rgba => samples
            .chunks_exact(4)
            .map(|p| [p[0], p[1], p[2], p[3]])
            .collect(),
        ColorType::Indexed => bail!("palette was not expanded"),
    };

    Ok(Image {
        width,
        height,
        pixels,
        source: SourceFormat {
            color,
            bit_depth,
            has_alpha,
        },
    })
}

/// Encodes in the layout chosen by [`SourceFormat::output`]. Gray outputs use Rec. 709 luma of
/// the (encoded) color channels, which is exact when `r == g == b`.
pub fn encode(image: &Image, writer: impl Write) -> Result<()> {
    let format = image.source.output();
    let (color, channels) = match format.channels {
        Channels::Gray => (ColorType::Grayscale, 1),
        Channels::GrayAlpha => (ColorType::GrayscaleAlpha, 2),
        Channels::Rgb => (ColorType::Rgb, 3),
        Channels::Rgba => (ColorType::Rgba, 4),
    };

    let mut samples = Vec::with_capacity(image.pixels.len() * channels);
    for &[r, g, b, a] in &image.pixels {
        let luma = || 0.2126 * r + 0.7152 * g + 0.0722 * b;
        match format.channels {
            Channels::Gray => samples.push(luma()),
            Channels::GrayAlpha => samples.extend([luma(), a]),
            Channels::Rgb => samples.extend([r, g, b]),
            Channels::Rgba => samples.extend([r, g, b, a]),
        }
    }

    let (depth, data): (_, Vec<u8>) = if format.sixteen_bit {
        let data = samples
            .iter()
            .flat_map(|&v| quantize(v, 65535.0).to_be_bytes())
            .collect();
        (BitDepth::Sixteen, data)
    } else {
        let data = samples.iter().map(|&v| quantize(v, 255.0) as u8).collect();
        (BitDepth::Eight, data)
    };

    let mut encoder = png::Encoder::new(writer, image.width, image.height);
    encoder.set_color(color);
    encoder.set_depth(depth);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&data)?;
    writer.finish()?;
    Ok(())
}

fn quantize(v: f32, max: f32) -> u16 {
    (v.clamp(0.0, 1.0) * max).round() as u16
}
