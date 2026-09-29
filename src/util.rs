//! Small helpers shared across modules.

use std::time::Duration;

/// A duration as whole milliseconds, for log lines.
pub fn ms(d: Duration) -> String {
    format!("{:.0}ms", d.as_secs_f64() * 1000.0)
}

/// Synthetic images for unit tests.
#[cfg(test)]
pub(crate) mod test_images {
    use crate::image::{Image, SourceColor, SourceFormat};

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
    pub fn noise(x: u32, y: u32) -> f32 {
        let mut v = x.wrapping_mul(0x9E37_79B9) ^ y.wrapping_mul(0x85EB_CA6B);
        v ^= v >> 15;
        v = v.wrapping_mul(0x2C1B_3C6D);
        v ^= v >> 12;
        (v & 0xFFFF) as f32 / 65535.0
    }
}
