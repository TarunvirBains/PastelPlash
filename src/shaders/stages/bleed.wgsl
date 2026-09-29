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
