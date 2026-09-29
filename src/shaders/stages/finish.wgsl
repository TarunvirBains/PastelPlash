// ---------------------------------------------------------------- watercolor finish

// Edge-aware local mean of the painted lightness around `p` (center plus two rings at r and
// r/2): samples across a strong lightness step get little weight, so structural edges survive
// value compression.
fn local_mean_a(p: vec2<i32>, r: f32, l0: f32) -> f32 {
    var sum = l0;
    var wsum = 1.0;
    let sigma = max(P.vc_range, 1e-3);
    for (var ring = 0; ring < 2; ring++) {
        let rr = select(r, r * 0.5, ring == 1);
        for (var k = 0; k < 8; k++) {
            let ang = (f32(k) + 0.5 * f32(ring)) * 0.78539816;
            let q = loadA(p + vec2<i32>(round(vec2<f32>(cos(ang), sin(ang)) * rr)));
            if (q.a <= 0.0) { continue; }
            let l = lightness(q);
            let d = (l - l0) / sigma;
            let w = exp(-d * d);
            sum += w * l;
            wsum += w;
        }
    }
    return sum / wsum;
}

// Brushstrokes from the texture itself: the de-lit source color (texB) averaged along the flow
// (line-integral convolution), so detail comes back as streaks that follow form. Never across
// an edge: the path follows the edge tangent.
fn smear_color(p: vec2<i32>) -> vec3<f32> {
    let start = vec2<f32>(p) + 0.5;
    let steps = i32(clamp(ceil(P.stroke_len / P.stroke_step), 1.0, 32.0));
    var sum = mix(textureLoad(texB, p, 0).rgb, textureLoad(texA, p, 0).rgb, P.busy);
    var wsum = 1.0;
    for (var side = 0; side < 2; side++) {
        var q = start;
        var d = flow(q);
        if (side == 1) { d = -d; }
        for (var i = 1; i <= steps; i++) {
            var nd = flow(q);
            if (dot(nd, d) < 0.0) { nd = -nd; }
            d = nd;
            q += d * P.stroke_step;
            // Busy textures: strokes describe the abstracted wash (texA), not the photo (texB).
            let qi = vec2<i32>(floor(q));
            let s = mix(loadB(qi), loadA(qi), P.busy);
            let w = (1.0 - f32(i) / f32(steps + 1)) * s.a;
            sum += w * s.rgb;
            wsum += w;
        }
    }
    return sum / wsum;
}

fn soft_min(x: f32, cap: f32, knee: f32) -> f32 {
    if (x <= cap - knee) { return x; }
    return cap - knee + knee * tanh((x - (cap - knee)) / knee);
}

// Brushstrokes smeared from the texture's own color along the form.
fn finish_smear(p: vec2<i32>, c: vec4<f32>) -> vec4<f32> {
    if (!(P.smear > 0.0)) { return c; }
    let flow_weight = smoothstep(0.1, 0.5, orientation(loadC(p).xyz).z);
    return vec4<f32>(mix(c.rgb, smear_color(p), P.smear * flow_weight), c.a);
}

// Local value-contrast compression: detail moves from light/dark into color.
fn finish_value(p: vec2<i32>, c: vec4<f32>) -> vec4<f32> {
    if (!(P.vc_fine > 0.0 || P.vc_mid > 0.0 || P.vc_coarse > 0.0)) { return c; }
    var v = srgb_to_oklab(c.rgb);
    let m_f = local_mean_a(p, P.vc_r_fine, v.x);
    let m_m = local_mean_a(p, P.vc_r_mid, v.x);
    let m_c = local_mean_a(p, P.vc_r_coarse, v.x);
    let l_new = v.x - P.vc_fine * (v.x - m_f) - P.vc_mid * (m_f - m_m)
        - P.vc_coarse * (m_m - m_c);
    let removed = abs(l_new - v.x);
    v = vec3<f32>(l_new, v.yz * (1.0 + P.vc_chroma * removed));
    return vec4<f32>(linear_to_srgb(clamp(oklab_to_linear(v), vec3<f32>(0.0), vec3<f32>(1.0))), c.a);
}

