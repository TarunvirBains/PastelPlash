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
