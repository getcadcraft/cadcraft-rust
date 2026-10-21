// CadCraft rendered stills, High quality (0.5 render-pt.js): a path tracer in a compute shader. One sample a pixel a
// dispatch: a jittered camera ray, then up to six bounces (ten through glass) over a box tree of world triangles,
// with the sun and the lamps sampled at every bounce (shadow rays), the gradient dome lighting what escapes. The sum
// is kept per pixel; the picture divides it by the samples, tone-maps (ACES filmic) and encodes it (fs_out).

struct Lamp {
    p: vec4<f32>,
    d: vec4<f32>,
    c: vec4<f32>,
    // area: the panel's two half sides (xyz) — e1 · w/2, e2 · h/2
    e1: vec4<f32>,
    e2: vec4<f32>,
};

struct Pt {
    // clip → scene-origin frame (no jitter)
    inv_pv: mat4x4<f32>,
    // x, y: picture size; z: the sample being added; w: lamps
    size: vec4<f32>,
    up: vec4<f32>,
    // towards the sun, w: 1 = on; colour × intensity
    sun_dir: vec4<f32>,
    sun_col: vec4<f32>,
    zen: vec4<f32>,
    hor: vec4<f32>,
    gnd: vec4<f32>,
    // x: image light intensity, y: 1 = textures, z: max bounces, w: max bounces through glass
    env: vec4<f32>,
    // x: 1 / samples (output), y: exposure factor
    out: vec4<f32>,
    lamps: array<Lamp, 64>,
};

@group(0) @binding(0) var<uniform> pt: Pt;
// box tree: per node two vec4 — (min, first child or first triangle), (max, triangle count; 0 = inner node)
@group(0) @binding(1) var<storage, read> nodes: array<vec4<f32>>;
// per triangle three corners (xyz)
@group(0) @binding(2) var<storage, read> tpos: array<vec4<f32>>;
// per triangle: three normals (xyz, w: u of that corner), three (v of the corner, …), albedo + rough, metal glass layer
@group(0) @binding(3) var<storage, read> tattr: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read_write> acc: array<vec4<f32>>;
@group(0) @binding(5) var mtex: texture_2d_array<f32>;
@group(0) @binding(6) var msamp: sampler;

const PI: f32 = 3.14159265;
const BIG: f32 = 1e30;

var<private> seed: u32;

fn pcg(v: u32) -> u32 {
    let s = v * 747796405u + 2891336453u;
    let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}
fn rnd() -> f32 {
    seed = pcg(seed);
    return f32(seed >> 8u) / 16777216.0;
}

struct Hit {
    t: f32,
    tri: u32,
    b1: f32,
    b2: f32,
};

fn hit_tri(o: vec3<f32>, d: vec3<f32>, i: u32, tmax: f32) -> vec3<f32> {
    let a = tpos[i * 3u].xyz;
    let e1 = tpos[i * 3u + 1u].xyz - a;
    let e2 = tpos[i * 3u + 2u].xyz - a;
    let h = cross(d, e2);
    let det = dot(e1, h);
    if abs(det) < 1e-12 {
        return vec3<f32>(-1.0, 0.0, 0.0);
    }
    let f = 1.0 / det;
    let s = o - a;
    let u = f * dot(s, h);
    if u < 0.0 || u > 1.0 {
        return vec3<f32>(-1.0, 0.0, 0.0);
    }
    let q = cross(s, e1);
    let v = f * dot(d, q);
    if v < 0.0 || u + v > 1.0 {
        return vec3<f32>(-1.0, 0.0, 0.0);
    }
    let t = f * dot(e2, q);
    if t <= 1e-5 || t >= tmax {
        return vec3<f32>(-1.0, 0.0, 0.0);
    }
    return vec3<f32>(t, u, v);
}