// Busy textures: bright grayish highlight patches (photographic glare) calm into the local
// surface color.
fn finish_glare(p: vec2<i32>, c: vec4<f32>) -> vec4<f32> {
    if (!(P.busy > 0.0 && P.highlight_calm > 0.0)) { return c; }
    let v = srgb_to_oklab(c.rgb);
    // Only SMALL glints: the texel must stand out against both a ring at the glint radius and
    // one at half of it. A large coherent light region (pale lichen, sunlit patches) has at
    // least one ring inside itself and is left alone.
    var m = vec3<f32>(0.0);
    var mh = vec3<f32>(0.0);
    var n = 0.0;
    for (var k = 0; k < 8; k++) {
        let dir = vec2<f32>(cos(f32(k) * 0.78539816), sin(f32(k) * 0.78539816));
        let q = loadA(p + vec2<i32>(round(dir * P.highlight_radius)));
        let qh = loadA(p + vec2<i32>(round(dir * P.highlight_radius * 0.5)));
        if (q.a > 0.0 && qh.a > 0.0) {
            m += srgb_to_oklab(q.rgb);
            mh += srgb_to_oklab(qh.rgb);
            n += 1.0;
        }
    }
    if (!(n > 0.0)) { return c; }
    m /= n;
    mh /= n;
    let t_far = smoothstep(0.03, 0.10, v.x - m.x)
        * smoothstep(0.0, 0.03, length(m.yz) - length(v.yz));
    let t_near = smoothstep(0.03, 0.10, v.x - mh.x);
    // Salient objects (far outside the texture's value distribution) are not glare.
    let salient = smoothstep(3.0, 4.0, abs(v.x - P.mean_l) / max(P.spread, 1e-3));
    let t = P.busy * P.highlight_calm * min(t_far, t_near) * (1.0 - salient);
    let w = mix(v, m, t);
    return vec4<f32>(linear_to_srgb(clamp(oklab_to_linear(w), vec3<f32>(0.0), vec3<f32>(1.0))), c.a);
}

// Palette: the mapped color (lightness only for tint-safe textures) and, in w, the lightness
// floor the palette promised.
fn finish_palette(c: vec4<f32>, src: vec3<f32>, tint_safe: bool) -> vec4<f32> {
    if (!(P.lut_size > 1)) { return vec4<f32>(src, 0.0); }
    let e = lut_sample(c.rgb);
    let mapped = srgb_to_oklab(e.rgb);
    if (tint_safe) { return vec4<f32>(mapped.x, src.yz, e.a); }
    return vec4<f32>(mapped, e.a);
}

fn cast_exposure(l: f32) -> f32 {
    if (l <= P.cast_pivot) { return l; }
    return P.cast_pivot + (l - P.cast_pivot) * (1.0 - P.cast_s * (1.0 - P.cast_exposure));
}

// Weight of hue `h` (degrees) inside [lo, hi], feathered by `f` degrees outside.
fn band_weight(lo: f32, hi: f32, f: f32, h: f32) -> f32 {
    let span = (hi - lo) - 360.0 * floor((hi - lo) / 360.0);
    let d = h - lo + 180.0;
    let x = d - 360.0 * floor(d / 360.0) - 180.0;
    return smoothstep(-f, 0.0, x) * (1.0 - smoothstep(span, span + f, x));
}

