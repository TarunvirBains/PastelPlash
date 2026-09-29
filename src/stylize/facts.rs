//! What the CPU analysis finds out about one image: its size and scale, whether it tiles,
//! whether it is tint-safe, and (on demand) its low-res luminance field, speckle and value
//! spread. Stage plans read these facts; none of them measures the image itself.

use crate::analysis::{self, Lowres};
use crate::config::{Style, Treatment};
use crate::image::Image;
use crate::pipeline::FileContext;

pub(super) struct ImageFacts {
    pub w: u32,
    pub h: u32,
    /// Geometric-mean side `sqrt(w·h)`.
    pub gm: f32,
    /// Size factor: reference texels → texels of this image.
    pub scale: f32,
    /// Seam-to-interior discontinuity per axis.
    pub seam_ratios: [f32; 2],
    /// Axes that wrap (seamless tiling).
    pub wrap: [bool; 2],
    /// 99th-percentile OKLab chroma.
    pub chroma_p99: f32,
    /// Lightness changes only (engine-tinted grayscale).
    pub tint_safe: bool,
}

impl ImageFacts {
    pub fn analyze(image: &Image, ctx: &FileContext, style: &Style, tr: &Treatment) -> Self {
        let (w, h) = (image.width, image.height);
        let gm = ((w as f64) * (h as f64)).sqrt() as f32;
        let source_scale = image
            .source_scale
            .or(ctx.config.pack.source_scale)
            .or(style.scale.source_scale);
        let scale = match source_scale {
            Some(s) => s / style.scale.reference_source_scale.max(1e-3),
            None => (gm / style.scale.reference_size.max(1.0)).powf(style.scale.exponent),
        };
        let seam_ratios = [
            analysis::seam_ratio(image, false),
            analysis::seam_ratio(image, true),
        ];
        let wrap = if ctx.category.may_tile() {
            seam_ratios.map(|r| r <= style.tiling.threshold)
        } else {
            [false; 2]
        };
        let chroma_p99 = analysis::chroma_p99(image);
        let tint_safe = tr
            .tint_safe
            .or(image.tint_safe)
            .unwrap_or(chroma_p99 < style.palette.tint_safe_chroma);
        Self {
            w,
            h,
            gm,
            scale,
            seam_ratios,
            wrap,
            chroma_p99,
            tint_safe,
        }
    }

    /// The low-res luminance field (de-light, temperature, adaptive-contrast pivot).
    pub fn lowres(&self, image: &Image, style: &Style) -> Lowres {
        analysis::lowres_luminance(image, style.delight.radius * self.gm, self.wrap)
    }

    /// Median lightness std over windows of `radius` texels.
    pub fn l_std(&self, image: &Image, radius: f32) -> f32 {
        analysis::local_l_std(image, radius, self.wrap)
    }
}