fn slab(o: vec3<f32>, inv: vec3<f32>, lo: vec3<f32>, hi: vec3<f32>, tmax: f32) -> f32 {
    let t0 = (lo - o) * inv;
    let t1 = (hi - o) * inv;
    let a = min(t0, t1);
    let b = max(t0, t1);
    let near = max(max(a.x, a.y), max(a.z, 0.0));
    let far = min(min(b.x, b.y), min(b.z, tmax));
    return select(BIG, near, near <= far);
}

fn trace(o: vec3<f32>, d: vec3<f32>, tmax0: f32) -> Hit {
    var best = Hit(tmax0, 0xffffffffu, 0.0, 0.0);
    let inv = 1.0 / select(d, vec3<f32>(1e-20), abs(d) < vec3<f32>(1e-20));
    var stack: array<u32, 48>;
    var sp = 0;
    stack[0] = 0u;
    sp = 1;
    var guard = 0;
    while sp > 0 && guard < 20000 {
        guard++;
        sp--;
        let ni = stack[sp];
        let a = nodes[ni * 2u];
        let b = nodes[ni * 2u + 1u];
        if slab(o, inv, a.xyz, b.xyz, best.t) >= BIG {
            continue;
        }
        let count = bitcast<u32>(b.w);
        let first = bitcast<u32>(a.w);
        if count > 0u {
            for (var k = 0u; k < count; k++) {
                let r = hit_tri(o, d, first + k, best.t);
                if r.x > 0.0 {
                    best = Hit(r.x, first + k, r.y, r.z);
                }
            }
        } else if sp + 2 <= 48 {
            stack[sp] = first;
            stack[sp + 1] = first + 1u;
            sp += 2;
        }
    }
    return best;
}

fn dome(dir: vec3<f32>) -> vec3<f32> {
    let z = dot(normalize(dir), pt.up.xyz);
    if z >= 0.0 {
        return mix(pt.hor.rgb, pt.zen.rgb, pow(z, 0.6));
    }
    return mix(pt.hor.rgb, pt.gnd.rgb, pow(-z, 0.4));
}

struct Surf {
    n: vec3<f32>,
    albedo: vec3<f32>,
    rough: f32,
    metal: f32,
    glass: f32,
};

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

fn surface(h: Hit) -> Surf {
    let i = h.tri * 6u;
    let w0 = 1.0 - h.b1 - h.b2;
    let n0 = tattr[i];
    let n1 = tattr[i + 1u];
    let n2 = tattr[i + 2u];
    let vv = tattr[i + 3u];
    let col = tattr[i + 4u];
    let m = tattr[i + 5u];
    var s: Surf;
    s.n = normalize(n0.xyz * w0 + n1.xyz * h.b1 + n2.xyz * h.b2);
    s.albedo = col.rgb;
    s.rough = clamp(col.w, 0.03, 1.0);
    s.metal = m.x;
    s.glass = m.y;
    if m.z >= 0.0 && pt.env.y > 0.0 {
        let uv = vec2<f32>(n0.w * w0 + n1.w * h.b1 + n2.w * h.b2, vv.x * w0 + vv.y * h.b1 + vv.z * h.b2);
        s.albedo = srgb_to_linear(textureSampleLevel(mtex, msamp, uv, i32(round(m.z)), 0.0).rgb);
    }
    return s;
}

// how much light gets from o along d over a distance (glass lets some through)
fn visible(o: vec3<f32>, d: vec3<f32>, dist: f32) -> vec3<f32> {
    var t = vec3<f32>(1.0);
    var p = o;
    var left = dist;
    for (var k = 0; k < 4; k++) {
        let h = trace(p, d, left);
        if h.tri == 0xffffffffu {
            return t;
        }
        let s = surface(h);
        if s.glass <= 0.0 {
            return vec3<f32>(0.0);
        }
        t *= s.albedo * s.glass;
        p = p + d * (h.t + 1e-4);
        left -= h.t + 1e-4;
    }
    return t;
}

fn basis(n: vec3<f32>) -> mat3x3<f32> {
    let a = select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), abs(n.x) > 0.9);
    let t = normalize(cross(n, a));
    let b = cross(n, t);
    return mat3x3<f32>(t, b, n);
}