// The shared moonlight cast of a mood (same math as `palette::apply_cast`): a colored light
// (linear RGB times the cast's filter), exposure down above the palette floor, chroma scaled;
// colored sources keep most of their color; near-neutral darks blend (continuously) into a muted
// midnight capped by lightness; warm darks are never dull (no mud). `lf` is the mapped lab color
// and, in w, the lightness floor (dimmed the same way).
fn finish_cast(lf: vec4<f32>, src: vec3<f32>, tint_safe: bool) -> vec4<f32> {
    if (!(P.cast_on > 0.0)) { return lf; }
    let floor_l = cast_exposure(lf.w);
    if (tint_safe) { return vec4<f32>(cast_exposure(lf.x), lf.yz, floor_l); }
    let filt = vec3<f32>(P.cast_fr, P.cast_fg, P.cast_fb);
    let lit = linear_to_oklab(max(oklab_to_linear(lf.xyz) * filt, vec3<f32>(0.0)));
    let l2 = cast_exposure(lit.x);
    let c_src = length(src.yz);
    let dark = 1.0 - smoothstep(P.cast_dark_below - 0.03, P.cast_dark_below + 0.05, l2);
    let mud_dark = 1.0 - smoothstep(P.cast_dark_below + 0.02, P.cast_dark_below + 0.1, l2);
    let c_rel = c_src * max(1.0, 0.55 / (max(src.x, 0.0) + 0.05));
    let neutral = 1.0 - smoothstep(0.012, 0.03, c_rel);
    let kd = hue_dir(P.cast_hue);
    var abn = lit.yz * (1.0 - P.cast_s * (1.0 - P.cast_chroma));
    let keep = smoothstep(0.03, 0.05, c_src) * min(0.65 * c_src, 0.052);
    let len = max(length(abn), 1e-9);
    let cn = max(len, keep);
    abn = abn / len * cn;
    let cm = max(P.cast_dark_cap * l2, P.cast_dark_min);
    let w = neutral * dark;
    let ab = mix(abn, kd * cm, w);
    var c3 = mix(cn, cm, w);
    var dir = kd;
    if (length(ab) > 1e-9) { dir = normalize(ab); }
    let h3 = atan2(dir.y, dir.x) * 57.29577951;
    let warm = band_weight(P.cast_warm0, P.cast_warm1, 5.0, h3);
    c3 = max(max(c3, P.cast_dark_chroma * mud_dark * warm), P.cast_dark_min * dark);
    return vec4<f32>(l2, dir * c3, floor_l);
}

// Warm/cool temperature from the residual low-frequency shading.
fn finish_temperature(gp: vec2<f32>, lab: vec3<f32>, tint_safe: bool) -> vec3<f32> {
    if (!(P.temp_strength > 0.0 && P.low_w > 0 && !tint_safe)) { return lab; }
    let yb = lowres_sample(gp);
    let t = clamp(log2(max(yb, 1e-4) / max(P.delight_mean, 1e-4)) * P.temp_sens, -1.0, 1.0);
    let h = select(P.temp_cool_hue, P.temp_warm_hue, t > 0.0);
    return vec3<f32>(lab.x, lab.yz + hue_dir(h) * (P.temp_strength * abs(t)));
}

// Accent darks from high-frequency detail (crevices, gaps between painted shapes), in cool
// colored shadow. Thresholds come from the image's own histogram (accent_hist/threshold).
// `lf` is the lab color and, in w, the lightness floor (lowered under an accent).
fn finish_accent(p: vec2<i32>, lf: vec4<f32>, tint_safe: bool) -> vec4<f32> {
    if (!(P.accent_fraction > 0.0 && P.accent_depth > 0.0)) { return lf; }
    var lab = lf.xyz;
    var floor_l = lf.w;
    let lo = bitcast<f32>(atomicLoad(&hist[256]));
    let hi = bitcast<f32>(atomicLoad(&hist[257]));
    let t = smoothstep(lo, hi, accent_measure(p)) * P.accent_depth;
    if (t > 0.0) {
        let l_acc = min(lab.x, P.accent_min_l);
        let l_new = mix(lab.x, l_acc, t);
        if (tint_safe) {
            lab = vec3<f32>(l_new, lab.yz);
        } else {
            // Interpolate chroma and hue (shortest arc) so mixes never pass through gray.
            // Hue leads lightness, so a darkening texel has already left its own hue
            // family (no dark greens, even half-way into an accent).
            let th = min(1.0, 3.0 * t);
            let ch = length(lab.yz);
            let h = atan2(lab.z, lab.y);
            var dh = P.accent_hue - h;
            dh = dh - 6.2831853 * round(dh / 6.2831853);
            let h2 = select(P.accent_hue, h + dh * th, ch > 1e-4);
            let c2 = mix(ch, P.accent_chroma, th);
            lab = vec3<f32>(l_new, hue_dir(h2) * c2);
        }
        floor_l = min(floor_l, l_new);
    }
    return vec4<f32>(lab, floor_l);
}

