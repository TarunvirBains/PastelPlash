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
