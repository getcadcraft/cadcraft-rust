// CadCraft rendered stills (0.5 render-core.js "Fast"): one jittered pass of the scene into a linear HDR target — sky
// dome, ground, physically based faces lit by the sun, one sampled sky light (both shadow mapped), the dome's image
// light and lamps — summed over passes, then divided, tone-mapped (ACES filmic, exposure) and encoded to sRGB.

struct Lamp {
    // xyz: position (scene-origin frame), w: kind (0 point, 1 spot, 2 area)
    p: vec4<f32>,
    // xyz: direction it points, w: cos of the cone (spot, area)
    d: vec4<f32>,
    // rgb: colour × intensity (candela), w: cos of the penumbra's inner edge
    c: vec4<f32>,
};

struct Still {
    // projection × view rotation × T(origin − eye), jittered under a pixel
    pv: mat4x4<f32>,
    // the inverse of projection × view rotation (no jitter): clip → view ray (world axes)
    inv_pr: mat4x4<f32>,
    // the sun's and the sky light's shadow cameras (scene-origin frame → shadow clip)
    lpv_sun: mat4x4<f32>,
    lpv_sky: mat4x4<f32>,
    // the eye, scene-origin frame
    eye: vec4<f32>,
    up: vec4<f32>,
    // direction towards the sun / the sky light; rgb × intensity; w of col: 1 = on
    sun_dir: vec4<f32>,
    sun_col: vec4<f32>,
    sky_dir: vec4<f32>,
    sky_col: vec4<f32>,
    // the dome: zenith, horizon, ground (linear)
    zen: vec4<f32>,
    hor: vec4<f32>,
    gnd: vec4<f32>,
    // x: image light intensity, y: fog near, z: fog far, w: 1 = the ground fogs
    env: vec4<f32>,
    // the ground disc: centre (scene-origin frame) and radius; colour (w: 1 = drawn)
    ground: vec4<f32>,
    ground_col: vec4<f32>,
    // x: depth bias, y: normal bias (m), z: shadow texel, w: 1 = edges drawn
    sh: vec4<f32>,
    edge: vec4<f32>,
    // a section cut: keep n · p ≤ w (n zero: none)
    clip: vec4<f32>,
    // x: lamps, y: 1/passes (output), z: exposure factor, w: 1 = textures
    misc: vec4<f32>,
    lamps: array<Lamp, 64>,
};
@group(0) @binding(0) var<uniform> st: Still;
@group(1) @binding(0) var smap_sun: texture_depth_2d;
@group(1) @binding(1) var smap_sky: texture_depth_2d;
@group(1) @binding(2) var ssamp: sampler_comparison;
@group(1) @binding(3) var mtex: texture_2d_array<f32>;
@group(1) @binding(4) var msamp: sampler;

const PI: f32 = 3.14159265;

struct Inst {
    @location(3) m0: vec4<f32>,
    @location(4) m1: vec4<f32>,
    @location(5) m2: vec4<f32>,
    @location(6) m3: vec4<f32>,
    @location(7) n0: vec4<f32>,
    @location(8) n1: vec4<f32>,
    @location(9) n2: vec4<f32>,
    @location(10) tint: vec4<f32>,
};

fn cut(rel: vec3<f32>) -> bool {
    return dot(st.clip.xyz, st.clip.xyz) > 0.0 && dot(st.clip.xyz, rel) > st.clip.w;
}

fn shadow(smap: texture_depth_2d, lpv: mat4x4<f32>, rel: vec3<f32>, n: vec3<f32>) -> f32 {
    let p = lpv * vec4<f32>(rel + n * st.sh.y, 1.0);
    let uv = vec2<f32>(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5);
    let z = p.z + st.sh.x;
    if uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 || z > 1.0 {
        return 1.0;
    }
    return textureSampleCompareLevel(smap, ssamp, uv, z);
}

// the dome's colour along a direction (render-core.js skyDome)
fn dome(dir: vec3<f32>) -> vec3<f32> {
    let z = dot(normalize(dir), st.up.xyz);
    if z >= 0.0 {
        return mix(st.hor.rgb, st.zen.rgb, pow(z, 0.6));
    }
    return mix(st.hor.rgb, st.gnd.rgb, pow(-z, 0.4));
}

// the dome's light falling on a surface facing n, over π (a cosine-weighted average of the dome)
fn dome_irr(n: vec3<f32>) -> vec3<f32> {
    let z = dot(n, st.up.xyz);
    let sky = mix(st.hor.rgb, st.zen.rgb, 0.55);
    let gr = mix(st.hor.rgb, st.gnd.rgb, 0.6);
    return mix(gr, sky, clamp(0.5 + 0.5 * z, 0.0, 1.0));
}

