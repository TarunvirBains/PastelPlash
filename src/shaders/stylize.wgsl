// PastelPlash stylization passes. See src/stylize.rs for the pass graph and PLAN.md for intent.
//
// All textures are rgba32float holding gamma-encoded sRGB with straight alpha. Each pass writes
// `out`; passes are dispatched over row bands [band.y0, band.y1).

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
    pivot_r: f32, _pad7: f32, _pad8: f32, _pad9: f32,
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

// ---------------------------------------------------------------- addressing

fn wrapi(v: i32, n: i32) -> i32 {
    return ((v % n) + n) % n;
}

fn addr(p: vec2<i32>) -> vec2<i32> {
    var q = p;
    if (P.wrap_x != 0) { q.x = wrapi(q.x, P.size_x); } else { q.x = clamp(q.x, 0, P.size_x - 1); }
    if (P.wrap_y != 0) { q.y = wrapi(q.y, P.size_y); } else { q.y = clamp(q.y, 0, P.size_y - 1); }
    return q;
}

fn loadA(p: vec2<i32>) -> vec4<f32> { return textureLoad(texA, addr(p), 0); }
fn loadB(p: vec2<i32>) -> vec4<f32> { return textureLoad(texB, addr(p), 0); }
fn loadC(p: vec2<i32>) -> vec4<f32> { return textureLoad(texC, addr(p), 0); }

// Texel center in full-image coordinates (not wrapped; noise is periodic by construction).
fn gpos(p: vec2<i32>) -> vec2<f32> {
    return vec2<f32>(f32(p.x + P.origin_x), f32(p.y + P.origin_y)) + 0.5;
}

fn pixel(gid: vec3<u32>) -> vec2<i32> {
    return vec2<i32>(i32(gid.x), i32(gid.y) + band.y0);
}

fn outside(p: vec2<i32>) -> bool {
    return p.x >= P.size_x || p.y >= band.y1 || p.y >= P.size_y;
}

