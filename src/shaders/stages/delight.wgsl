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