// Brushstrokes along the flow field.
fn finish_strokes(p: vec2<i32>, lab: vec3<f32>) -> vec3<f32> {
    if (!(P.stroke_strength > 0.0)) { return lab; }
    let v = strokes(p);
    // Grouped textures: variation within a mass is mostly hue and chroma, less value.
    let lv = mix(1.0, P.grp_stroke, P.grp);
    let cv = 1.0 + P.grp * (1.0 - P.grp_stroke);
    // Amplitude scales with the headroom toward black or white (strokes never clip).
    let dl = P.stroke_strength * lv * v;
    let room = select(smoothstep(0.0, 0.12, lab.x), smoothstep(0.0, 0.12, 1.0 - lab.x), dl > 0.0);
    return vec3<f32>(lab.x + dl * room, lab.yz * (1.0 + P.stroke_chroma * cv * v));
}

// Wet edges: pigment pools on the darker side of painted boundaries (dL proportional to the
// wash depth, capped); the lighter side feathers slightly lighter.
fn finish_wet_edges(p: vec2<i32>, lab: vec3<f32>, src: vec3<f32>) -> vec3<f32> {
    if (!(P.edge_dark > 0.0)) { return lab; }
    let s = max(1, i32(round(P.edge_step)));
    let l00 = lightness(loadA(p + vec2<i32>(-s, -s)));
    let l10 = lightness(loadA(p + vec2<i32>(0, -s)));
    let l20 = lightness(loadA(p + vec2<i32>(s, -s)));
    let l01 = lightness(loadA(p + vec2<i32>(-s, 0)));
    let l21 = lightness(loadA(p + vec2<i32>(s, 0)));
    let l02 = lightness(loadA(p + vec2<i32>(-s, s)));
    let l12 = lightness(loadA(p + vec2<i32>(0, s)));
    let l22 = lightness(loadA(p + vec2<i32>(s, s)));
    let gx = (l20 + 2.0 * l21 + l22 - l00 - 2.0 * l01 - l02) * 0.25;
    let gy = (l02 + 2.0 * l12 + l22 - l00 - 2.0 * l10 - l20) * 0.25;
    let ring = (l00 + l10 + l20 + l01 + l21 + l02 + l12 + l22) / 8.0;
    let side = smoothstep(-0.005, 0.02, ring - src.x);
    let th = max(P.edge_threshold, 1e-3);
    var e = smoothstep(th, 2.0 * th, length(vec2<f32>(gx, gy)));
    // Busy textures: only boundaries of large regions (a step that also shows at a much
    // coarser scale) get a wet edge, and a subtler one. Shape-based textures keep theirs.
    if (P.busy > 0.0) {
        let t = max(1, i32(round(P.edge_coarse_step)));
        let cx = lightness(loadA(p + vec2<i32>(t, 0))) - lightness(loadA(p - vec2<i32>(t, 0)));
        let cy = lightness(loadA(p + vec2<i32>(0, t))) - lightness(loadA(p - vec2<i32>(0, t)));
        let ec = smoothstep(th, 2.0 * th, 0.5 * length(vec2<f32>(cx, cy)));
        e = mix(e, min(e, ec) * (1.0 - P.edge_soften), P.busy);
    }
    let depth = max(1.0 - lab.x, 0.0);
    let dl = -min(P.edge_dark, P.edge_rel * depth) * e * side + P.edge_feather * e * (1.0 - side);
    return vec3<f32>(lab.x + dl, lab.yz * (1.0 + 0.3 * e * side));
}

// Granulation: pigment settles in the source texture's valleys and in paper tooth, in
// proportion to how much pigment the wash holds.
fn finish_granulation(p: vec2<i32>, gp: vec2<f32>, lab: vec3<f32>) -> vec3<f32> {
    if (!(P.gran > 0.0)) { return lab; }
    let lb = lightness(textureLoad(texB, p, 0));
    let valley = clamp((ring_mean_b(p, P.gran_radius) - lb) * 8.0, -1.0, 1.0);
    let n = fbm(gp, vec2<f32>(P.gran_cells_x, P.gran_cells_y), 5u) * 2.0 - 1.0;
    let depth = clamp((1.0 - lab.x) / 0.25, 0.0, 1.5);
    let g = mix(n, valley, P.gran_valley);
    return vec3<f32>(lab.x - P.gran * g * depth, lab.yz * (1.0 + 2.0 * P.gran * g));
}

