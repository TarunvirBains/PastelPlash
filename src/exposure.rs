//! Exposure preservation: a smooth, monotone tone curve, solved per image, that restores the
//! source's alpha-weighted mean lightness after stylization.
//!
//! Pre-rendered backgrounds carry the designers' lighting and mood; the palette's murk lift makes
//! their crushed darks readable, which also brightens the room. The curve keeps that lift (it
//! leaves the darks almost untouched) and brings mids and lights down just enough that the room's
//! overall exposure matches the source:
//!
//! `f(L) = L − k · s(L) · (1 − L)`, with `s = smoothstep(protect[0], protect[1], L)`.
//!
//! `f(0) = 0`, `f(1) = 1`, and `k` is bounded so the slope stays at least [`MIN_SLOPE`]: value
//! order is always preserved and no range collapses into a band.

use crate::color;

/// Smallest slope the curve may have anywhere (monotone, no banding).
pub const MIN_SLOPE: f32 = 0.5;

/// A solved curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToneCurve {
    pub k: f32,
    pub protect: [f32; 2],
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0).max(1e-6)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl ToneCurve {
    /// The bump the curve subtracts (zero in the protected darks and at white).
    pub fn bump(protect: [f32; 2], l: f32) -> f32 {
        smoothstep(protect[0], protect[1], l) * (1.0 - l)
    }

    pub fn apply(&self, l: f32) -> f32 {
        let l = l.clamp(0.0, 1.0);
        (l - self.k * Self::bump(self.protect, l)).clamp(0.0, 1.0)
    }

    /// The range of `k` that keeps the slope at least [`MIN_SLOPE`].
    pub fn k_bounds(protect: [f32; 2]) -> (f32, f32) {
        let n = 1024;
        let (mut dmax, mut dmin) = (0.0f32, 0.0f32);
        for i in 0..n {
            let (a, b) = (i as f32 / n as f32, (i + 1) as f32 / n as f32);
            let d = (Self::bump(protect, b) - Self::bump(protect, a)) * n as f32;
            dmax = dmax.max(d);
            dmin = dmin.min(d);
        }
        // A little margin for the sampled derivative.
        let room = (1.0 - MIN_SLOPE) * 0.98;
        (
            if dmin < 0.0 { room / dmin } else { f32::MIN },
            if dmax > 0.0 { room / dmax } else { f32::MAX },
        )
    }
}

/// Alpha-weighted mean OKLab lightness of sRGB pixels.
pub fn mean_l(pixels: &[[f32; 4]]) -> f32 {
    let (mut s, mut w) = (0.0f64, 0.0f64);
    for p in pixels {
        s += (color::srgb_to_oklab([p[0], p[1], p[2]])[0] * p[3]) as f64;
        w += p[3] as f64;
    }
    if w > 0.0 { (s / w) as f32 } else { 0.0 }
}

/// Applies `curve` to the lightness of `pixels` (OKLab a/b kept; gamut-clamped).
pub fn apply(pixels: &mut [[f32; 4]], curve: &ToneCurve) {
    for p in pixels.iter_mut() {
        let [l, a, b] = color::srgb_to_oklab([p[0], p[1], p[2]]);
        let rgb = color::oklab_to_srgb([curve.apply(l), a, b]);
        for (c, v) in p.iter_mut().zip(rgb) {
            *c = v.clamp(0.0, 1.0);
        }
    }
}

/// Solves the curve that brings `out`'s mean lightness back to `target`, within the monotone
/// bounds, and applies it. Returns the curve.
pub fn preserve_mean(out: &mut [[f32; 4]], target: f32, protect: [f32; 2]) -> ToneCurve {
    let (lo, hi) = ToneCurve::k_bounds(protect);
    let src: Vec<[f32; 4]> = out.to_vec();
    let (mut bsum, mut w) = (0.0f64, 0.0f64);
    for p in &src {
        let l = color::srgb_to_oklab([p[0], p[1], p[2]])[0];
        bsum += (ToneCurve::bump(protect, l) * p[3]) as f64;
        w += p[3] as f64;
    }
    let mb = if w > 0.0 { (bsum / w) as f32 } else { 0.0 };
    let mut curve = ToneCurve { k: 0.0, protect };
    if mb <= 1e-4 {
        return curve;
    }
    // Linear in k up to gamut clamping; a few corrections absorb that.
    let mut current = mean_l(&src);
    for _ in 0..4 {
        curve.k = (curve.k + (current - target) / mb).clamp(lo, hi);
        let mut trial = src.clone();
        apply(&mut trial, &curve);
        current = mean_l(&trial);
        if (current - target).abs() < 5e-4 {
            break;
        }
    }
    out.copy_from_slice(&src);
    apply(out, &curve);
    curve
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curve_is_monotone_within_bounds() {
        let protect = [0.15, 0.75];
        let (lo, hi) = ToneCurve::k_bounds(protect);
        for k in [lo, 0.0, hi] {
            let c = ToneCurve { k, protect };
            let mut prev = -1.0;
            for i in 0..=1000 {
                let v = c.apply(i as f32 / 1000.0);
                assert!(v >= prev, "k {k}: not monotone at {i}");
                prev = v;
            }
            assert_eq!(c.apply(0.0), 0.0);
            assert!((c.apply(1.0) - 1.0).abs() < 1e-6);
        }
    }

    #[test]
    fn restores_mean_of_a_brightened_image() {
        let mut px: Vec<[f32; 4]> = (0..1000)
            .map(|i| {
                let v = 0.1 + 0.8 * i as f32 / 1000.0;
                [v, v, v, 1.0]
            })
            .collect();
        let target = mean_l(&px);
        for p in px.iter_mut() {
            for c in p.iter_mut().take(3) {
                *c = (*c + 0.05).min(1.0);
            }
        }
        preserve_mean(&mut px, target, [0.15, 0.75]);
        assert!((mean_l(&px) - target).abs() < 1e-3);
    }
}
