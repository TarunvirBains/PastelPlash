// ---------------------------------------------------------------- 5. palette LUT (finish)

fn lut_at(r: i32, g: i32, b: i32) -> vec4<f32> {
    let n = P.lut_size;
    return lut[r + n * (g + n * b)];
}

// Tetrahedral interpolation (keeps the gray axis exact; same math as `Lut3d::sample`).
fn lut_sample(rgb: vec3<f32>) -> vec4<f32> {
    let n = P.lut_size - 1;
    let pos = clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)) * f32(n);
    let i = min(vec3<i32>(floor(pos)), vec3<i32>(n - 1));
    let f = pos - vec3<f32>(i);
    let c000 = lut_at(i.x, i.y, i.z);
    let c111 = lut_at(i.x + 1, i.y + 1, i.z + 1);
    if (f.x > f.y) {
        if (f.y > f.z) {
            return (1.0 - f.x) * c000 + (f.x - f.y) * lut_at(i.x + 1, i.y, i.z)
                + (f.y - f.z) * lut_at(i.x + 1, i.y + 1, i.z) + f.z * c111;
        } else if (f.x > f.z) {
            return (1.0 - f.x) * c000 + (f.x - f.z) * lut_at(i.x + 1, i.y, i.z)
                + (f.z - f.y) * lut_at(i.x + 1, i.y, i.z + 1) + f.y * c111;
        } else {
            return (1.0 - f.z) * c000 + (f.z - f.x) * lut_at(i.x, i.y, i.z + 1)
                + (f.x - f.y) * lut_at(i.x + 1, i.y, i.z + 1) + f.y * c111;
        }
    } else if (f.z > f.y) {
        return (1.0 - f.z) * c000 + (f.z - f.y) * lut_at(i.x, i.y, i.z + 1)
            + (f.y - f.x) * lut_at(i.x, i.y + 1, i.z + 1) + f.x * c111;
    } else if (f.z > f.x) {
        return (1.0 - f.y) * c000 + (f.y - f.z) * lut_at(i.x, i.y + 1, i.z)
            + (f.z - f.x) * lut_at(i.x, i.y + 1, i.z + 1) + f.x * c111;
    }
    return (1.0 - f.y) * c000 + (f.y - f.x) * lut_at(i.x, i.y + 1, i.z)
        + (f.x - f.z) * lut_at(i.x + 1, i.y + 1, i.z) + f.z * c111;
}
