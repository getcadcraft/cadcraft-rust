// CadCraft 3D: faces (lit, two-sided) and edges, camera-relative instances, reversed-Z depth.

struct Frame {
    // projection × view rotation (the eye is at the origin: instances arrive camera-relative)
    pv: mat4x4<f32>,
    // direction towards the light (view-independent, world axes), w = 1: sun shadows on
    light: vec4<f32>,
    // back-face colour (rgb) and how much a face's own colour shows on its back (a)
    back: vec4<f32>,
    // edge colour
    edge: vec4<f32>,
    // x: ambient, y: diffuse, z: x-ray alpha (1 = opaque), w: clip-space depth nudge for edges
    shade: vec4<f32>,
    // section cut: xyz = normal, w = offset, in the scene-origin frame; what lies where dot(n, p) > w is not drawn
    // (a zero normal: no cut)
    clip: vec4<f32>,
    // a = 1: faces drawn in this colour, unlit, both sides (hidden line)
    fill: vec4<f32>,
    // the sun's shadow camera: scene-origin frame -> shadow clip (x, y in -1..1, depth 0..1)
    lpv: mat4x4<f32>,
    // the ground that catches shadows: centre (scene-origin frame) and half its side
    ground: vec4<f32>,
    // x: depth bias, y: normal bias (m), z: one shadow-map texel (uv), w: the ground's shadow opacity
    sh: vec4<f32>,
};
@group(0) @binding(0) var<uniform> fr: Frame;
@group(1) @binding(0) var smap: texture_depth_2d;
@group(1) @binding(1) var ssamp: sampler_comparison;

// How much of the sun reaches a point (1 = all), 3 x 3 filtered taps (three.js PCF soft shadows); outside the map: lit.
fn sunlit(rel: vec3<f32>, n: vec3<f32>) -> f32 {
    let p = fr.lpv * vec4<f32>(rel + n * fr.sh.y, 1.0);
    let uv = vec2<f32>(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5);
    let z = p.z + fr.sh.x;
    if uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 || z > 1.0 {
        return 1.0;
    }
    var s = 0.0;
    for (var i = -1; i <= 1; i++) {
        for (var j = -1; j <= 1; j++) {
            s += textureSampleCompareLevel(smap, ssamp, uv + vec2<f32>(f32(i), f32(j)) * fr.sh.z, z);
        }
    }
    return s / 9.0;
}

struct Inst {
    @location(3) m0: vec4<f32>,
    @location(4) m1: vec4<f32>,
    @location(5) m2: vec4<f32>,
    @location(6) m3: vec4<f32>,
    @location(7) n0: vec4<f32>,
    @location(8) n1: vec4<f32>,
    @location(9) n2: vec4<f32>,
    // a colour painted on the instance (alpha 0 = none: the faces keep their own)
    @location(10) tint: vec4<f32>,
};

struct FaceOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) n: vec3<f32>,
    @location(1) col: vec4<f32>,
    @location(2) rel: vec3<f32>,
};

fn cut(rel: vec3<f32>) -> bool {
    return dot(fr.clip.xyz, fr.clip.xyz) > 0.0 && dot(fr.clip.xyz, rel) > fr.clip.w;
}

@vertex
fn vs_face(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>, @location(2) c: vec4<f32>, i: Inst) -> FaceOut {
    let m = mat4x4<f32>(i.m0, i.m1, i.m2, i.m3);
    let nm = mat3x3<f32>(i.n0.xyz, i.n1.xyz, i.n2.xyz);
    var o: FaceOut;
    let w = m * vec4<f32>(p, 1.0);
    o.pos = fr.pv * w;
    o.rel = w.xyz;
    o.n = nm * n;
    o.col = select(c, i.tint, i.tint.a > 0.0);
    return o;
}

@fragment
fn fs_face(v: FaceOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    if cut(v.rel) {
        discard;
    }
    if fr.fill.a > 0.0 {
        return vec4<f32>(fr.fill.rgb, 1.0);
    }
    let len_n = length(v.n);
    var n = select(vec3<f32>(0.0, 0.0, 1.0), v.n / len_n, len_n > 1e-12);
    if !front {
        n = -n;
    }
    let l = normalize(fr.light.xyz);
    var direct = max(dot(n, l), 0.0);
    if fr.light.w > 0.0 && direct > 0.0 && fr.shade.y > 0.0 {
        direct *= sunlit(v.rel, n);
    }
    let lit = fr.shade.x + fr.shade.y * direct;
    let base = select(mix(fr.back.rgb, v.col.rgb, fr.back.a), v.col.rgb, front);
    return vec4<f32>(base * lit, fr.shade.z);
}

struct EdgeOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) rel: vec3<f32>,
};

@vertex
fn vs_edge(@location(0) p: vec3<f32>, i: Inst) -> EdgeOut {
    let m = mat4x4<f32>(i.m0, i.m1, i.m2, i.m3);
    var o: EdgeOut;
    let w = m * vec4<f32>(p, 1.0);
    let c = fr.pv * w;
    o.rel = w.xyz;
    // reversed Z: nearer is larger; nudge edges towards the eye so they win over their own faces
    o.pos = vec4<f32>(c.xy, c.z + fr.shade.w * c.w, c.w);
    return o;
}

@fragment
fn fs_edge(v: EdgeOut) -> @location(0) vec4<f32> {
    if cut(v.rel) {
        discard;
    }
    return fr.edge;
}

// ---- the sun's shadow map: faces seen from the sun, depth only ----

struct ShadowOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) rel: vec3<f32>,
};

@vertex
fn vs_shadow(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>, @location(2) c: vec4<f32>, i: Inst) -> ShadowOut {
    let m = mat4x4<f32>(i.m0, i.m1, i.m2, i.m3);
    var o: ShadowOut;
    let w = m * vec4<f32>(p, 1.0);
    o.pos = fr.lpv * w;
    o.rel = w.xyz;
    return o;
}

@fragment
fn fs_shadow(v: ShadowOut) {
    if cut(v.rel) {
        discard;
    }
}

// ---- the ground under the model: transparent, dark where the sun is blocked (three.js ShadowMaterial) ----

struct GroundOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) rel: vec3<f32>,
};

@vertex
fn vs_ground(@builtin(vertex_index) k: u32) -> GroundOut {
    // two triangles, counter-clockwise seen from above
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let c = corners[min(k, 5u)];
    var o: GroundOut;
    let rel = vec3<f32>(fr.ground.xy + c * fr.ground.w, fr.ground.z);
    o.pos = fr.pv * vec4<f32>(rel, 1.0);
    o.rel = rel;
    return o;
}

@fragment
fn fs_ground(v: GroundOut) -> @location(0) vec4<f32> {
    let s = sunlit(v.rel, vec3<f32>(0.0, 0.0, 1.0));
    return vec4<f32>(0.0, 0.0, 0.0, fr.sh.w * (1.0 - s));
}