// ---------------------------------------------------------------- color

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((max(c, vec3<f32>(0.0)) + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

fn cbrt(x: f32) -> f32 {
    return sign(x) * pow(abs(x), 1.0 / 3.0);
}

fn linear_to_oklab(c: vec3<f32>) -> vec3<f32> {
    let l = cbrt(0.4122214708 * c.r + 0.5363325363 * c.g + 0.0514459929 * c.b);
    let m = cbrt(0.2119034982 * c.r + 0.6806995451 * c.g + 0.1073969566 * c.b);
    let s = cbrt(0.0883024619 * c.r + 0.2817188376 * c.g + 0.6299787005 * c.b);
    return vec3<f32>(
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s);
}

fn oklab_to_linear(c: vec3<f32>) -> vec3<f32> {
    let l0 = c.x + 0.3963377774 * c.y + 0.2158037573 * c.z;
    let m0 = c.x - 0.1055613458 * c.y - 0.0638541728 * c.z;
    let s0 = c.x - 0.0894841775 * c.y - 1.2914855480 * c.z;
    let l = l0 * l0 * l0;
    let m = m0 * m0 * m0;
    let s = s0 * s0 * s0;
    return vec3<f32>(
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s);
}

fn srgb_to_oklab(c: vec3<f32>) -> vec3<f32> { return linear_to_oklab(srgb_to_linear(c)); }

fn lightness(c: vec4<f32>) -> f32 { return srgb_to_oklab(c.rgb).x; }

fn luminance(c: vec3<f32>) -> f32 {
    let l = srgb_to_linear(c);
    return 0.2126 * l.r + 0.7152 * l.g + 0.0722 * l.b;
}

// ---------------------------------------------------------------- noise (periodic)

fn pcg(v: u32) -> u32 {
    let s = v * 747796405u + 2891336453u;
    let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}

fn hash2(x: i32, y: i32, salt: u32) -> f32 {
    return f32(pcg(u32(x) ^ pcg(u32(y) ^ pcg(P.seed ^ salt))) & 0xffffffu) / 16777215.0;
}

// Value noise over a lattice of `period` cells per axis (so it tiles with the image).
fn vnoise(pos: vec2<f32>, period: vec2<i32>, salt: u32) -> f32 {
    let i = vec2<i32>(floor(pos));
    let f = pos - floor(pos);
    let u = f * f * (3.0 - 2.0 * f);
    let x0 = wrapi(i.x, period.x);
    let x1 = wrapi(i.x + 1, period.x);
    let y0 = wrapi(i.y, period.y);
    let y1 = wrapi(i.y + 1, period.y);
    let a = mix(hash2(x0, y0, salt), hash2(x1, y0, salt), u.x);
    let b = mix(hash2(x0, y1, salt), hash2(x1, y1, salt), u.x);
    return mix(a, b, u.y);
}

// Periodic noise with `cells` base cells across the full image (0..1).
fn noise1(gp: vec2<f32>, cells: vec2<f32>, salt: u32) -> f32 {
    let full = vec2<f32>(f32(P.full_x), f32(P.full_y));
    return vnoise(gp / full * cells, vec2<i32>(cells), salt);
}

fn fbm(gp: vec2<f32>, cells: vec2<f32>, salt: u32) -> f32 {
    var sum = 0.0;
    var amp = 0.5;
    var norm = 0.0;
    var c = cells;
    for (var o = 0u; o < 3u; o++) {
        sum += amp * noise1(gp, c, salt + o * 101u);
        norm += amp;
        amp *= 0.5;
        c *= 2.0;
    }
    return sum / norm;
}

// ---------------------------------------------------------------- low-res luminance field

fn lowres_at(i: i32, j: i32) -> f32 {
    var x = i;
    var y = j;
    if (P.tile_x != 0) { x = wrapi(x, P.low_w); } else { x = clamp(x, 0, P.low_w - 1); }
    if (P.tile_y != 0) { y = wrapi(y, P.low_h); } else { y = clamp(y, 0, P.low_h - 1); }
    return lowres[y * P.low_w + x];
}

fn lowres_sample(gp: vec2<f32>) -> f32 {
    let u = gp.x / f32(P.full_x) * f32(P.low_w) - 0.5;
    let v = gp.y / f32(P.full_y) * f32(P.low_h) - 0.5;
    let x0 = i32(floor(u));
    let y0 = i32(floor(v));
    let tx = u - floor(u);
    let ty = v - floor(v);
    let top = mix(lowres_at(x0, y0), lowres_at(x0 + 1, y0), tx);
    let bot = mix(lowres_at(x0, y0 + 1), lowres_at(x0 + 1, y0 + 1), tx);
    return mix(top, bot, ty);
}

// ---------------------------------------------------------------- 1. de-light

@compute @workgroup_size(8, 8)
fn delight(@builtin(global_invocation_id) gid: vec3<u32>) {
    let p = pixel(gid);
    if (outside(p)) { return; }
    var c = textureLoad(texA, p, 0);
    if (P.delight_strength > 0.0 && P.low_w > 0) {
        let yb = lowres_sample(gpos(p));
        let gain = clamp(pow(P.delight_mean / max(yb, 1e-4), P.delight_strength),
                         P.delight_min, P.delight_max);
        var lin = srgb_to_linear(c.rgb) * gain;
        let m = max(max(lin.r, lin.g), lin.b);
        if (m > 1.0) { lin = lin / m; }
        c = vec4<f32>(linear_to_srgb(lin), c.a);
    }
    textureStore(outTex, p, c);
}

// ---------------------------------------------------------------- 2. structure tensor

fn premul(p: vec2<i32>) -> vec3<f32> {
    let c = loadA(p);
    return c.rgb * c.a;
}

@compute @workgroup_size(8, 8)
fn tensor(@builtin(global_invocation_id) gid: vec3<u32>) {
    let p = pixel(gid);
    if (outside(p)) { return; }
    let a = premul(p + vec2<i32>(-1, -1));
    let b = premul(p + vec2<i32>(0, -1));
    let c = premul(p + vec2<i32>(1, -1));
    let d = premul(p + vec2<i32>(-1, 0));
    let f = premul(p + vec2<i32>(1, 0));
    let g = premul(p + vec2<i32>(-1, 1));
    let h = premul(p + vec2<i32>(0, 1));
    let i = premul(p + vec2<i32>(1, 1));
    let gx = (c + 2.0 * f + i - a - 2.0 * d - g) * 0.25;
    let gy = (g + 2.0 * h + i - a - 2.0 * b - c) * 0.25;
    textureStore(outTex, p, vec4<f32>(dot(gx, gx), dot(gx, gy), dot(gy, gy), 1.0));
}

fn gauss_blur(p: vec2<i32>, dir: vec2<i32>) -> vec4<f32> {
    let sigma = max(P.tensor_sigma, 0.3);
    let r = i32(ceil(2.5 * sigma));
    var sum = vec4<f32>(0.0);
    var wsum = 0.0;
    for (var k = -r; k <= r; k++) {
        let w = exp(-f32(k * k) / (2.0 * sigma * sigma));
        sum += w * loadA(p + dir * k);
        wsum += w;
    }
    return sum / wsum;
}

@compute @workgroup_size(8, 8)
fn blur_h(@builtin(global_invocation_id) gid: vec3<u32>) {
    let p = pixel(gid);
    if (outside(p)) { return; }
    textureStore(outTex, p, gauss_blur(p, vec2<i32>(1, 0)));
}

@compute @workgroup_size(8, 8)
fn blur_v(@builtin(global_invocation_id) gid: vec3<u32>) {
    let p = pixel(gid);
    if (outside(p)) { return; }
    textureStore(outTex, p, gauss_blur(p, vec2<i32>(0, 1)));
}

// Minor eigenvector (edge tangent) and anisotropy of the smoothed tensor.
fn orientation(t: vec3<f32>) -> vec3<f32> {
    let e = t.x;
    let f = t.y;
    let g = t.z;
    let root = sqrt(max(g * g - 2.0 * e * g + e * e + 4.0 * f * f, 0.0));
    let l1 = 0.5 * (g + e + root);
    let l2 = 0.5 * (g + e - root);
    let v = vec2<f32>(l1 - e, -f);
    let len = length(v);
    var dir = vec2<f32>(0.0, 1.0);
    if (len > 1e-12) { dir = v / len; }
    var aniso = 0.0;
    if (l1 + l2 > 1e-12) { aniso = (l1 - l2) / (l1 + l2); }
    return vec3<f32>(dir, aniso);
}

// ---------------------------------------------------------------- 3. anisotropic Kuwahara

@compute @workgroup_size(8, 8)
fn kuwahara(@builtin(global_invocation_id) gid: vec3<u32>) {
    let p = pixel(gid);
    if (outside(p)) { return; }
    let center = textureLoad(texA, p, 0);
    // band._a == 1: the large-scale abstraction pass (busy, photographic textures only).
    let coarse = band._a == 1;
    let radius = select(P.kuw_radius, P.kuw_radius_coarse, coarse);
    let strength = select(P.kuw_strength, P.busy, coarse);
    if (radius < 0.5 || strength <= 0.0) {
        textureStore(outTex, p, center);
        return;
    }
    let o = orientation(textureLoad(texB, p, 0).xyz);
    let phi = -atan2(o.y, o.x);
    let aniso = o.z;
    let alpha = P.kuw_alpha;
    let a = radius * clamp((alpha + aniso) / alpha, 0.1, 2.0);
    let b = radius * clamp(alpha / (alpha + aniso), 0.1, 2.0);
    let cp = cos(phi);
    let sp = sin(phi);
    let max_x = i32(sqrt(a * a * cp * cp + b * b * sp * sp));
    let max_y = i32(sqrt(a * a * sp * sp + b * b * cp * cp));
    let zeta = 1.0 / radius;
    let zc = P.kuw_zero_cross;
    let eta = (zeta + cos(zc)) / (sin(zc) * sin(zc));

    var m: array<vec4<f32>, 8>;
    var s: array<vec3<f32>, 8>;
    for (var k = 0; k < 8; k++) {
        m[k] = vec4<f32>(0.0);
        s[k] = vec3<f32>(0.0);
    }
    for (var y = -max_y; y <= max_y; y++) {
        for (var x = -max_x; x <= max_x; x++) {
            let fx = f32(x);
            let fy = f32(y);
            var v = vec2<f32>((cp * fx - sp * fy) * 0.5 / a, (sp * fx + cp * fy) * 0.5 / b);
            if (dot(v, v) > 0.25) { continue; }
            let smp = loadA(p + vec2<i32>(x, y));
            if (smp.a <= 0.0) { continue; }
            let c = clamp(smp.rgb, vec3<f32>(0.0), vec3<f32>(1.0));
            var w: array<f32, 8>;
            var sum = 0.0;
            var vxx = zeta - eta * v.x * v.x;
            var vyy = zeta - eta * v.y * v.y;
            var z = max(0.0, v.y + vxx); w[0] = z * z; sum += w[0];
            z = max(0.0, -v.x + vyy); w[2] = z * z; sum += w[2];
            z = max(0.0, -v.y + vxx); w[4] = z * z; sum += w[4];
            z = max(0.0, v.x + vyy); w[6] = z * z; sum += w[6];
            v = 0.70710678 * vec2<f32>(v.x - v.y, v.x + v.y);
            vxx = zeta - eta * v.x * v.x;
            vyy = zeta - eta * v.y * v.y;
            z = max(0.0, v.y + vxx); w[1] = z * z; sum += w[1];
            z = max(0.0, -v.x + vyy); w[3] = z * z; sum += w[3];
            z = max(0.0, -v.y + vxx); w[5] = z * z; sum += w[5];
            z = max(0.0, v.x + vyy); w[7] = z * z; sum += w[7];
            let g = exp(-3.125 * dot(v, v)) / max(sum, 1e-8) * smp.a;
            for (var k = 0; k < 8; k++) {
                let wk = w[k] * g;
                m[k] += vec4<f32>(c * wk, wk);
                s[k] += c * c * wk;
            }
        }
    }
    var acc = vec4<f32>(0.0);
    for (var k = 0; k < 8; k++) {
        if (m[k].w <= 1e-8) { continue; }
        let mean = m[k].rgb / m[k].w;
        let var3 = abs(s[k] / m[k].w - mean * mean);
        let sigma2 = var3.r + var3.g + var3.b;
        let w = 1.0 / (1.0 + pow(P.kuw_hardness * 1000.0 * sigma2, 0.5 * P.kuw_q));
        acc += vec4<f32>(mean * w, w);
    }
    var rgb = center.rgb;
    if (acc.w > 1e-8) {
        let painted = acc.rgb / acc.w;
        // Fully transparent texels take the painted neighborhood color (fewer dark fringes
        // under bilinear filtering); others blend by strength.
        var t = select(strength, 1.0, center.a <= 0.0);
        // Coarse pass: never erase small salient objects. Where the simplification would change
        // a texel's lightness a lot (a thin stick, a hook, a bowl rim against the wall), keep
        // it; noise (small changes) and large shapes (little change) are simplified as before.
        if (coarse && center.a > 0.0) {
            let dl = abs(srgb_to_oklab(painted).x - srgb_to_oklab(center.rgb).x);
            t *= 1.0 - smoothstep(0.07, 0.16, dl);
        }
        rgb = mix(center.rgb, painted, t);
    }
    textureStore(outTex, p, vec4<f32>(rgb, center.a));
}

// ---------------------------------------------------------------- 4. edge-aware color bleeding

@compute @workgroup_size(8, 8)
fn bleed(@builtin(global_invocation_id) gid: vec3<u32>) {
    let p = pixel(gid);
    if (outside(p)) { return; }
    let c0 = textureLoad(texA, p, 0);
    if (P.bleed <= 0.0 || P.bleed_radius < 1.0 || c0.a <= 0.0) {
        textureStore(outTex, p, c0);
        return;
    }
    let lab0 = srgb_to_oklab(c0.rgb);
    let r = P.bleed_radius;
    let ri = i32(ceil(r));
    let st = max(1, i32(round(r / 3.0)));
    let sig_s = max(r * 0.5, 0.5);
    let range = max(P.bleed_range, 1e-3);
    var sum = vec3<f32>(0.0);
    var wsum = 0.0;
    for (var y = -ri; y <= ri; y += st) {
        for (var x = -ri; x <= ri; x += st) {
            let d2 = f32(x * x + y * y);
            if (d2 > r * r) { continue; }
            let q = loadA(p + vec2<i32>(x, y));
            if (q.a <= 0.0) { continue; }
            let lab = srgb_to_oklab(q.rgb);
            let dl = length(lab - lab0) / range;
            let w = q.a * exp(-d2 / (2.0 * sig_s * sig_s)) * exp(-dl * dl);
            sum += lab * w;
            wsum += w;
        }
    }
    let avg = sum / max(wsum, 1e-8);
    // Blooms: uneven, noise-modulated reach like wet-in-wet washes.
    let bloom = fbm(gpos(p), vec2<f32>(P.bloom_cells_x, P.bloom_cells_y), 17u);
    let amt = clamp(P.bleed * (0.3 + 1.4 * bloom), 0.0, 1.0);
    var lab = lab0;
    lab = vec3<f32>(mix(lab0.x, avg.x, amt * 0.35), mix(lab0.yz, avg.yz, amt));
    if (P.tint_safe != 0) { lab = vec3<f32>(lab.x, lab0.yz); }
    let rgb = linear_to_srgb(clamp(oklab_to_linear(lab), vec3<f32>(0.0), vec3<f32>(1.0)));
    textureStore(outTex, p, vec4<f32>(rgb, c0.a));
}

// ---------------------------------------------------------------- 5. palette + watercolor finish

fn lut_at(r: i32, g: i32, b: i32) -> vec4<f32> {
    let n = P.lut_size;
    return lut[r + n * (g + n * b)];
}

// Tetrahedral interpolation (keeps the gray axis exact; same math as `Lut3d::sample`).
fn lut_sample(rgb: vec3<f32>) -> vec4<f32> {
    let n = P.lut_size - 1;
    let pos = clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)) * f32(n);
    let i = min(vec3<i32>(floor(pos)), vec3<i32>(n - 1));
    let f = pos - vec3<f32>(i);
    let c000 = lut_at(i.x, i.y, i.z);
    let c111 = lut_at(i.x + 1, i.y + 1, i.z + 1);
    if (f.x > f.y) {
        if (f.y > f.z) {
            return (1.0 - f.x) * c000 + (f.x - f.y) * lut_at(i.x + 1, i.y, i.z)
                + (f.y - f.z) * lut_at(i.x + 1, i.y + 1, i.z) + f.z * c111;
        } else if (f.x > f.z) {
            return (1.0 - f.x) * c000 + (f.x - f.z) * lut_at(i.x + 1, i.y, i.z)
                + (f.z - f.y) * lut_at(i.x + 1, i.y, i.z + 1) + f.y * c111;
        } else {
            return (1.0 - f.z) * c000 + (f.z - f.x) * lut_at(i.x, i.y, i.z + 1)
                + (f.x - f.y) * lut_at(i.x + 1, i.y, i.z + 1) + f.y * c111;
        }
    } else if (f.z > f.y) {
        return (1.0 - f.z) * c000 + (f.z - f.y) * lut_at(i.x, i.y, i.z + 1)
            + (f.y - f.x) * lut_at(i.x, i.y + 1, i.z + 1) + f.x * c111;
    } else if (f.z > f.x) {
        return (1.0 - f.y) * c000 + (f.y - f.z) * lut_at(i.x, i.y + 1, i.z)
            + (f.z - f.x) * lut_at(i.x, i.y + 1, i.z + 1) + f.x * c111;
    }
    return (1.0 - f.y) * c000 + (f.y - f.x) * lut_at(i.x, i.y + 1, i.z)
        + (f.x - f.z) * lut_at(i.x + 1, i.y + 1, i.z) + f.z * c111;
}

