//! The in-memory working image that pipeline stages operate on.

/// An RGBA image in the pipeline's working representation.
///
/// Pixels are row-major `[r, g, b, a]` as `f32` in `0.0..=1.0`, with straight (not premultiplied)
/// alpha. Color channels hold the file's **gamma-encoded sRGB** values, not linear light: this keeps
/// decode → encode exact at 8 and 16 bits, and it is the same layout as a GPU `rgba32float` texture
/// or `array<vec4<f32>>` buffer (see [`Image::as_f32`]). Stages that need linear light or OKLCH
/// convert on the way in and back on the way out.
///
/// Grayscale sources have `r == g == b`; images without alpha have `a == 1.0`.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<[f32; 4]>,
    /// How the source file stored its pixels; decides how the result is encoded.
    pub source: SourceFormat,
    /// Output pixels per source pixel, when an adapter knows the texture was upscaled from a
    /// lower-resolution original. Stages use it to size brushes consistently; `None` falls back
    /// to the pack/style default, then to image-size-relative sizing.
    pub source_scale: Option<f32>,
    /// `Some(true)` when an adapter knows the texture is tinted by the engine (grayscale
    /// origin): it then gets lightness changes only. `None` lets stages detect it.
    pub tint_safe: Option<bool>,
}

impl Image {
    /// The pixels as a flat `f32` slice, ready for upload to the GPU.
    pub fn as_f32(&self) -> &[f32] {
        self.pixels.as_flattened()
    }

    pub fn as_f32_mut(&mut self) -> &mut [f32] {
        self.pixels.as_flattened_mut()
    }
}

/// PNG color type of the source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceColor {
    Gray,
    GrayAlpha,
    Rgb,
    Rgba,
    Indexed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceFormat {
    pub color: SourceColor,
    /// Bits per sample (per palette index for indexed images): 1, 2, 4, 8 or 16.
    pub bit_depth: u8,
    /// True if the source had an alpha channel or a `tRNS` transparency chunk.
    pub has_alpha: bool,
}

/// Channel layout of an encoded output file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channels {
    Gray,
    GrayAlpha,
    Rgb,
    Rgba,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputFormat {
    pub channels: Channels,
    pub sixteen_bit: bool,
}

impl SourceFormat {
    /// The encoding that preserves the source's color model, alpha presence and bit depth.
    ///
    /// Exceptions: palette-indexed images are written as RGB8/RGBA8 (a stylized image no longer
    /// fits the original palette), gray below 8 bits is written as 8-bit gray, and `tRNS`
    /// transparency becomes a real alpha channel.
    pub fn output(&self) -> OutputFormat {
        let gray = matches!(self.color, SourceColor::Gray | SourceColor::GrayAlpha);
        let channels = match (gray, self.has_alpha) {
            (true, false) => Channels::Gray,
            (true, true) => Channels::GrayAlpha,
            (false, false) => Channels::Rgb,
            (false, true) => Channels::Rgba,
        };
        OutputFormat {
            channels,
            sixteen_bit: self.bit_depth == 16,
        }
    }
}
