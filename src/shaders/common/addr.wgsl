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