fn ggx_d(nh: f32, a: f32) -> f32 {
    let a2 = a * a;
    let dd = nh * nh * (a2 - 1.0) + 1.0;
    return a2 / (PI * dd * dd);
}

// the surface's response to light from l seen from v: diffuse + GGX specular, times cos
fn brdf(s: Surf, v: vec3<f32>, l: vec3<f32>) -> vec3<f32> {
    let nl = dot(s.n, l);
    let nv = dot(s.n, v);
    if nl <= 0.0 || nv <= 0.0 {
        return vec3<f32>(0.0);
    }
    let h = normalize(v + l);
    let a = s.rough * s.rough;
    let k = a / 2.0;
    let g = (nl / (nl * (1.0 - k) + k)) * (nv / (nv * (1.0 - k) + k));
    let f0 = mix(vec3<f32>(0.04), s.albedo, s.metal);
    let f = f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - max(dot(v, h), 0.0), 5.0);
    let spec = f * ggx_d(max(dot(s.n, h), 0.0), a) * g / max(4.0 * nl * nv, 1e-4);
    let diff = (vec3<f32>(1.0) - f) * s.albedo * (1.0 - s.metal) / PI;
    return (diff + spec) * nl;
}

// the sun and every lamp, seen from p on s
fn direct(p: vec3<f32>, s: Surf, v: vec3<f32>) -> vec3<f32> {
    var c = vec3<f32>(0.0);
    let o = p + s.n * 1e-4;
    if pt.sun_dir.w > 0.0 {
        let l = normalize(pt.sun_dir.xyz);
        let f = brdf(s, v, l);
        if dot(f, f) > 0.0 {
            c += f * pt.sun_col.rgb * visible(o, l, BIG);
        }
    }
    let count = i32(pt.size.w);
    for (var k = 0; k < count; k++) {
        let L = pt.lamps[k];
        var q = L.p.xyz;
        var extra = 1.0;
        if L.p.w > 1.5 {
            // a panel: a point on it, lighting its own side
            q = q + L.e1.xyz * (2.0 * rnd() - 1.0) + L.e2.xyz * (2.0 * rnd() - 1.0);
        } else {
            // a bulb of radius e1.w
            let z = 2.0 * rnd() - 1.0;
            let ph = 2.0 * PI * rnd();
            let r = sqrt(max(0.0, 1.0 - z * z));
            q = q + vec3<f32>(r * cos(ph), r * sin(ph), z) * L.e1.w;
        }
        let dv = q - o;
        let d2 = max(dot(dv, dv), 1e-4);
        let dist = sqrt(d2);
        let l = dv / dist;
        if L.p.w > 1.5 {
            extra = max(dot(-l, normalize(L.d.xyz)), 0.0);
        } else if L.p.w > 0.5 {
            extra = smoothstep(L.d.w, L.c.w, dot(-l, normalize(L.d.xyz)));
        }
        if extra <= 0.0 {
            continue;
        }
        let f = brdf(s, v, l);
        if dot(f, f) > 0.0 {
            c += f * L.c.rgb * extra / d2 * visible(o, l, dist - 2e-4);
        }
    }
    return c;
}