// Mean OKLab L of the de-lit source (texB) on a ring of 8 samples at radius r.
fn ring_mean_b(p: vec2<i32>, r: f32) -> f32 {
    var s = 0.0;
    for (var k = 0; k < 8; k++) {
        let ang = f32(k) * 0.78539816;
        let o = vec2<i32>(round(vec2<f32>(cos(ang), sin(ang)) * r));
        s += lightness(loadB(p + o));
    }
    return s / 8.0;
}

fn hue_dir(h: f32) -> vec2<f32> { return vec2<f32>(cos(h), sin(h)); }

// Line-integral convolution of stroke-width noise along the flow field (texC = tensor).
fn flow(p: vec2<f32>) -> vec2<f32> {
    let q = vec2<i32>(floor(p));
    return orientation(loadC(q).xyz).xy;
}

fn strokes(p: vec2<i32>) -> f32 {
    let cells = vec2<f32>(P.stroke_cells_x, P.stroke_cells_y);
    let origin = vec2<f32>(f32(P.origin_x), f32(P.origin_y));
    let start = vec2<f32>(p) + 0.5;
    let steps = i32(clamp(ceil(P.stroke_len / P.stroke_step), 1.0, 32.0));
    var sum = noise1(start + origin, cells, 29u);
    var wsum = 1.0;
    var w2 = 1.0;
    for (var side = 0; side < 2; side++) {
        var q = start;
        var d = flow(q);
        if (side == 1) { d = -d; }
        for (var i = 1; i <= steps; i++) {
            var nd = flow(q);
            if (dot(nd, d) < 0.0) { nd = -nd; }
            d = nd;
            q += d * P.stroke_step;
            let w = 1.0 - f32(i) / f32(steps + 1);
            sum += w * noise1(q + origin, cells, 29u);
            wsum += w;
            w2 += w * w;
        }
    }
    // Zero-mean, renormalized so contrast does not depend on stroke length. Strokes only show
    // where the flow is coherent; in isotropic areas LIC of noise reads as marbling.
    let n_eff = wsum * wsum / w2;
    let flow_weight = smoothstep(0.15, 0.6, orientation(loadC(p).xyz).z);
    return clamp((sum / wsum - 0.5) * sqrt(n_eff) * 1.6, -1.0, 1.0) * flow_weight;
}

