// ---------------------------------------------------------------- 1b. soft value grouping

// Soft-assigned value-mass lightness for lightness `l` (the texel itself when there are no
// masses).
fn mass_value(l: f32) -> f32 {
    var wl = 0.0;
    var wt = 0.0;
    let n = i32(P.grp_count);
    for (var k = 0; k < 4; k++) {
        if (k >= n) { break; }
        let d = (l - P.grp_l[k]) / max(P.grp_sigma, 1e-3);
        let w = exp(-d * d);
        wl += w * P.grp_l[k];
        wt += w;
    }
    return select(l, wl / wt, wt > 1e-12);
}

// Pulls each texel's lightness toward its soft-assigned value mass (masses found on the CPU,
// src/grouping.rs) and its color modestly toward the mass color. The assignment uses an
// edge-aware (bilateral) smoothed lightness, so mass boundaries follow the texture's own shapes;
// the soft weights blend masses at their boundaries (no posterization). Texels far from every
// mass (small salient objects) and differently colored texels keep their value / color.
@compute @workgroup_size(8, 8)
fn group(@builtin(global_invocation_id) gid: vec3<u32>) {
    let p = pixel(gid);
    if (outside(p)) { return; }
    let c = textureLoad(texA, p, 0);
    if (P.grp <= 0.0 || c.a <= 0.0 || P.grp_count < 2.0) {
        textureStore(outTex, p, c);
        return;
    }
    let v = srgb_to_oklab(c.rgb);
    // Edge-aware smoothed lightness.
    let r = max(P.grp_radius, 1.0);
    let ri = i32(ceil(r));
    let st = max(1, i32(round(r / 3.0)));
    let sig_s = max(r * 0.5, 0.5);
    let range = max(P.grp_range, 1e-3);
    var sum = 0.0;
    var wsum = 0.0;
    for (var y = -ri; y <= ri; y += st) {
        for (var x = -ri; x <= ri; x += st) {
            let d2 = f32(x * x + y * y);
            if (d2 > r * r) { continue; }
            let q = loadA(p + vec2<i32>(x, y));
            if (q.a <= 0.0) { continue; }
            let l = lightness(q);
            let dl = (l - v.x) / range;
            let w = exp(-d2 / (2.0 * sig_s * sig_s) - dl * dl);
            sum += w * l;
            wsum += w;
        }
    }
    let ls = sum / max(wsum, 1e-8);
    // Soft assignment to the masses.
    var wl = 0.0;
    var wa = vec2<f32>(0.0);
    var wt = 0.0;
    var dmin = 1.0;
    let n = i32(P.grp_count);
    for (var k = 0; k < 4; k++) {
        if (k >= n) { break; }
        let d = (ls - P.grp_l[k]) / max(P.grp_sigma, 1e-3);
        let w = exp(-d * d);
        wl += w * P.grp_l[k];
        wa += w * vec2<f32>(P.grp_a[k], P.grp_b[k]);
        wt += w;
        dmin = min(dmin, abs(v.x - P.grp_l[k]));
    }
    if (wt < 1e-12) {
        // Far outside every mass: nearest mass by lightness decides (no blend needed; the texel
        // is salient and keeps its value below anyway).
        textureStore(outTex, p, c);
        return;
    }
    let target_l = wl / wt;
    let target_ab = wa / wt;
    // Salient objects (far from every mass) keep their value and color.
    let keep = smoothstep(P.grp_sal0, P.grp_sal1, dmin);
    let s = P.grp * (1.0 - keep);
    let l_new = mix(v.x, target_l, s);
    // Color: only within the mass's own color family (a red berry in a green mass stays red).
    let family = 1.0 - smoothstep(P.grp_family, 2.0 * P.grp_family, length(v.yz - target_ab));
    var ab = v.yz;
    if (P.tint_safe == 0) {
        ab = mix(v.yz, target_ab, P.grp_color * s * family);
    }
    let rgb = linear_to_srgb(clamp(oklab_to_linear(vec3<f32>(l_new, ab)), vec3<f32>(0.0), vec3<f32>(1.0)));
    textureStore(outTex, p, vec4<f32>(rgb, c.a));
}