fn ggx(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, rough: f32, f0: vec3<f32>) -> vec3<f32> {
    let h = normalize(v + l);
    let nl = max(dot(n, l), 0.0);
    let nv = max(dot(n, v), 1e-4);
    let nh = max(dot(n, h), 0.0);
    let vh = max(dot(v, h), 0.0);
    let a = max(rough * rough, 0.002);
    let a2 = a * a;
    let dd = nh * nh * (a2 - 1.0) + 1.0;
    let d = a2 / (PI * dd * dd);
    let k = a / 2.0;
    let g = (nl / (nl * (1.0 - k) + k)) * (nv / (nv * (1.0 - k) + k));
    let f = f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - vh, 5.0);
    return f * d * g / max(4.0 * nl * nv, 1e-4);
}

// one light's share: direct diffuse + specular, Lambert over π as three.js does
fn light(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, radiance: vec3<f32>, albedo: vec3<f32>, rough: f32, metal: f32) -> vec3<f32> {
    let nl = max(dot(n, l), 0.0);
    if nl <= 0.0 {
        return vec3<f32>(0.0);
    }
    let f0 = mix(vec3<f32>(0.04), albedo, metal);
    let diff = albedo * (1.0 - metal) / PI;
    return radiance * nl * (diff + ggx(n, v, l, rough, f0));
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

struct FaceOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) n: vec3<f32>,
    @location(1) col: vec4<f32>,
    @location(2) rel: vec3<f32>,
    @location(3) uvl: vec3<f32>,
    // rough, metal, glass
    @location(4) look: vec3<f32>,
};

@vertex
fn vs_face(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>, @location(2) c: vec4<f32>, @location(11) uvl: vec3<f32>,
    @location(12) look: vec4<f32>, i: Inst) -> FaceOut {
    let m = mat4x4<f32>(i.m0, i.m1, i.m2, i.m3);
    let nm = mat3x3<f32>(i.n0.xyz, i.n1.xyz, i.n2.xyz);
    var o: FaceOut;
    let w = m * vec4<f32>(p, 1.0);
    o.pos = st.pv * w;
    o.rel = w.xyz;
    o.n = nm * n;
    o.col = c;
    o.uvl = uvl;
    o.look = look.xyz;
    return o;
}

fn shade(v: FaceOut, front: bool) -> vec4<f32> {
    let texel = textureSample(mtex, msamp, v.uvl.xy, max(i32(round(v.uvl.z)), 0));
    let len_n = length(v.n);
    var n = select(vec3<f32>(0.0, 0.0, 1.0), v.n / len_n, len_n > 1e-12);
    if !front {
        n = -n;
    }
    var albedo = srgb_to_linear(v.col.rgb);
    if v.uvl.z >= 0.0 && st.misc.w > 0.0 {
        albedo = srgb_to_linear(texel.rgb);
    }
    let rough = clamp(v.look.x, 0.03, 1.0);
    let metal = v.look.y;
    let vdir = normalize(st.eye.xyz - v.rel);
    var c = vec3<f32>(0.0);
    if st.sun_col.w > 0.0 {
        let l = normalize(st.sun_dir.xyz);
        if dot(n, l) > 0.0 {
            c += light(n, vdir, l, st.sun_col.rgb, albedo, rough, metal) * shadow(smap_sun, st.lpv_sun, v.rel, n);
        }
    }
    if st.sky_col.w > 0.0 {
        let l = normalize(st.sky_dir.xyz);
        if dot(n, l) > 0.0 {
            c += light(n, vdir, l, st.sky_col.rgb, albedo, rough, metal) * shadow(smap_sky, st.lpv_sky, v.rel, n);
        }
    }
    let count = i32(st.misc.x);
    for (var k = 0; k < count; k++) {
        let L = st.lamps[k];
        let d = L.p.xyz - v.rel;
        let dist2 = max(dot(d, d), 1e-4);
        let l = d / sqrt(dist2);
        var att = 1.0 / dist2;
        if L.p.w > 0.5 {
            // a spot's cone; a panel lights its half of the world as a wide soft cone (render-core.js)
            att *= smoothstep(L.d.w, L.c.w, dot(-l, normalize(L.d.xyz)));
        }
        c += light(n, vdir, l, L.c.rgb * att, albedo, rough, metal);
    }
    // the dome's image light: diffuse from its irradiance, specular from its colour along the reflection
    let f0 = mix(vec3<f32>(0.04), albedo, metal);
    let nv = max(dot(n, vdir), 0.0);
    let fr = f0 + (max(vec3<f32>(1.0 - rough), f0) - f0) * pow(1.0 - nv, 5.0);
    let r = reflect(-vdir, n);
    let spec_env = mix(dome(r), dome_irr(r), rough);
    c += st.env.x * (albedo * (1.0 - metal) * dome_irr(n) + fr * spec_env);
    let glass = v.look.z;
    if glass > 0.0 {
        return vec4<f32>(c, max(0.08, 1.0 - glass));
    }
    return vec4<f32>(c, 1.0);
}

@fragment
fn fs_face(v: FaceOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let s = shade(v, front);
    if cut(v.rel) || v.look.z > 0.0 {
        discard;
    }
    return s;
}

