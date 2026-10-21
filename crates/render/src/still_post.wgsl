// CadCraft rendered stills: the passes summed (additive blending), then the picture — divided by the passes done,
// tone-mapped as three.js ACESFilmicToneMapping (exposure in stops) and encoded to sRGB once.
@group(0) @binding(0) var src: texture_2d<f32>;
struct Post {
    // x: 1 / passes, y: exposure factor (2^stops)
    k: vec4<f32>,
};
@group(0) @binding(1) var<uniform> post: Post;

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

@fragment
fn fs_add(v: Out) -> @location(0) vec4<f32> {
    return vec4<f32>(textureLoad(src, vec2<i32>(v.pos.xy), 0).rgb, 1.0);
}

fn rrt_odt(v: vec3<f32>) -> vec3<f32> {
    let a = v * (v + 0.0245786) - 0.000090537;
    let b = v * (0.983729 * v + 0.4329510) + 0.238081;
    return a / b;
}

fn aces(c0: vec3<f32>) -> vec3<f32> {
    let inm = mat3x3<f32>(vec3<f32>(0.59719, 0.07600, 0.02840), vec3<f32>(0.35458, 0.90834, 0.13383), vec3<f32>(0.04823, 0.01566, 0.83777));
    let outm = mat3x3<f32>(vec3<f32>(1.60475, -0.10208, -0.00327), vec3<f32>(-0.53108, 1.10813, -0.07276), vec3<f32>(-0.07367, -0.00605, 1.07602));
    var c = c0 * (post.k.y / 0.6);
    c = inm * c;
    c = rrt_odt(c);
    c = outm * c;
    return clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
}

fn encode(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055);
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

@fragment
fn fs_out(v: Out) -> @location(0) vec4<f32> {
    let c = textureLoad(src, vec2<i32>(v.pos.xy), 0).rgb * post.k.x;
    return vec4<f32>(encode(aces(c)), 1.0);
}