@compute @workgroup_size(8, 8)
fn trace_px(@builtin(global_invocation_id) gid: vec3<u32>) {
    let w = u32(pt.size.x);
    let hgt = u32(pt.size.y);
    if gid.x >= w || gid.y >= hgt {
        return;
    }
    let pix = gid.y * w + gid.x;
    seed = pcg(pix * 9781u + u32(pt.size.z) * 6271u + 1u);
    // the camera ray through a jittered point of the pixel
    let jx = (f32(gid.x) + rnd()) / f32(w) * 2.0 - 1.0;
    let jy = 1.0 - (f32(gid.y) + rnd()) / f32(hgt) * 2.0;
    let a = pt.inv_pv * vec4<f32>(jx, jy, 1.0, 1.0);
    let b = pt.inv_pv * vec4<f32>(jx, jy, 0.5, 1.0);
    var o = a.xyz / a.w;
    var d = normalize(b.xyz / b.w - o);
    var thru = vec3<f32>(1.0);
    var col = vec3<f32>(0.0);
    var bounces = 0;
    var glassy = 0;
    loop {
        let h = trace(o, d, BIG);
        if h.tri == 0xffffffffu {
            // the dome: the background for camera rays, the image light after a bounce
            col += thru * dome(d) * select(pt.env.x, 1.0, bounces == 0 && glassy == 0);
            break;
        }
        var s = surface(h);
        let p = o + d * h.t;
        let front = dot(s.n, d) < 0.0;
        if !front {
            s.n = -s.n;
        }
        let v = -d;
        if s.glass > 0.0 {
            // thin glass: Fresnel's share reflects, the rest goes on through, tinted
            let cosi = max(dot(v, s.n), 0.0);
            let fr = 0.04 + 0.96 * pow(1.0 - cosi, 5.0);
            glassy++;
            if glassy > i32(pt.env.w) {
                break;
            }
            if rnd() < fr {
                d = reflect(d, s.n);
                o = p + s.n * 1e-4;
            } else {
                thru *= mix(vec3<f32>(1.0), s.albedo, 0.5) * s.glass;
                o = p + d * 1e-4;
            }
            continue;
        }
        col += thru * direct(p, s, v);
        bounces++;
        if bounces > i32(pt.env.z) {
            break;
        }
        // the next direction: specular (GGX) or diffuse (cosine), by how shiny the surface is
        let ps = clamp(mix(0.15, 1.0, s.metal) * (1.0 - s.rough * 0.6), 0.05, 0.95);
        let tbn = basis(s.n);
        var l: vec3<f32>;
        if rnd() < ps {
            let a = s.rough * s.rough;
            let u1 = rnd();
            let u2 = rnd();
            let th = atan(a * sqrt(u1 / max(1.0 - u1, 1e-6)));
            let ph = 2.0 * PI * u2;
            let hm = tbn * vec3<f32>(sin(th) * cos(ph), sin(th) * sin(ph), cos(th));
            l = reflect(-v, hm);
            if dot(l, s.n) <= 0.0 {
                break;
            }
            let nh = max(dot(s.n, hm), 1e-4);
            let pdf_s = ggx_d(nh, a) * nh / max(4.0 * max(dot(v, hm), 1e-4), 1e-6);
            let pdf = ps * pdf_s + (1.0 - ps) * max(dot(s.n, l), 0.0) / PI;
            thru *= brdf(s, v, l) / max(pdf, 1e-6);
        } else {
            let u1 = rnd();
            let u2 = rnd();
            let r = sqrt(u1);
            let ph = 2.0 * PI * u2;
            l = tbn * vec3<f32>(r * cos(ph), r * sin(ph), sqrt(max(0.0, 1.0 - u1)));
            let hm = normalize(v + l);
            let a = s.rough * s.rough;
            let nh = max(dot(s.n, hm), 1e-4);
            let pdf_s = ggx_d(nh, a) * nh / max(4.0 * max(dot(v, hm), 1e-4), 1e-6);
            let pdf = ps * pdf_s + (1.0 - ps) * max(dot(s.n, l), 0.0) / PI;
            thru *= brdf(s, v, l) / max(pdf, 1e-6);
        }
        // Russian roulette after three bounces
        if bounces >= 3 {
            let q = clamp(max(thru.x, max(thru.y, thru.z)), 0.05, 1.0);
            if rnd() > q {
                break;
            }
            thru /= q;
        }
        if !(thru.x < 1e6 && thru.y < 1e6 && thru.z < 1e6) {
            break;
        }
        o = p + s.n * 1e-4;
        d = l;
    }
    if col.x >= 0.0 && col.y >= 0.0 && col.z >= 0.0 && col.x < 1e5 && col.y < 1e5 && col.z < 1e5 {
        acc[pix] = acc[pix] + vec4<f32>(col, 1.0);
    } else {
        acc[pix] = acc[pix] + vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
}
