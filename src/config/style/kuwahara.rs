//! `[kuwahara]`: the painterly edge-preserving filter.

use serde::Deserialize;

use anyhow::Result;

use crate::config::{check, non_negative, unit};

/// Anisotropic Kuwahara filter with polynomial sector weights (Kyprianidis et al.).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Kuwahara {
    /// Filter radius in texels at the reference size; 0 disables the filter.
    pub radius: f32,
    /// Blend between the de-lit input (0) and the filtered result (1).
    pub strength: f32,
    /// Sector selection sharpness `q`; higher keeps edges crisper.
    pub sharpness: f32,
    /// Sector variance scale; higher prefers flat sectors more strongly.
    pub hardness: f32,
    /// Anisotropy tuning `α`; larger keeps ellipses rounder.
    pub anisotropy: f32,
    /// Polynomial weight zero crossing (radians-ish, ~0.58 ≈ 8-sector overlap).
    pub zero_crossing: f32,
    /// Structure-tensor smoothing sigma in texels at the reference size.
    pub tensor_sigma: f32,
    /// Hard cap on the scaled radius (cost grows with its square).
    pub max_radius: f32,
    /// Floor on the scaled radius so small textures still get painted.
    pub min_radius: f32,
}

impl Default for Kuwahara {
    fn default() -> Self {
        Self {
            radius: 0.0,
            strength: 1.0,
            sharpness: 8.0,
            hardness: 8.0,
            anisotropy: 1.0,
            zero_crossing: 0.58,
            tensor_sigma: 2.0,
            max_radius: 24.0,
            min_radius: 2.0,
        }
    }
}

impl Kuwahara {
    pub fn validate(&self) -> Result<()> {
        let k = self;
        non_negative("kuwahara.radius", k.radius)?;
        unit("kuwahara.strength", k.strength)?;
        check(k.min_radius <= k.max_radius, || {
            format!(
                "kuwahara.min_radius ({}) is above kuwahara.max_radius ({})",
                k.min_radius, k.max_radius
            )
        })?;
        check(k.anisotropy > 0.0, || {
            "kuwahara.anisotropy must be > 0".into()
        })?;
        Ok(())
    }
}