// Mean OKLab L of the painted image (texA) on a ring of 8 samples at radius r.
fn ring_mean_a(p: vec2<i32>, r: f32) -> f32 {
    var s = 0.0;
    for (var k = 0; k < 8; k++) {
        let ang = f32(k) * 0.78539816;
        let o = vec2<i32>(round(vec2<f32>(cos(ang), sin(ang)) * r));
        s += lightness(loadA(p + o));
    }
    return s / 8.0;
}

// How much darker than its surroundings a painted texel's neighborhood is (crevices, gaps;
// flat washes ~0). A band-pass (outer ring vs. inner disc) so isolated specks don't count.
fn accent_measure(p: vec2<i32>) -> f32 {
    let inner = 0.5 * lightness(loadA(p)) + 0.5 * ring_mean_a(p, max(P.accent_radius / 3.0, 1.0));
    return ring_mean_a(p, P.accent_radius) - inner;
}

const HIST_MIN: f32 = -0.1;
const HIST_SCALE: f32 = 512.0; // bins per unit L; 256 bins cover -0.1 .. 0.4

@compute @workgroup_size(8, 8)
fn accent_hist(@builtin(global_invocation_id) gid: vec3<u32>) {
    let p = pixel(gid);
    if (outside(p) || (p.x & 1) != 0 || (p.y & 1) != 0) { return; }
    if (textureLoad(texA, p, 0).a < 0.5) { return; }
    let bin = clamp(i32((accent_measure(p) - HIST_MIN) * HIST_SCALE), 0, 255);
    atomicAdd(&hist[bin], 1u);
}

