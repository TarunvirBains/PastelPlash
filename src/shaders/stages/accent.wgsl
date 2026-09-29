// ---------------------------------------------------------------- accent darks: histogram and thresholds

// Mean OKLab L of the painted image (texA) on a ring of 8 samples at radius r.
fn ring_mean_a(p: vec2<i32>, r: f32) -> f32 {
    var s = 0.0;
    for (var k = 0; k < 8; k++) {
        let ang = f32(k) * 0.78539816;
        let o = vec2<i32>(round(vec2<f32>(cos(ang), sin(ang)) * r));
        s += lightness(loadA(p + o));
    }
    return s / 8.0;
}

// How much darker than its surroundings a painted texel's neighborhood is (crevices, gaps;
// flat washes ~0). A band-pass (outer ring vs. inner disc) so isolated specks don't count.
fn accent_measure(p: vec2<i32>) -> f32 {
    let inner = 0.5 * lightness(loadA(p)) + 0.5 * ring_mean_a(p, max(P.accent_radius / 3.0, 1.0));
    return ring_mean_a(p, P.accent_radius) - inner;
}

const HIST_MIN: f32 = -0.1;
const HIST_SCALE: f32 = 512.0; // bins per unit L; 256 bins cover -0.1 .. 0.4

@compute @workgroup_size(8, 8)
fn accent_hist(@builtin(global_invocation_id) gid: vec3<u32>) {
    let p = pixel(gid);
    if (outside(p) || (p.x & 1) != 0 || (p.y & 1) != 0) { return; }
    if (textureLoad(texA, p, 0).a < 0.5) { return; }
    let bin = clamp(i32((accent_measure(p) - HIST_MIN) * HIST_SCALE), 0, 255);
    atomicAdd(&hist[bin], 1u);
}

// Turns the histogram into the lo/hi thresholds for the configured top fraction.
@compute @workgroup_size(1)
fn accent_threshold() {
    var total = 0u;
    for (var i = 0; i < 256; i++) { total += atomicLoad(&hist[i]); }
    let want_lo = f32(total) * P.accent_fraction;
    let want_hi = want_lo * (1.0 - clamp(P.accent_softness, 0.0, 0.95));
    var acc = 0.0;
    var lo = 1.0;
    var hi = 1.0;
    var found_hi = false;
    for (var i = 255; i >= 0; i--) {
        acc += f32(atomicLoad(&hist[i]));
        let v = f32(i) / HIST_SCALE + HIST_MIN;
        if (!found_hi && acc >= want_hi) { hi = v; found_hi = true; }
        if (acc >= want_lo) { lo = v; break; }
    }
    lo = max(lo, P.accent_min_depth);
    hi = max(hi, lo + 0.01);
    atomicStore(&hist[256], bitcast<u32>(lo));
    atomicStore(&hist[257], bitcast<u32>(hi));
}
