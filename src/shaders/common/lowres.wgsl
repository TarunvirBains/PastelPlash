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