// Turns the histogram into the lo/hi thresholds for the configured top fraction.
@compute @workgroup_size(1)
fn accent_threshold() {
    var total = 0u;
    for (var i = 0; i < 256; i++) { total += atomicLoad(&hist[i]); }
    let want_lo = f32(total) * P.accent_fraction;
    let want_hi = want_lo * (1.0 - clamp(P.accent_softness, 0.0, 0.95));
    var acc = 0.0;
    var lo = 1.0;
    var hi = 1.0;
    var found_hi = false;
    for (var i = 255; i >= 0; i--) {
        acc += f32(atomicLoad(&hist[i]));
        let v = f32(i) / HIST_SCALE + HIST_MIN;
        if (!found_hi && acc >= want_hi) { hi = v; found_hi = true; }
        if (acc >= want_lo) { lo = v; break; }
    }
    lo = max(lo, P.accent_min_depth);
    hi = max(hi, lo + 0.01);
    atomicStore(&hist[256], bitcast<u32>(lo));
    atomicStore(&hist[257], bitcast<u32>(hi));
}

// Edge-aware local mean of the painted lightness around `p` (center plus two rings at r and
// r/2): samples across a strong lightness step get little weight, so structural edges survive
// value compression.
fn local_mean_a(p: vec2<i32>, r: f32, l0: f32) -> f32 {
    var sum = l0;
    var wsum = 1.0;
    let sigma = max(P.vc_range, 1e-3);
    for (var ring = 0; ring < 2; ring++) {
        let rr = select(r, r * 0.5, ring == 1);
        for (var k = 0; k < 8; k++) {
            let ang = (f32(k) + 0.5 * f32(ring)) * 0.78539816;
            let q = loadA(p + vec2<i32>(round(vec2<f32>(cos(ang), sin(ang)) * rr)));
            if (q.a <= 0.0) { continue; }
            let l = lightness(q);
            let d = (l - l0) / sigma;
            let w = exp(-d * d);
            sum += w * l;
            wsum += w;
        }
    }
    return sum / wsum;
}