// Paper tooth everywhere, and (bounded by paper_tint) the paper's own color showing through
// in highlights.
fn finish_paper(gp: vec2<f32>, lab: vec3<f32>, tint_safe: bool) -> vec3<f32> {
    if (!(P.paper > 0.0 || P.paper_tint > 0.0)) { return lab; }
    let hl = smoothstep(P.paper_hl, 1.0, lab.x);
    let n = fbm(gp, vec2<f32>(P.paper_cells_x, P.paper_cells_y), 11u) * 2.0 - 1.0;
    let paper = srgb_to_oklab(vec3<f32>(P.paper_r, P.paper_g, P.paper_b));
    var ab = lab.yz;
    if (!tint_safe) {
        ab = mix(ab, paper.yz, clamp(P.paper_tint * hl * (0.3 + 0.7 * max(n, 0.0)), 0.0, 1.0));
    }
    return vec3<f32>(lab.x + P.paper * n * (0.4 + 0.6 * hl), ab);
}

// Adaptive contrast: remove (1 − amp) of the source's deviation from its local lightness
// (a ring mean at `pivot_r`, a bit larger than a groove, smaller than a lichen patch).
// Groove and grit amplitude shrinks; the coarse light/dark pattern (big patches, lichen,
// large lighting shapes), every edge position and the palette's per-texel changes stay.
fn finish_adaptive(p: vec2<i32>, lab: vec3<f32>, src: vec3<f32>) -> vec3<f32> {
    if (!(P.amp < 1.0)) { return lab; }
    var pl = 0.0;
    var pn = 0.0;
    for (var k = 0; k < 16; k++) {
        let ang = f32(k) * 0.39269908;
        let q = loadA(p + vec2<i32>(round(vec2<f32>(cos(ang), sin(ang)) * P.pivot_r)));
        if (q.a > 0.0) { pl += lightness(q); pn += 1.0; }
    }
    var pivot = select(src.x, pl / max(pn, 1.0), pn > 0.0);
    // Grouped textures: compress toward the texel's own value mass instead, so variation
    // within a mass shrinks while the masses keep their separation.
    if (P.grp > 0.0 && P.grp_count >= 2.0) {
        pivot = mix(pivot, mass_value(src.x), P.grp);
    }
    // Salient objects (value outliers beyond ~3 sigma of the texture's spread) keep their
    // contrast; grooves and grain within the distribution are compressed.
    let keep = smoothstep(2.0, 3.0, abs(src.x - pivot) / max(P.spread, 1e-3));
    return vec3<f32>(lab.x - (1.0 - P.amp) * (1.0 - keep) * (src.x - pivot), lab.yz);
}

// Busy textures: keep each texel's own source chroma (no gray-and-warm averaging into mud).
fn finish_chroma_retain(p: vec2<i32>, lab: vec3<f32>, tint_safe: bool) -> vec3<f32> {
    if (!(P.busy > 0.0 && P.chroma_retain > 0.0 && !tint_safe)) { return lab; }
    let sc = length(srgb_to_oklab(textureLoad(texB, p, 0).rgb).yz);
    let lc = length(lab.yz);
    let want = P.chroma_retain * P.busy * sc;
    if (lc > 1e-4 && lc < want) { return vec3<f32>(lab.x, lab.yz * (want / lc)); }
    return lab;
}

// Keep watercolor darkening from undercutting the palette floor; relit categories compress
// only the top `ceiling_knee` below the ceiling (mid and light values keep their lightness;
// the renderer's lit multiplier gets headroom).
fn finish_limits(lab: vec3<f32>, floor_l: f32) -> vec3<f32> {
    var l = max(lab.x, floor_l - P.floor_margin);
    if (P.ceiling < 1.0) {
        l = soft_min(l, P.ceiling, max(P.ceiling_knee, 1e-3));
    }
    return vec3<f32>(clamp(l, 0.0, 1.0), lab.yz);
}

