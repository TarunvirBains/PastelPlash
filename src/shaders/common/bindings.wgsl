// PastelPlash stylization passes. See ARCHITECTURE.md for the pass graph and PLAN.md for intent.
//
// All textures are rgba32float holding gamma-encoded sRGB with straight alpha. Each pass writes
// `out`; passes are dispatched over row bands [band.y0, band.y1).
//
// The shader is these files concatenated in a fixed order (`SHADER` in src/stylize/mod.rs):
// common/ first (this file: the uniform block and the bindings every pass shares), then one
// file per stage.

struct Params {
    size_x: i32, size_y: i32, origin_x: i32, origin_y: i32,
    full_x: i32, full_y: i32, wrap_x: i32, wrap_y: i32,
    tile_x: i32, tile_y: i32, low_w: i32, low_h: i32,
    lut_size: i32, tint_safe: i32, seed: u32, _pad0: i32,

    delight_strength: f32, delight_min: f32, delight_max: f32, delight_mean: f32,
    kuw_radius: f32, kuw_q: f32, kuw_hardness: f32, kuw_alpha: f32,
    kuw_zero_cross: f32, kuw_strength: f32, tensor_sigma: f32, edge_dark: f32,
    edge_step: f32, bleed: f32, bleed_radius: f32, bleed_range: f32,
    gran: f32, gran_cells_x: f32, gran_cells_y: f32, gran_valley: f32,
    gran_radius: f32, paper: f32, paper_cells_x: f32, paper_cells_y: f32,
    paper_hl: f32, paper_r: f32, paper_g: f32, paper_b: f32,
    floor_margin: f32, ceiling: f32, accent_fraction: f32, accent_softness: f32,
    accent_radius: f32, accent_min_l: f32, accent_hue: f32, accent_chroma: f32,
    accent_depth: f32, temp_strength: f32, temp_warm_hue: f32, temp_cool_hue: f32,
    temp_sens: f32, stroke_strength: f32, stroke_chroma: f32, stroke_len: f32,
    stroke_step: f32, stroke_cells_x: f32, stroke_cells_y: f32, bloom_cells_x: f32,
    bloom_cells_y: f32, ceiling_knee: f32, accent_min_depth: f32, edge_rel: f32,
    edge_threshold: f32, edge_feather: f32, paper_tint: f32, smear: f32,
    vc_fine: f32, vc_mid: f32, vc_coarse: f32, vc_chroma: f32,
    vc_r_fine: f32, vc_r_mid: f32, vc_r_coarse: f32, vc_range: f32,
    amp: f32, busy: f32, kuw_radius_coarse: f32, edge_coarse_step: f32,
    edge_soften: f32, highlight_calm: f32, highlight_radius: f32, chroma_retain: f32,
    mean_l: f32, mean_a: f32, mean_b: f32, spread: f32,
    pivot_r: f32, grp: f32, grp_sigma: f32, grp_radius: f32,
    grp_range: f32, grp_color: f32, grp_family: f32, grp_stroke: f32,
    grp_sal0: f32, grp_sal1: f32, grp_count: f32, _pad10: f32,
    grp_l: vec4<f32>, grp_a: vec4<f32>, grp_b: vec4<f32>,
    cast_s: f32, cast_hue: f32, cast_fr: f32, cast_fg: f32,
    cast_exposure: f32, cast_chroma: f32, cast_dark_cap: f32, cast_dark_min: f32,
    cast_dark_chroma: f32, cast_pivot: f32, cast_dark_below: f32, cast_warm0: f32,
    cast_warm1: f32, cast_on: f32, cast_fb: f32, cast_black0: f32,
    ts_on: f32, ts_shift: f32, ts_amp: f32, ts_max: f32,
    speck_r: f32, speck_thr: f32, clip_r: f32, thin_r: f32,
    thin_amount: f32, cast_black1: f32, cast_black_chroma: f32, _pad17: f32,
    tc_amount: f32, tc_hue: f32, tc_mean: f32, tc_gather: f32,
    tc_band0: f32, tc_band1: f32, tc_feather: f32, tc_min_c: f32,
    tc_boost: f32, _pad18: f32, _pad19: f32, _pad20: f32,
    df_on: f32, df_chroma: f32, df_below: f32, df_hue: f32,
    df_tint: f32, df_tint_chroma: f32, df_tint_below: f32, df_cool_bias: f32,
    df_cool_hue: f32, ctx_r: f32, ctx_neutral: f32, ctx_gain: f32,
    ctx_warm0: f32, ctx_warm1: f32, cast_stone: f32, _pad22: f32,
};

struct Band { y0: i32, y1: i32, _a: i32, _b: i32 };

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var texA: texture_2d<f32>;
@group(0) @binding(2) var texB: texture_2d<f32>;
@group(0) @binding(3) var outTex: texture_storage_2d<rgba32float, write>;
@group(0) @binding(4) var<storage, read> lut: array<vec4<f32>>;
@group(0) @binding(5) var<storage, read> lowres: array<f32>;
@group(0) @binding(6) var<uniform> band: Band;
@group(0) @binding(7) var texC: texture_2d<f32>;
// 256-bin histogram of the accent measure, then [256] lo and [257] hi thresholds (f32 bits).
@group(0) @binding(8) var<storage, read_write> hist: array<atomic<u32>, 260>;
// The original upload (T0, never written): the source texels before any pass.
@group(0) @binding(9) var texD: texture_2d<f32>;
