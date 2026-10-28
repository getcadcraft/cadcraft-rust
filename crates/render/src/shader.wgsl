// CadCraft 2D renderer: anti-aliased line quads (one instance per segment) and solid fills.
// Coordinates arrive relative to the space's origin (f32); the view centre is relative to the same origin,
// so large survey coordinates stay steady (the f64 subtraction happens on the CPU).

struct View {
    centre: vec2<f32>,      // view centre, relative to the space origin
    scale: vec2<f32>,       // world units → clip (x, y), includes the zoom
    rot: vec2<f32>,         // cos, sin of the view twist
    viewport: vec2<f32>,    // size in physical pixels
    ink: vec4<f32>,         // colour of "ink" entities (ACI 7)
    line_px: f32,           // line width in physical pixels
    fill_alpha: f32,        // pattern fills are drawn at this alpha
    print: f32,             // print view: everything in ink, lines as wide as their weight
    px_per_mm: f32,         // physical px per paper mm (sheets in print view or with line weights shown), else 0
};
@group(0) @binding(0) var<uniform> view: View;

fn to_clip(p: vec2<f32>) -> vec2<f32> {
    let d = p - view.centre;
    let r = vec2<f32>(d.x * view.rot.x - d.y * view.rot.y, d.x * view.rot.y + d.y * view.rot.x);
    return r * view.scale;
}

// the alpha byte: the line weight in 0.05 mm steps, + 128 for a true colour (below: "ink")
fn alpha8(c: vec4<f32>) -> f32 { return round(c.a * 255.0); }

fn colour(c: vec4<f32>) -> vec4<f32> {
    // print = 1: the print view (all in ink); 2: the drawing's colours with line weights (LWDISPLAY)
    if (alpha8(c) < 127.5 || (view.print > 0.5 && view.print < 1.5)) { return view.ink; }
    return vec4<f32>(c.rgb, 1.0);
}

// line width in physical pixels: a hairline, or in print view the weight (dwg-view.js: at least its screen weight,
// 0.25 mm = 1 px, on a sheet its true width on paper)
fn width_px(c: vec4<f32>) -> f32 {
    if (view.print < 0.5) { return view.line_px; }
    let a = alpha8(c);
    let lw = select(a, a - 128.0, a > 127.5) * 0.05;
    // LWDISPLAY (lwdisplay.js lwPixels): 0.25 mm or less = 1 px, above in proportion, at most 12 px; on a sheet at
    // least the true width on paper
    if (view.print > 1.5) { return max(view.line_px * clamp(lw / 0.25, 1.0, 12.0), lw * view.px_per_mm); }
    return max(view.line_px * max(1.0, floor(lw / 0.25 + 0.5)), lw * view.px_per_mm);
}

struct LineOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) colour: vec4<f32>,
    @location(1) dist: f32,     // signed distance from the centre line in pixels
    @location(2) half: f32,
};

@vertex
fn vs_line(@builtin(vertex_index) vi: u32, @location(0) a: vec2<f32>, @location(1) b: vec2<f32>, @location(2) c: vec4<f32>) -> LineOut {
    let ca = to_clip(a);
    let cb = to_clip(b);
    let px = 2.0 / view.viewport;               // one pixel in clip units
    var dir = (cb - ca) / px;                   // in pixels
    let len = length(dir);
    if (len < 1e-6) { dir = vec2<f32>(1.0, 0.0); } else { dir = dir / len; }
    let n = vec2<f32>(-dir.y, dir.x);
    let half = width_px(c) * 0.5 + 0.75;        // + AA fringe
    // quad corners: 0 a-, 1 a+, 2 b-, 3 b+  (two triangles: 0 1 2, 2 1 3)
    var corner = array<vec2<f32>, 6>(vec2<f32>(0.0, -1.0), vec2<f32>(0.0, 1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 1.0));
    let k = corner[vi];
    let base = mix(ca, cb, k.x);
    let ext = (dir * (select(-0.75, 0.75, k.x > 0.5))) * px;   // a little past the ends so joins close
    var out: LineOut;
    out.pos = vec4<f32>(base + ext + n * k.y * half * px, 0.0, 1.0);
    out.colour = colour(c);
    out.dist = k.y * half;
    out.half = half;
    return out;
}

@fragment
fn fs_line(in: LineOut) -> @location(0) vec4<f32> {
    let edge = in.half - 0.75;
    let a = clamp(edge + 0.75 - abs(in.dist), 0.0, 1.0);
    return vec4<f32>(in.colour.rgb, in.colour.a * a);
}

struct FillOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) colour: vec4<f32>,
};

@vertex
fn vs_fill(@location(0) p: vec2<f32>, @location(1) c: vec4<f32>, @location(2) pattern: f32) -> FillOut {
    var out: FillOut;
    out.pos = vec4<f32>(to_clip(p), 0.0, 1.0);
    let k = colour(c);
    out.colour = vec4<f32>(k.rgb, k.a * select(1.0, view.fill_alpha, pattern > 0.5));
    return out;
}

@fragment
fn fs_fill(in: FillOut) -> @location(0) vec4<f32> {
    return in.colour;
}