// No new clipping: an output channel stays inside 8-bit 1..254 wherever the source channel was;
// where the source was clipped it may stay so.
// Source-blown whites: small compact ones (glints, sparkles, snow, stars: under half of a ring
// of neighbors at `clip_r` is blown too) keep their clean white; large blown regions may only
// darken.
fn finish_clip(p: vec2<i32>, rgb: vec3<f32>) -> vec3<f32> {
    let s = textureLoad(texD, p, 0).rgb;
    let lo = 1.0 / 255.0;
    let hi = 254.0 / 255.0;
    if (all(s >= vec3<f32>(hi))) {
        var blown = 0.0;
        for (var k = 0; k < 8; k++) {
            let ang = f32(k) * 0.78539816;
            let q = textureLoad(texD, addr(p + vec2<i32>(round(vec2<f32>(cos(ang), sin(ang)) * P.clip_r))), 0).rgb;
            if (all(q >= vec3<f32>(hi))) { blown += 1.0; }
        }
        if (blown < 4.0) { return s; }
        return min(rgb, s);
    }
    // (The soft knee is in the strokes' headroom; this guard is exact for unclipped 8-bit values.)
    var out = rgb;
    for (var k = 0; k < 3; k++) {
        if (s[k] >= lo && s[k] <= hi) {
            out[k] = clamp(out[k], lo, hi);
        }
    }
    return out;
}

// Engine-tinted grayscale (tint-safe) textures of categories with a tint-safe gray: the painted
// gray raised (shifted so the median sits at the target, the top soft-capped: the folds keep
// their contrast), brightness-only strokes along the texture's own flow (cloth folds) with
// headroom toward the cap, a few near-white highlight strokes, capped below white. Replaces the
// palette's lightness and the ceiling.
fn finish_tint_safe(p: vec2<i32>, l_src: f32) -> f32 {
    let base = soft_min(clamp(l_src, 0.0, 1.0) + P.ts_shift, P.ts_max, 0.1);
    // Soft, broad strokes: twice the style's width, three times its length, never saturated.
    let v = strokes_at(p, vec2<f32>(P.stroke_cells_x, P.stroke_cells_y) * 0.5, 3.0 * P.stroke_len, 0.8);
    let room = smoothstep(0.0, 0.08, P.ts_max - base);
    var l = base + P.ts_amp * v * select(1.0, room, v > 0.0);
    l = mix(l, P.ts_max, 0.5 * smoothstep(0.75, 1.0, v));
    return clamp(soft_min(l, P.ts_max, 0.04), 0.0, 1.0);
}

@compute @workgroup_size(8, 8)
fn finish(@builtin(global_invocation_id) gid: vec3<u32>) {
    let p = pixel(gid);
    if (outside(p)) { return; }
    var c = textureLoad(texA, p, 0);
    let gp = gpos(p);
    let tint_safe = P.tint_safe != 0;

    c = finish_smear(p, c);
    c = finish_value(p, c);
    c = finish_glare(p, c);
    let src = srgb_to_oklab(c.rgb);
    var lf = finish_palette(c, src, tint_safe);
    lf = finish_cast(lf, src, tint_safe);
    lf = vec4<f32>(finish_temperature(gp, lf.xyz, tint_safe), lf.w);
    lf = finish_accent(p, lf, tint_safe);
    var lab = lf.xyz;
    lab = finish_strokes(p, lab);
    lab = finish_wet_edges(p, lab, src);
    lab = finish_granulation(p, gp, lab);
    lab = finish_paper(gp, lab, tint_safe);
    lab = finish_adaptive(p, lab, src);
    lab = finish_chroma_retain(p, lab, tint_safe);
    lab = finish_limits(lab, lf.w);
    if (tint_safe && P.ts_on > 0.0) {
        lab = vec3<f32>(finish_tint_safe(p, src.x), lab.yz);
    }

    let rgb = finish_clip(p, linear_to_srgb(clamp(oklab_to_linear(lab), vec3<f32>(0.0), vec3<f32>(1.0))));
    textureStore(outTex, p, vec4<f32>(rgb, c.a));
}