// Brushstrokes from the texture itself: the de-lit source color (texB) averaged along the flow
// (line-integral convolution), so detail comes back as streaks that follow form. Never across
// an edge: the path follows the edge tangent.
fn smear_color(p: vec2<i32>) -> vec3<f32> {
    let start = vec2<f32>(p) + 0.5;
    let steps = i32(clamp(ceil(P.stroke_len / P.stroke_step), 1.0, 32.0));
    var sum = mix(textureLoad(texB, p, 0).rgb, textureLoad(texA, p, 0).rgb, P.busy);
    var wsum = 1.0;
    for (var side = 0; side < 2; side++) {
        var q = start;
        var d = flow(q);
        if (side == 1) { d = -d; }
        for (var i = 1; i <= steps; i++) {
            var nd = flow(q);
            if (dot(nd, d) < 0.0) { nd = -nd; }
            d = nd;
            q += d * P.stroke_step;
            // Busy textures: strokes describe the abstracted wash (texA), not the photo (texB).
            let qi = vec2<i32>(floor(q));
            let s = mix(loadB(qi), loadA(qi), P.busy);
            let w = (1.0 - f32(i) / f32(steps + 1)) * s.a;
            sum += w * s.rgb;
            wsum += w;
        }
    }
    return sum / wsum;
}

fn soft_min(x: f32, cap: f32, knee: f32) -> f32 {
    if (x <= cap - knee) { return x; }
    return cap - knee + knee * tanh((x - (cap - knee)) / knee);
}

