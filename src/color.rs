//! Color-space conversions: gamma sRGB ↔ linear sRGB ↔ OKLab ↔ OKLCH (Björn Ottosson's OKLab).
//!
//! The WGSL shaders carry the same formulas (see `src/shaders/stylize.wgsl`).

/// sRGB transfer function, gamma-encoded → linear.
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// sRGB transfer function, linear → gamma-encoded.
pub fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.max(0.0).powf(1.0 / 2.4) - 0.055
    }
}

/// Linear sRGB → OKLab `[L, a, b]`.
pub fn linear_to_oklab([r, g, b]: [f32; 3]) -> [f32; 3] {
    let l = 0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b;
    let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
    let s = 0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b;
    let (l, m, s) = (l.cbrt(), m.cbrt(), s.cbrt());
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

/// OKLab `[L, a, b]` → linear sRGB (may be out of gamut).
pub fn oklab_to_linear([ll, a, b]: [f32; 3]) -> [f32; 3] {
    let l = ll + 0.396_337_78 * a + 0.215_803_76 * b;
    let m = ll - 0.105_561_346 * a - 0.063_854_17 * b;
    let s = ll - 0.089_484_18 * a - 1.291_485_5 * b;
    let (l, m, s) = (l * l * l, m * m * m, s * s * s);
    [
        4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s,
        -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s,
        -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s,
    ]
}

/// Gamma sRGB → OKLab.
pub fn srgb_to_oklab(rgb: [f32; 3]) -> [f32; 3] {
    linear_to_oklab(rgb.map(srgb_to_linear))
}

/// OKLab → gamma sRGB (not clamped).
pub fn oklab_to_srgb(lab: [f32; 3]) -> [f32; 3] {
    oklab_to_linear(lab).map(linear_to_srgb)
}

/// OKLab → OKLCH `[L, C, h°]` with `h` in `0..360`.
pub fn oklab_to_oklch([l, a, b]: [f32; 3]) -> [f32; 3] {
    let h = b.atan2(a).to_degrees();
    [l, a.hypot(b), if h < 0.0 { h + 360.0 } else { h }]
}

/// OKLCH `[L, C, h°]` → OKLab.
pub fn oklch_to_oklab([l, c, h]: [f32; 3]) -> [f32; 3] {
    let (s, co) = h.to_radians().sin_cos();
    [l, c * co, c * s]
}

/// True if a linear sRGB color lies inside the unit cube (with a small tolerance).
pub fn in_gamut(rgb: [f32; 3]) -> bool {
    rgb.iter().all(|&c| (-1e-4..=1.0 + 1e-4).contains(&c))
}

/// Maps an OKLCH color into the sRGB gamut by lowering chroma at constant L and hue, and returns
/// it as gamma sRGB clamped to `0..=1`.
pub fn oklch_to_srgb_gamut([l, c, h]: [f32; 3]) -> [f32; 3] {
    let l = l.clamp(0.0, 1.0);
    let lin = |c: f32| oklab_to_linear(oklch_to_oklab([l, c, h]));
    let mut rgb = lin(c);
    if !in_gamut(rgb) {
        let (mut lo, mut hi) = (0.0f32, c);
        for _ in 0..24 {
            let mid = 0.5 * (lo + hi);
            if in_gamut(lin(mid)) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        rgb = lin(lo);
    }
    rgb.map(|v| linear_to_srgb(v.clamp(0.0, 1.0)).clamp(0.0, 1.0))
}

/// Signed shortest angular difference `to - from` in degrees, in `-180..180`.
pub fn hue_diff(from: f32, to: f32) -> f32 {
    (to - from + 540.0).rem_euclid(360.0) - 180.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 3], b: [f32; 3], eps: f32) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() <= eps)
    }

    #[test]
    fn srgb_transfer_round_trips() {
        for i in 0..=255 {
            let c = i as f32 / 255.0;
            assert!((linear_to_srgb(srgb_to_linear(c)) - c).abs() < 1e-5);
        }
    }

    #[test]
    fn oklab_known_values() {
        // White is L = 1 with no chroma; reference values from Ottosson's post.
        assert!(close(
            linear_to_oklab([1.0, 1.0, 1.0]),
            [1.0, 0.0, 0.0],
            1e-3
        ));
        assert!(close(
            linear_to_oklab([1.0, 0.0, 0.0]),
            [0.627_955, 0.224_863, 0.125_846],
            1e-3
        ));
    }

    #[test]
    fn oklab_and_oklch_round_trip() {
        for r in 0..8 {
            for g in 0..8 {
                for b in 0..8 {
                    let rgb = [r, g, b].map(|v| v as f32 / 7.0);
                    let lab = srgb_to_oklab(rgb);
                    assert!(close(oklab_to_srgb(lab), rgb, 1e-4), "{rgb:?}");
                    let lch = oklab_to_oklch(lab);
                    assert!(close(oklch_to_oklab(lch), lab, 1e-5));
                    assert!(close(oklch_to_srgb_gamut(lch), rgb, 1e-3), "{rgb:?}");
                }
            }
        }
    }

    #[test]
    fn gamut_mapping_keeps_lightness_and_hue() {
        let out = oklch_to_srgb_gamut([0.7, 0.4, 140.0]);
        let lch = oklab_to_oklch(srgb_to_oklab(out));
        assert!((lch[0] - 0.7).abs() < 2e-3);
        assert!(hue_diff(lch[2], 140.0).abs() < 1.0);
        assert!(lch[1] < 0.4);
    }

    #[test]
    fn hue_differences_wrap() {
        assert_eq!(hue_diff(350.0, 10.0), 20.0);
        assert_eq!(hue_diff(10.0, 350.0), -20.0);
        assert_eq!(hue_diff(90.0, 90.0), 0.0);
    }
}
