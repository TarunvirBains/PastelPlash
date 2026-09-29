// ---------------------------------------------------------------- brushstrokes and granulation helpers (finish)

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
    return strokes_at(p, vec2<f32>(P.stroke_cells_x, P.stroke_cells_y), P.stroke_len, 1.6);
}

// Strokes with `cells` noise cells across the image (fewer = broader strokes), `len` texels
// long; `gain` sets how quickly they saturate at ±1.
fn strokes_at(p: vec2<i32>, cells_in: vec2<f32>, len: f32, gain: f32) -> f32 {
    let cells = max(round(cells_in), vec2<f32>(1.0));
    let origin = vec2<f32>(f32(P.origin_x), f32(P.origin_y));
    let start = vec2<f32>(p) + 0.5;
    let step = max(len / 16.0, 1.0);
    let steps = i32(clamp(ceil(len / step), 1.0, 32.0));
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
            q += d * step;
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
    return clamp((sum / wsum - 0.5) * sqrt(n_eff) * gain, -1.0, 1.0) * flow_weight;
}
