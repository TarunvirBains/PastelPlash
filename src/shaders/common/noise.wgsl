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