@compute @workgroup_size(8, 8)
fn finish(@builtin(global_invocation_id) gid: vec3<u32>) {
    let p = pixel(gid);
    if (outside(p)) { return; }
    var c = textureLoad(texA, p, 0);
    let gp = gpos(p);
    let tint_safe = P.tint_safe != 0;

    // Brushstrokes smeared from the texture's own color along the form.
    if (P.smear > 0.0) {
        let flow_weight = smoothstep(0.1, 0.5, orientation(loadC(p).xyz).z);
        c = vec4<f32>(mix(c.rgb, smear_color(p), P.smear * flow_weight), c.a);
    }

    // Local value-contrast compression: detail moves from light/dark into color.
    if (P.vc_fine > 0.0 || P.vc_mid > 0.0 || P.vc_coarse > 0.0) {
        var v = srgb_to_oklab(c.rgb);
        let m_f = local_mean_a(p, P.vc_r_fine, v.x);
        let m_m = local_mean_a(p, P.vc_r_mid, v.x);
        let m_c = local_mean_a(p, P.vc_r_coarse, v.x);
        let l_new = v.x - P.vc_fine * (v.x - m_f) - P.vc_mid * (m_f - m_m)
            - P.vc_coarse * (m_m - m_c);
        let removed = abs(l_new - v.x);
        v = vec3<f32>(l_new, v.yz * (1.0 + P.vc_chroma * removed));
        c = vec4<f32>(linear_to_srgb(clamp(oklab_to_linear(v), vec3<f32>(0.0), vec3<f32>(1.0))), c.a);
    }

    // Busy textures: bright grayish highlight patches (photographic glare) calm into the local
    // surface color.
    if (P.busy > 0.0 && P.highlight_calm > 0.0) {
        let v = srgb_to_oklab(c.rgb);
        // Only SMALL glints: the texel must stand out against both a ring at the glint radius and
        // one at half of it. A large coherent light region (pale lichen, sunlit patches) has at
        // least one ring inside itself and is left alone.
        var m = vec3<f32>(0.0);
        var mh = vec3<f32>(0.0);
        var n = 0.0;
        for (var k = 0; k < 8; k++) {
            let dir = vec2<f32>(cos(f32(k) * 0.78539816), sin(f32(k) * 0.78539816));
            let q = loadA(p + vec2<i32>(round(dir * P.highlight_radius)));
            let qh = loadA(p + vec2<i32>(round(dir * P.highlight_radius * 0.5)));
            if (q.a > 0.0 && qh.a > 0.0) {
                m += srgb_to_oklab(q.rgb);
                mh += srgb_to_oklab(qh.rgb);
                n += 1.0;
            }
        }
        if (n > 0.0) {
            m /= n;
            mh /= n;
            let t_far = smoothstep(0.03, 0.10, v.x - m.x)
                * smoothstep(0.0, 0.03, length(m.yz) - length(v.yz));
            let t_near = smoothstep(0.03, 0.10, v.x - mh.x);
            // Salient objects (far outside the texture's value distribution) are not glare.
            let salient = smoothstep(3.0, 4.0, abs(v.x - P.mean_l) / max(P.spread, 1e-3));
            let t = P.busy * P.highlight_calm * min(t_far, t_near) * (1.0 - salient);
            let w = mix(v, m, t);
            c = vec4<f32>(linear_to_srgb(clamp(oklab_to_linear(w), vec3<f32>(0.0), vec3<f32>(1.0))), c.a);
        }
    }

    let src = srgb_to_oklab(c.rgb);
    var lab = src;
    var floor_l = 0.0;

    // Palette.
    if (P.lut_size > 1) {
        let e = lut_sample(c.rgb);
        let mapped = srgb_to_oklab(e.rgb);
        floor_l = e.a;
        if (tint_safe) { lab = vec3<f32>(mapped.x, src.yz); } else { lab = mapped; }
    }

    // Warm/cool temperature from the residual low-frequency shading.
    if (P.temp_strength > 0.0 && P.low_w > 0 && !tint_safe) {
        let yb = lowres_sample(gp);
        let t = clamp(log2(max(yb, 1e-4) / max(P.delight_mean, 1e-4)) * P.temp_sens, -1.0, 1.0);
        let h = select(P.temp_cool_hue, P.temp_warm_hue, t > 0.0);
        lab = vec3<f32>(lab.x, lab.yz + hue_dir(h) * (P.temp_strength * abs(t)));
    }

    // Accent darks from high-frequency detail (crevices, gaps between painted shapes), in cool
    // colored shadow. Thresholds come from the image's own histogram (accent_hist/threshold).
    if (P.accent_fraction > 0.0 && P.accent_depth > 0.0) {
        let lo = bitcast<f32>(atomicLoad(&hist[256]));
        let hi = bitcast<f32>(atomicLoad(&hist[257]));
        let t = smoothstep(lo, hi, accent_measure(p)) * P.accent_depth;
        if (t > 0.0) {
            let l_acc = min(lab.x, P.accent_min_l);
            let l_new = mix(lab.x, l_acc, t);
            if (tint_safe) {
                lab = vec3<f32>(l_new, lab.yz);
            } else {
                // Interpolate chroma and hue (shortest arc) so mixes never pass through gray.
                // Hue leads lightness, so a darkening texel has already left its own hue
                // family (no dark greens, even half-way into an accent).
                let th = min(1.0, 3.0 * t);
                let ch = length(lab.yz);
                let h = atan2(lab.z, lab.y);
                var dh = P.accent_hue - h;
                dh = dh - 6.2831853 * round(dh / 6.2831853);
                let h2 = select(P.accent_hue, h + dh * th, ch > 1e-4);
                let c2 = mix(ch, P.accent_chroma, th);
                lab = vec3<f32>(l_new, hue_dir(h2) * c2);
            }
            floor_l = min(floor_l, l_new);
        }
    }

    // Brushstrokes along the flow field.
    if (P.stroke_strength > 0.0) {
        let v = strokes(p);
        lab = vec3<f32>(lab.x + P.stroke_strength * v, lab.yz * (1.0 + P.stroke_chroma * v));
    }

    // Wet edges: pigment pools on the darker side of painted boundaries (dL proportional to the
    // wash depth, capped); the lighter side feathers slightly lighter.
    if (P.edge_dark > 0.0) {
        let s = max(1, i32(round(P.edge_step)));
        let l00 = lightness(loadA(p + vec2<i32>(-s, -s)));
        let l10 = lightness(loadA(p + vec2<i32>(0, -s)));
        let l20 = lightness(loadA(p + vec2<i32>(s, -s)));
        let l01 = lightness(loadA(p + vec2<i32>(-s, 0)));
        let l21 = lightness(loadA(p + vec2<i32>(s, 0)));
        let l02 = lightness(loadA(p + vec2<i32>(-s, s)));
        let l12 = lightness(loadA(p + vec2<i32>(0, s)));
        let l22 = lightness(loadA(p + vec2<i32>(s, s)));
        let gx = (l20 + 2.0 * l21 + l22 - l00 - 2.0 * l01 - l02) * 0.25;
        let gy = (l02 + 2.0 * l12 + l22 - l00 - 2.0 * l10 - l20) * 0.25;
        let ring = (l00 + l10 + l20 + l01 + l21 + l02 + l12 + l22) / 8.0;
        let side = smoothstep(-0.005, 0.02, ring - src.x);
        let th = max(P.edge_threshold, 1e-3);
        var e = smoothstep(th, 2.0 * th, length(vec2<f32>(gx, gy)));
        // Busy textures: only boundaries of large regions (a step that also shows at a much
        // coarser scale) get a wet edge, and a subtler one. Shape-based textures keep theirs.
        if (P.busy > 0.0) {
            let t = max(1, i32(round(P.edge_coarse_step)));
            let cx = lightness(loadA(p + vec2<i32>(t, 0))) - lightness(loadA(p - vec2<i32>(t, 0)));
            let cy = lightness(loadA(p + vec2<i32>(0, t))) - lightness(loadA(p - vec2<i32>(0, t)));
            let ec = smoothstep(th, 2.0 * th, 0.5 * length(vec2<f32>(cx, cy)));
            e = mix(e, min(e, ec) * (1.0 - P.edge_soften), P.busy);
        }
        let depth = max(1.0 - lab.x, 0.0);
        let dl = -min(P.edge_dark, P.edge_rel * depth) * e * side + P.edge_feather * e * (1.0 - side);
        lab = vec3<f32>(lab.x + dl, lab.yz * (1.0 + 0.3 * e * side));
    }

    // Granulation: pigment settles in the source texture's valleys and in paper tooth, in
    // proportion to how much pigment the wash holds.
    if (P.gran > 0.0) {
        let lb = lightness(textureLoad(texB, p, 0));
        let valley = clamp((ring_mean_b(p, P.gran_radius) - lb) * 8.0, -1.0, 1.0);
        let n = fbm(gp, vec2<f32>(P.gran_cells_x, P.gran_cells_y), 5u) * 2.0 - 1.0;
        let depth = clamp((1.0 - lab.x) / 0.25, 0.0, 1.5);
        let g = mix(n, valley, P.gran_valley);
        lab = vec3<f32>(lab.x - P.gran * g * depth, lab.yz * (1.0 + 2.0 * P.gran * g));
    }

    // Paper tooth everywhere, and (bounded by paper_tint) the paper's own color showing through
    // in highlights.
    if (P.paper > 0.0 || P.paper_tint > 0.0) {
        let hl = smoothstep(P.paper_hl, 1.0, lab.x);
        let n = fbm(gp, vec2<f32>(P.paper_cells_x, P.paper_cells_y), 11u) * 2.0 - 1.0;
        let paper = srgb_to_oklab(vec3<f32>(P.paper_r, P.paper_g, P.paper_b));
        var ab = lab.yz;
        if (!tint_safe) {
            ab = mix(ab, paper.yz, clamp(P.paper_tint * hl * (0.3 + 0.7 * max(n, 0.0)), 0.0, 1.0));
        }
        lab = vec3<f32>(lab.x + P.paper * n * (0.4 + 0.6 * hl), ab);
    }

    // Adaptive contrast: remove (1 − amp) of the source's deviation from its local lightness
    // (a ring mean at `pivot_r`, a bit larger than a groove, smaller than a lichen patch).
    // Groove and grit amplitude shrinks; the coarse light/dark pattern (big patches, lichen,
    // large lighting shapes), every edge position and the palette's per-texel changes stay.
    if (P.amp < 1.0) {
        var pl = 0.0;
        var pn = 0.0;
        for (var k = 0; k < 16; k++) {
            let ang = f32(k) * 0.39269908;
            let q = loadA(p + vec2<i32>(round(vec2<f32>(cos(ang), sin(ang)) * P.pivot_r)));
            if (q.a > 0.0) { pl += lightness(q); pn += 1.0; }
        }
        let pivot = select(src.x, pl / max(pn, 1.0), pn > 0.0);
        // Salient objects (value outliers beyond ~3 sigma of the texture's spread) keep their
        // contrast; grooves and grain within the distribution are compressed.
        let keep = smoothstep(2.0, 3.0, abs(src.x - pivot) / max(P.spread, 1e-3));
        lab.x = lab.x - (1.0 - P.amp) * (1.0 - keep) * (src.x - pivot);
    }

    // Busy textures: keep each texel's own source chroma (no gray-and-warm averaging into mud).
    if (P.busy > 0.0 && P.chroma_retain > 0.0 && !tint_safe) {
        let sc = length(srgb_to_oklab(textureLoad(texB, p, 0).rgb).yz);
        let lc = length(lab.yz);
        let want = P.chroma_retain * P.busy * sc;
        if (lc > 1e-4 && lc < want) { lab = vec3<f32>(lab.x, lab.yz * (want / lc)); }
    }

    // Keep watercolor darkening from undercutting the palette floor.
    lab.x = max(lab.x, floor_l - P.floor_margin);
    // Relit categories: compress only the top `ceiling_knee` below the ceiling (mid and light
    // values keep their lightness; the renderer's lit multiplier gets headroom).
    if (P.ceiling < 1.0) {
        lab.x = soft_min(lab.x, P.ceiling, max(P.ceiling_knee, 1e-3));
    }
    lab.x = clamp(lab.x, 0.0, 1.0);

    let rgb = linear_to_srgb(clamp(oklab_to_linear(lab), vec3<f32>(0.0), vec3<f32>(1.0)));
    textureStore(outTex, p, vec4<f32>(rgb, c.a));
}
