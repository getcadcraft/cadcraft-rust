// CadCraft 3D: faces (lit, two-sided) and edges, camera-relative instances, reversed-Z depth.

struct Frame {
    // projection × view rotation (the eye is at the origin: instances arrive camera-relative)
    pv: mat4x4<f32>,
    // direction towards the light (view-independent, world axes), w unused
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
};
@group(0) @binding(0) var<uniform> fr: Frame;

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
    let lit = fr.shade.x + fr.shade.y * max(dot(n, normalize(fr.light.xyz)), 0.0);
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
