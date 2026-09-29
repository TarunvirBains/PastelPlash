//! The uniform block shared by every stylize pass (`Params` in the shader).

use bytemuck::{Pod, Zeroable};

/// Declares a `#[repr(C)]` uniform struct plus, for tests, its field offsets by name (checked
/// against the WGSL declaration of the same struct).
macro_rules! uniform_struct {
    ($(#[$meta:meta])* $vis:vis struct $name:ident { $($field:ident: $ty:ty,)* }) => {
        $(#[$meta])*
        #[repr(C)]
        $vis struct $name { $($vis $field: $ty,)* }

        #[cfg(test)]
        impl $name {
            const FIELDS: &[(&str, usize)] =
                &[$((stringify!($field), std::mem::offset_of!($name, $field)),)*];
        }
    };
}

uniform_struct! {
#[derive(Debug, Clone, Copy, Default, Pod, Zeroable)]
pub(super) struct Params {
    size_x: i32,
    size_y: i32,
    origin_x: i32,
    origin_y: i32,
    full_x: i32,
    full_y: i32,
    wrap_x: i32,
    wrap_y: i32,
    tile_x: i32,
    tile_y: i32,
    low_w: i32,
    low_h: i32,
    lut_size: i32,
    tint_safe: i32,
    seed: u32,
    _pad0: i32,

    delight_strength: f32,
    delight_min: f32,
    delight_max: f32,
    delight_mean: f32,
    kuw_radius: f32,
    kuw_q: f32,
    kuw_hardness: f32,
    kuw_alpha: f32,
    kuw_zero_cross: f32,
    kuw_strength: f32,
    tensor_sigma: f32,
    edge_dark: f32,
    edge_step: f32,
    bleed: f32,
    bleed_radius: f32,
    bleed_range: f32,
    gran: f32,
    gran_cells_x: f32,
    gran_cells_y: f32,
    gran_valley: f32,
    gran_radius: f32,
    paper: f32,
    paper_cells_x: f32,
    paper_cells_y: f32,
    paper_hl: f32,
    paper_r: f32,
    paper_g: f32,
    paper_b: f32,
    floor_margin: f32,
    ceiling: f32,
    accent_fraction: f32,
    accent_softness: f32,
    accent_radius: f32,
    accent_min_l: f32,
    accent_hue: f32,
    accent_chroma: f32,
    accent_depth: f32,
    temp_strength: f32,
    temp_warm_hue: f32,
    temp_cool_hue: f32,
    temp_sens: f32,
    stroke_strength: f32,
    stroke_chroma: f32,
    stroke_len: f32,
    stroke_step: f32,
    stroke_cells_x: f32,
    stroke_cells_y: f32,
    bloom_cells_x: f32,
    bloom_cells_y: f32,
    ceiling_knee: f32,
    accent_min_depth: f32,
    edge_rel: f32,
    edge_threshold: f32,
    edge_feather: f32,
    paper_tint: f32,
    smear: f32,
    vc_fine: f32,
    vc_mid: f32,
    vc_coarse: f32,
    vc_chroma: f32,
    vc_r_fine: f32,
    vc_r_mid: f32,
    vc_r_coarse: f32,
    vc_range: f32,
    amp: f32,
    busy: f32,
    kuw_radius_coarse: f32,
    edge_coarse_step: f32,
    edge_soften: f32,
    highlight_calm: f32,
    highlight_radius: f32,
    chroma_retain: f32,
    mean_l: f32,
    mean_a: f32,
    mean_b: f32,
    spread: f32,
    pivot_r: f32,
    grp: f32,
    grp_sigma: f32,
    grp_radius: f32,
    grp_range: f32,
    grp_color: f32,
    grp_family: f32,
    grp_stroke: f32,
    grp_sal0: f32,
    grp_sal1: f32,
    grp_count: f32,
    _pad10: f32,
    grp_l: [f32; 4],
    grp_a: [f32; 4],
    grp_b: [f32; 4],

    cast_s: f32,
    cast_hue: f32,
    cast_tint: f32,
    cast_tint_l: f32,
    cast_exposure: f32,
    cast_chroma: f32,
    cast_dark_cap: f32,
    cast_dark_min: f32,
    cast_dark_chroma: f32,
    cast_pivot: f32,
    cast_dark_below: f32,
    cast_warm0: f32,
    cast_warm1: f32,
    cast_on: f32,
    _pad12: f32,
    _pad13: f32,
}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stylize::SHADER;

    /// Member offsets of a WGSL struct in `SHADER`, by name, and its size.
    fn wgsl_layout(name: &str) -> (Vec<(String, usize)>, usize) {
        let module = naga::front::wgsl::parse_str(SHADER).unwrap();
        let (members, span) = module
            .types
            .iter()
            .find_map(|(_, ty)| match &ty.inner {
                naga::TypeInner::Struct { members, span } if ty.name.as_deref() == Some(name) => {
                    Some((members.clone(), *span))
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("no struct {name} in the shader"));
        let members = members
            .iter()
            .map(|m| (m.name.clone().unwrap(), m.offset as usize))
            .collect();
        (members, span as usize)
    }

    #[test]
    fn params_layout_matches_the_shader() {
        let (wgsl, span) = wgsl_layout("Params");
        let rust: Vec<(String, usize)> = Params::FIELDS
            .iter()
            .map(|&(n, o)| (n.to_string(), o))
            .collect();
        assert_eq!(rust, wgsl, "Params fields (name, offset): Rust vs WGSL");
        assert_eq!(std::mem::size_of::<Params>(), span, "Params size");
    }
}
