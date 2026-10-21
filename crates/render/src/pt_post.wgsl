// CadCraft High quality stills: the samples summed per pixel, divided, tone-mapped (ACES filmic) and encoded to sRGB.
struct Post {
    // x: picture width, y: exposure factor
    k: vec4<f32>,
};
@group(0) @binding(0) var<uniform> pto: Post;
@group(0) @binding(1) var<storage, read> sum: array<vec4<f32>>;

// ---- the picture: the sum over the samples, tone-mapped and encoded ----

struct Out {
    @builtin(position) pos: vec4<f32>,
};

@vertex
fn vs_full(@builtin(vertex_index) k: u32) -> Out {
    var xy = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    var o: Out;
    o.pos = vec4<f32>(xy[min(k, 2u)], 0.0, 1.0);
    return o;
}


fn rrt_odt(v: vec3<f32>) -> vec3<f32> {
    let a = v * (v + 0.0245786) - 0.000090537;
    let b = v * (0.983729 * v + 0.4329510) + 0.238081;
    return a / b;
}

@fragment
fn fs_out(v: Out) -> @location(0) vec4<f32> {
    let x = u32(v.pos.x);
    let y = u32(v.pos.y);
    let s = sum[y * u32(pto.k.x) + x];
    var c = s.rgb / max(s.w, 1.0);
    let inm = mat3x3<f32>(vec3<f32>(0.59719, 0.07600, 0.02840), vec3<f32>(0.35458, 0.90834, 0.13383), vec3<f32>(0.04823, 0.01566, 0.83777));
    let outm = mat3x3<f32>(vec3<f32>(1.60475, -0.10208, -0.00327), vec3<f32>(-0.53108, 1.10813, -0.07276), vec3<f32>(-0.07367, -0.00605, 1.07602));
    c = c * (pto.k.y / 0.6);
    c = clamp(outm * rrt_odt(inm * c), vec3<f32>(0.0), vec3<f32>(1.0));
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055);
    return vec4<f32>(select(hi, lo, c <= vec3<f32>(0.0031308)), 1.0);
}
