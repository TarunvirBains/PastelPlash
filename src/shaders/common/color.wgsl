// ---------------------------------------------------------------- color

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((max(c, vec3<f32>(0.0)) + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

fn cbrt(x: f32) -> f32 {
    return sign(x) * pow(abs(x), 1.0 / 3.0);
}

fn linear_to_oklab(c: vec3<f32>) -> vec3<f32> {
    let l = cbrt(0.4122214708 * c.r + 0.5363325363 * c.g + 0.0514459929 * c.b);
    let m = cbrt(0.2119034982 * c.r + 0.6806995451 * c.g + 0.1073969566 * c.b);
    let s = cbrt(0.0883024619 * c.r + 0.2817188376 * c.g + 0.6299787005 * c.b);
    return vec3<f32>(
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s);
}

fn oklab_to_linear(c: vec3<f32>) -> vec3<f32> {
    let l0 = c.x + 0.3963377774 * c.y + 0.2158037573 * c.z;
    let m0 = c.x - 0.1055613458 * c.y - 0.0638541728 * c.z;
    let s0 = c.x - 0.0894841775 * c.y - 1.2914855480 * c.z;
    let l = l0 * l0 * l0;
    let m = m0 * m0 * m0;
    let s = s0 * s0 * s0;
    return vec3<f32>(
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s);
}

fn srgb_to_oklab(c: vec3<f32>) -> vec3<f32> { return linear_to_oklab(srgb_to_linear(c)); }

fn lightness(c: vec4<f32>) -> f32 { return srgb_to_oklab(c.rgb).x; }

fn luminance(c: vec3<f32>) -> f32 {
    let l = srgb_to_linear(c);
    return 0.2126 * l.r + 0.7152 * l.g + 0.0722 * l.b;
}