@fragment
fn fs_glass(v: FaceOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let s = shade(v, front);
    if cut(v.rel) || v.look.z <= 0.0 {
        discard;
    }
    return s;
}

// ---- shadow maps: faces seen from a light (glass casts none) ----

struct Shadow {
    lpv: mat4x4<f32>,
};
@group(0) @binding(1) var<uniform> shd: Shadow;

struct ShadowOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) rel: vec3<f32>,
    @location(1) glass: f32,
};

@vertex
fn vs_shadow(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>, @location(2) c: vec4<f32>, @location(11) uvl: vec3<f32>,
    @location(12) look: vec4<f32>, i: Inst) -> ShadowOut {
    let m = mat4x4<f32>(i.m0, i.m1, i.m2, i.m3);
    var o: ShadowOut;
    let w = m * vec4<f32>(p, 1.0);
    o.pos = shd.lpv * w;
    o.rel = w.xyz;
    o.glass = look.z;
    return o;
}

@fragment
fn fs_shadow(v: ShadowOut) {
    if cut(v.rel) || v.glass > 0.0 {
        discard;
    }
}

// ---- edges (settings.edges): dark, half see-through ----

struct EdgeOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) rel: vec3<f32>,
};

@vertex
fn vs_edge(@location(0) p: vec3<f32>, i: Inst) -> EdgeOut {
    let m = mat4x4<f32>(i.m0, i.m1, i.m2, i.m3);
    var o: EdgeOut;
    let w = m * vec4<f32>(p, 1.0);
    let c = st.pv * w;
    o.rel = w.xyz;
    o.pos = vec4<f32>(c.xy, c.z + 2e-6 * c.w, c.w);
    return o;
}

@fragment
fn fs_edge(v: EdgeOut) -> @location(0) vec4<f32> {
    if cut(v.rel) {
        discard;
    }
    return vec4<f32>(st.edge.rgb, st.edge.a);
}

// ---- the sky behind everything ----

struct SkyOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_full(@builtin(vertex_index) k: u32) -> SkyOut {
    var xy = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    let p = xy[min(k, 2u)];
    var o: SkyOut;
    o.pos = vec4<f32>(p, 0.0, 1.0);
    o.ndc = p;
    return o;
}

@fragment
fn fs_sky(v: SkyOut) -> @location(0) vec4<f32> {
    let a = st.inv_pr * vec4<f32>(v.ndc, 0.5, 1.0);
    let b = st.inv_pr * vec4<f32>(v.ndc, 0.25, 1.0);
    // reversed Z: depth 0.25 lies beyond 0.5
    let dir = b.xyz / b.w - a.xyz / a.w;
    return vec4<f32>(dome(dir), 1.0);
}

// ---- the ground: a matte disc under the model, shadowed, fading into the horizon ----

struct GroundOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) rel: vec3<f32>,
};

@vertex
fn vs_ground(@builtin(vertex_index) k: u32) -> GroundOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let c = corners[min(k, 5u)];
    var o: GroundOut;
    let rel = vec3<f32>(st.ground.xy + c * st.ground.w, st.ground.z);
    o.pos = st.pv * vec4<f32>(rel, 1.0);
    o.rel = rel;
    return o;
}

@fragment
fn fs_ground(v: GroundOut) -> @location(0) vec4<f32> {
    if length(v.rel.xy - st.ground.xy) > st.ground.w {
        discard;
    }
    let n = st.up.xyz;
    let albedo = st.ground_col.rgb;
    let vdir = normalize(st.eye.xyz - v.rel);
    var c = vec3<f32>(0.0);
    if st.sun_col.w > 0.0 {
        let l = normalize(st.sun_dir.xyz);
        c += light(n, vdir, l, st.sun_col.rgb, albedo, 1.0, 0.0) * shadow(smap_sun, st.lpv_sun, v.rel, n);
    }
    if st.sky_col.w > 0.0 {
        let l = normalize(st.sky_dir.xyz);
        c += light(n, vdir, l, st.sky_col.rgb, albedo, 1.0, 0.0) * shadow(smap_sky, st.lpv_sky, v.rel, n);
    }
    let count = i32(st.misc.x);
    for (var k = 0; k < count; k++) {
        let L = st.lamps[k];
        let d = L.p.xyz - v.rel;
        let dist2 = max(dot(d, d), 1e-4);
        let l = d / sqrt(dist2);
        var att = 1.0 / dist2;
        if L.p.w > 0.5 {
            att *= smoothstep(L.d.w, L.c.w, dot(-l, normalize(L.d.xyz)));
        }
        c += light(n, vdir, l, L.c.rgb * att, albedo, 1.0, 0.0);
    }
    c += st.env.x * albedo * dome_irr(n);
    if st.env.w > 0.0 {
        let f = smoothstep(st.env.y, st.env.z, length(st.eye.xyz - v.rel));
        c = mix(c, st.hor.rgb, f);
    }
    return vec4<f32>(c, 1.0);
}
