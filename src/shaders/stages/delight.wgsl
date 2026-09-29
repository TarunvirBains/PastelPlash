// ---------------------------------------------------------------- 1. despeckle and de-light

// Mean color of a ring of 8 source texels at radius r, and how many of them are
// within `tol` of lightness `l0`.
fn speck_ring(p: vec2<i32>, r: f32, l0: f32, tol: f32) -> vec4<f32> {
    var sum = vec3<f32>(0.0);
    var similar = 0.0;
    for (var k = 0; k < 8; k++) {
        let ang = f32(k) * 0.78539816;
        let q = loadA(p + vec2<i32>(round(vec2<f32>(cos(ang), sin(ang)) * r)));
        let l = lightness(q);
        sum += q.rgb;
        if (abs(l - l0) < tol) { similar += 1.0; }
    }
    return vec4<f32>(sum / 8.0, similar);
}

// Dark specks of a few texels (photographic dirt, dust in a cobweb) are noise: a texel darker
// than the rings around it at the speck radius and at 1.5× it, where at most one ring sample
// shares its value (so it is no line, crack, handle or rim), takes the ring's color. Real small
// objects are larger than the speck radius and keep their value.
fn despeckle(p: vec2<i32>, c: vec4<f32>) -> vec4<f32> {
    if (!(P.speck_r > 0.0) || c.a < 0.5) { return c; }
    let l0 = lightness(c);
    let tol = 0.5 * P.speck_thr;
    let a = speck_ring(p, P.speck_r, l0, tol);
    let b = speck_ring(p, P.speck_r * 1.5, l0, tol);
    if (a.w > 1.0 || b.w > 1.0) { return c; }
    let ring = srgb_to_oklab(a.rgb).x;
    let t = smoothstep(P.speck_thr, 2.0 * P.speck_thr, ring - l0)
        * smoothstep(P.speck_thr, 2.0 * P.speck_thr, srgb_to_oklab(b.rgb).x - l0);
    return vec4<f32>(mix(c.rgb, a.rgb, t), c.a);
}

@compute @workgroup_size(8, 8)
fn delight(@builtin(global_invocation_id) gid: vec3<u32>) {
    let p = pixel(gid);
    if (outside(p)) { return; }
    var c = despeckle(p, textureLoad(texA, p, 0));
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
