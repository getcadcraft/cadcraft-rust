//! strokefont.js — a small single-stroke font of our own, for drawing text from imported drawings as edges (0.4 M1).
//! Glyphs are drawn on a grid where the capital height is 9, the x-height 6 and descenders go to −3; each glyph is a
//! list of polylines plus its width. Arcs are elliptical arcs sampled every 15°.
//!
//!   text_strokes(text, h, wf, oblique) → {lines, width, missing}: text-local coordinates (origin at the left end of the
//!     baseline, x along the text, y up, drawing units; h = capital height, wf = width factor, oblique = slant in
//!     radians). `missing` counts characters the font has no glyph for (drawn as a small box).
//!   clean_dxf_text(s) → TEXT/ATTRIB value with %%d %%p %%c %%% decoded and %%u %%o dropped
//!   clean_mtext(s) → MTEXT value as plain lines (format codes removed, \P → newline, stacked fractions as a/b)

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::jsx::{cos, sin, tan};

type P2 = [f64; 2];
type Glyph = (f64, Vec<Vec<P2>>);

const GAP: f64 = 2.0;

/// Points of an elliptical arc, degrees, a0 → a1.
fn e(cx: f64, cy: f64, rx: f64, ry: f64, a0: f64, a1: f64) -> Vec<P2> {
    let n = (2.0f64).max(((a1 - a0).abs() / 15.0).ceil());
    let mut out = Vec::new();
    let mut i = 0.0;
    while i <= n {
        let a = ((a0 + ((a1 - a0) * i) / n) * std::f64::consts::PI) / 180.0;
        out.push([cx + rx * cos(a), cy + ry * sin(a)]);
        i += 1.0;
    }
    out
}
fn ring(cx: f64, cy: f64, rx: f64, ry: f64) -> Vec<P2> {
    e(cx, cy, rx, ry, 0.0, 360.0)
}
fn dot(x: f64, y: f64) -> Vec<P2> {
    vec![[x, y], [x + 0.6, y], [x + 0.6, y + 0.6], [x, y + 0.6], [x, y]]
}
fn cat(parts: Vec<Vec<P2>>) -> Vec<P2> {
    parts.into_iter().flatten().collect()
}
fn pl(pts: &[P2]) -> Vec<P2> {
    pts.to_vec()
}
fn scale_y(lines: Vec<Vec<P2>>, s: f64, dy: f64) -> Vec<Vec<P2>> {
    lines.into_iter().map(|l| l.into_iter().map(|[x, y]| [x, y * s + dy]).collect()).collect()
}

fn glyphs() -> &'static HashMap<char, Glyph> {
    static G: OnceLock<HashMap<char, Glyph>> = OnceLock::new();
    G.get_or_init(build)
}

#[allow(clippy::too_many_lines)]
fn build() -> HashMap<char, Glyph> {
    let mut g: HashMap<char, Glyph> = HashMap::new();
    let s_upper = || vec![cat(vec![e(3.0, 6.75, 3.0, 2.25, 20.0, 270.0), e(3.0, 2.25, 3.0, 2.25, 90.0, -160.0)])];
    let mut put = |c: char, w: f64, lines: Vec<Vec<P2>>| {
        g.insert(c, (w, lines));
    };
    put(' ', 4.0, vec![]);
    put('A', 6.0, vec![pl(&[[0.0, 0.0], [3.0, 9.0], [6.0, 0.0]]), pl(&[[1.0, 3.0], [5.0, 3.0]])]);
    put(
        'B',
        6.5,
        vec![cat(vec![
            pl(&[[0.0, 4.5], [0.0, 0.0], [4.25, 0.0]]),
            e(4.25, 2.25, 2.25, 2.25, -90.0, 90.0),
            pl(&[[4.25, 4.5], [0.0, 4.5], [0.0, 9.0], [4.0, 9.0]]),
            e(4.0, 6.75, 2.25, 2.25, 90.0, -90.0),
            pl(&[[4.0, 4.5]]),
        ])],
    );
    put('C', 6.5, vec![e(3.5, 4.5, 3.5, 4.5, 45.0, 315.0)]);
    put(
        'D',
        6.5,
        vec![cat(vec![pl(&[[0.0, 0.0], [0.0, 9.0], [2.5, 9.0]]), e(2.5, 4.5, 4.0, 4.5, 90.0, -90.0), pl(&[[2.5, 0.0], [0.0, 0.0]])])],
    );
    put('E', 5.5, vec![pl(&[[5.5, 9.0], [0.0, 9.0], [0.0, 0.0], [5.5, 0.0]]), pl(&[[0.0, 4.5], [4.5, 4.5]])]);
    put('F', 5.5, vec![pl(&[[5.5, 9.0], [0.0, 9.0], [0.0, 0.0]]), pl(&[[0.0, 4.5], [4.5, 4.5]])]);
    put('G', 7.0, vec![cat(vec![e(3.5, 4.5, 3.5, 4.5, 40.0, 360.0), pl(&[[4.0, 4.5]])]), pl(&[[7.0, 4.5], [7.0, 0.8]])]);
    put('H', 6.0, vec![pl(&[[0.0, 0.0], [0.0, 9.0]]), pl(&[[6.0, 0.0], [6.0, 9.0]]), pl(&[[0.0, 4.5], [6.0, 4.5]])]);
    put('I', 2.0, vec![pl(&[[0.0, 9.0], [2.0, 9.0]]), pl(&[[1.0, 9.0], [1.0, 0.0]]), pl(&[[0.0, 0.0], [2.0, 0.0]])]);
    put('J', 5.0, vec![cat(vec![pl(&[[5.0, 9.0], [5.0, 2.5]]), e(2.5, 2.5, 2.5, 2.5, 0.0, -180.0)])]);
    put('K', 6.0, vec![pl(&[[0.0, 0.0], [0.0, 9.0]]), pl(&[[6.0, 9.0], [0.0, 3.0]]), pl(&[[2.0, 5.0], [6.0, 0.0]])]);
    put('L', 5.0, vec![pl(&[[0.0, 9.0], [0.0, 0.0], [5.0, 0.0]])]);
    put('M', 7.0, vec![pl(&[[0.0, 0.0], [0.0, 9.0], [3.5, 3.0], [7.0, 9.0], [7.0, 0.0]])]);
    put('N', 6.0, vec![pl(&[[0.0, 0.0], [0.0, 9.0], [6.0, 0.0], [6.0, 9.0]])]);
    put('O', 7.0, vec![ring(3.5, 4.5, 3.5, 4.5)]);
    put('P', 6.25, vec![cat(vec![pl(&[[0.0, 0.0], [0.0, 9.0], [4.0, 9.0]]), e(4.0, 6.75, 2.25, 2.25, 90.0, -90.0), pl(&[[0.0, 4.5]])])]);
    put('Q', 7.0, vec![ring(3.5, 4.5, 3.5, 4.5), pl(&[[4.5, 2.0], [7.0, -0.5]])]);
    put(
        'R',
        6.25,
        vec![
            cat(vec![pl(&[[0.0, 0.0], [0.0, 9.0], [4.0, 9.0]]), e(4.0, 6.75, 2.25, 2.25, 90.0, -90.0), pl(&[[0.0, 4.5]])]),
            pl(&[[3.5, 4.5], [6.25, 0.0]]),
        ],
    );
    put('S', 6.0, s_upper());
    put('T', 6.0, vec![pl(&[[0.0, 9.0], [6.0, 9.0]]), pl(&[[3.0, 9.0], [3.0, 0.0]])]);
    put('U', 6.0, vec![cat(vec![pl(&[[0.0, 9.0], [0.0, 3.0]]), e(3.0, 3.0, 3.0, 3.0, 180.0, 360.0), pl(&[[6.0, 9.0]])])]);
    put('V', 6.0, vec![pl(&[[0.0, 9.0], [3.0, 0.0], [6.0, 9.0]])]);
    put('W', 8.0, vec![pl(&[[0.0, 9.0], [2.0, 0.0], [4.0, 6.0], [6.0, 0.0], [8.0, 9.0]])]);
    put('X', 6.0, vec![pl(&[[0.0, 9.0], [6.0, 0.0]]), pl(&[[0.0, 0.0], [6.0, 9.0]])]);
    put('Y', 6.0, vec![pl(&[[0.0, 9.0], [3.0, 4.5], [6.0, 9.0]]), pl(&[[3.0, 4.5], [3.0, 0.0]])]);
    put('Z', 6.0, vec![pl(&[[0.0, 9.0], [6.0, 9.0], [0.0, 0.0], [6.0, 0.0]])]);

    put('a', 5.0, vec![pl(&[[5.0, 6.0], [5.0, 0.0]]), ring(2.5, 3.0, 2.5, 3.0)]);
    put('b', 5.0, vec![pl(&[[0.0, 9.0], [0.0, 0.0]]), ring(2.5, 3.0, 2.5, 3.0)]);
    put('c', 5.0, vec![e(2.5, 3.0, 2.5, 3.0, 45.0, 315.0)]);
    put('d', 5.0, vec![pl(&[[5.0, 9.0], [5.0, 0.0]]), ring(2.5, 3.0, 2.5, 3.0)]);
    put('e', 5.0, vec![cat(vec![pl(&[[0.0, 3.0], [5.0, 3.0]]), e(2.5, 3.0, 2.5, 3.0, 0.0, 315.0)])]);
    put('f', 4.5, vec![cat(vec![pl(&[[4.5, 8.6]]), e(3.0, 7.5, 1.5, 1.5, 45.0, 180.0), pl(&[[1.5, 0.0]])]), pl(&[[0.0, 6.0], [3.5, 6.0]])]);
    put('g', 5.0, vec![ring(2.5, 3.0, 2.5, 3.0), cat(vec![pl(&[[5.0, 6.0], [5.0, -1.0]]), e(2.5, -1.0, 2.5, 2.0, 0.0, -150.0)])]);
    put('h', 5.0, vec![pl(&[[0.0, 9.0], [0.0, 0.0]]), cat(vec![e(2.5, 3.5, 2.5, 2.5, 170.0, 0.0), pl(&[[5.0, 0.0]])])]);
    put('i', 0.6, vec![pl(&[[0.3, 0.0], [0.3, 6.0]]), dot(0.0, 8.0)]);
    put('j', 2.1, vec![cat(vec![pl(&[[1.8, 6.0], [1.8, -1.5]]), e(0.6, -1.5, 1.2, 1.5, 0.0, -110.0)]), dot(1.5, 8.0)]);
    put('k', 5.0, vec![pl(&[[0.0, 9.0], [0.0, 0.0]]), pl(&[[4.5, 6.0], [0.0, 2.0]]), pl(&[[1.5, 3.2], [5.0, 0.0]])]);
    put('l', 0.6, vec![pl(&[[0.3, 9.0], [0.3, 0.0]])]);
    put(
        'm',
        8.0,
        vec![
            pl(&[[0.0, 0.0], [0.0, 6.0]]),
            cat(vec![e(2.0, 4.0, 2.0, 2.0, 175.0, 0.0), pl(&[[4.0, 0.0]])]),
            cat(vec![e(6.0, 4.0, 2.0, 2.0, 175.0, 0.0), pl(&[[8.0, 0.0]])]),
        ],
    );
    put('n', 5.0, vec![pl(&[[0.0, 0.0], [0.0, 6.0]]), cat(vec![e(2.5, 3.5, 2.5, 2.5, 170.0, 0.0), pl(&[[5.0, 0.0]])])]);
    put('o', 5.0, vec![ring(2.5, 3.0, 2.5, 3.0)]);
    put('p', 5.0, vec![pl(&[[0.0, 6.0], [0.0, -3.0]]), ring(2.5, 3.0, 2.5, 3.0)]);
    put('q', 5.0, vec![pl(&[[5.0, 6.0], [5.0, -3.0]]), ring(2.5, 3.0, 2.5, 3.0)]);
    put('r', 4.0, vec![pl(&[[0.0, 0.0], [0.0, 6.0]]), e(3.0, 3.5, 3.0, 2.5, 170.0, 60.0)]);
    put('s', 4.5, scale_y(vec![cat(vec![e(2.25, 4.5, 2.25, 1.5, 20.0, 270.0), e(2.25, 1.5, 2.25, 1.5, 90.0, -160.0)])], 1.0, 0.0));
    put(
        't',
        3.5,
        vec![
            cat(vec![pl(&[[1.5, 8.0], [1.5, 1.0]]), e(2.5, 1.0, 1.0, 1.0, 180.0, 270.0), pl(&[[3.5, 0.0]])]),
            pl(&[[0.0, 6.0], [3.5, 6.0]]),
        ],
    );
    put('u', 5.0, vec![cat(vec![pl(&[[0.0, 6.0], [0.0, 2.5]]), e(2.5, 2.5, 2.5, 2.5, 180.0, 360.0)]), pl(&[[5.0, 6.0], [5.0, 0.0]])]);
    put('v', 5.0, vec![pl(&[[0.0, 6.0], [2.5, 0.0], [5.0, 6.0]])]);
    put('w', 7.0, vec![pl(&[[0.0, 6.0], [1.75, 0.0], [3.5, 4.5], [5.25, 0.0], [7.0, 6.0]])]);
    put('x', 5.0, vec![pl(&[[0.0, 6.0], [5.0, 0.0]]), pl(&[[0.0, 0.0], [5.0, 6.0]])]);
    put('y', 5.0, vec![pl(&[[0.0, 6.0], [2.5, 0.0]]), pl(&[[5.0, 6.0], [2.5, 0.0], [1.2, -3.0]])]);
    put('z', 5.0, vec![pl(&[[0.0, 6.0], [5.0, 6.0], [0.0, 0.0], [5.0, 0.0]])]);

    put('0', 5.0, vec![ring(2.5, 4.5, 2.5, 4.5)]);
    put('1', 4.0, vec![pl(&[[1.0, 7.5], [2.5, 9.0], [2.5, 0.0]]), pl(&[[1.0, 0.0], [4.0, 0.0]])]);
    put('2', 5.0, vec![cat(vec![e(2.5, 6.5, 2.5, 2.5, 160.0, -30.0), pl(&[[0.0, 0.0], [5.0, 0.0]])])]);
    put('3', 5.0, vec![cat(vec![e(2.5, 6.75, 2.5, 2.25, 150.0, -90.0), e(2.5, 2.25, 2.5, 2.25, 90.0, -150.0)])]);
    put('4', 5.0, vec![pl(&[[3.5, 0.0], [3.5, 9.0], [0.0, 2.5], [5.0, 2.5]])]);
    put('5', 5.0, vec![cat(vec![pl(&[[4.5, 9.0], [0.5, 9.0], [0.2, 5.2]]), e(2.5, 3.0, 2.5, 3.0, 140.0, -150.0)])]);
    put('6', 5.0, vec![ring(2.5, 2.75, 2.5, 2.75), pl(&[[0.0, 2.75], [0.3, 5.5], [1.2, 7.8], [2.5, 9.0], [4.0, 8.8]])]);
    put('7', 5.0, vec![pl(&[[0.0, 9.0], [5.0, 9.0], [1.5, 0.0]])]);
    put('8', 5.0, vec![ring(2.5, 6.9, 2.1, 2.1), ring(2.5, 2.4, 2.5, 2.4)]);
    put('9', 5.0, vec![ring(2.5, 6.25, 2.5, 2.75), pl(&[[5.0, 6.25], [4.7, 3.5], [3.8, 1.2], [2.5, 0.0], [1.0, 0.2]])]);

    put('.', 0.6, vec![dot(0.0, 0.0)]);
    put(',', 1.0, vec![pl(&[[0.8, 0.6], [0.8, 0.0], [0.0, -1.5]])]);
    put(':', 0.6, vec![dot(0.0, 0.0), dot(0.0, 5.0)]);
    put(';', 1.0, vec![dot(0.2, 5.0), pl(&[[0.8, 0.6], [0.8, 0.0], [0.0, -1.5]])]);
    put('-', 4.0, vec![pl(&[[0.0, 4.5], [4.0, 4.5]])]);
    put('_', 6.0, vec![pl(&[[0.0, -1.0], [6.0, -1.0]])]);
    put('+', 5.0, vec![pl(&[[0.0, 4.5], [5.0, 4.5]]), pl(&[[2.5, 2.0], [2.5, 7.0]])]);
    put('=', 5.0, vec![pl(&[[0.0, 3.3], [5.0, 3.3]]), pl(&[[0.0, 5.7], [5.0, 5.7]])]);
    put('/', 5.0, vec![pl(&[[0.0, -0.5], [5.0, 9.5]])]);
    put('\\', 5.0, vec![pl(&[[0.0, 9.5], [5.0, -0.5]])]);
    put('|', 0.0, vec![pl(&[[0.0, -1.5], [0.0, 10.0]])]);
    put('(', 2.0, vec![e(4.0, 4.5, 4.0, 6.0, 132.0, 228.0)]);
    put(')', 2.0, vec![e(-2.0, 4.5, 4.0, 6.0, 48.0, -48.0)]);
    put('[', 2.0, vec![pl(&[[2.0, 10.0], [0.0, 10.0], [0.0, -1.0], [2.0, -1.0]])]);
    put(']', 2.0, vec![pl(&[[0.0, 10.0], [2.0, 10.0], [2.0, -1.0], [0.0, -1.0]])]);
    put('{', 2.5, vec![pl(&[[2.5, 10.0], [1.5, 9.5], [1.2, 5.2], [0.0, 4.5], [1.2, 3.8], [1.5, -0.5], [2.5, -1.0]])]);
    put('}', 2.5, vec![pl(&[[0.0, 10.0], [1.0, 9.5], [1.3, 5.2], [2.5, 4.5], [1.3, 3.8], [1.0, -0.5], [0.0, -1.0]])]);
    put('<', 5.0, vec![pl(&[[5.0, 8.0], [0.0, 4.5], [5.0, 1.0]])]);
    put('>', 5.0, vec![pl(&[[0.0, 8.0], [5.0, 4.5], [0.0, 1.0]])]);
    put('\'', 0.0, vec![pl(&[[0.0, 9.0], [0.0, 6.5]])]);
    put('"', 2.0, vec![pl(&[[0.0, 9.0], [0.0, 6.5]]), pl(&[[2.0, 9.0], [2.0, 6.5]])]);
    put('`', 1.5, vec![pl(&[[0.0, 9.5], [1.5, 8.0]])]);
    put('^', 5.0, vec![pl(&[[0.0, 6.0], [2.5, 9.0], [5.0, 6.0]])]);
    put('~', 5.0, vec![pl(&[[0.0, 4.0], [1.2, 5.0], [2.5, 4.5], [3.8, 4.0], [5.0, 5.0]])]);
    put('!', 0.6, vec![pl(&[[0.3, 9.0], [0.3, 2.5]]), dot(0.0, 0.0)]);
    put('?', 5.0, vec![cat(vec![e(2.5, 6.75, 2.5, 2.25, 160.0, -60.0), pl(&[[2.5, 2.5]])]), dot(2.2, 0.0)]);
    put('*', 5.0, vec![pl(&[[2.5, 3.0], [2.5, 9.0]]), pl(&[[0.0, 7.5], [5.0, 4.5]]), pl(&[[0.0, 4.5], [5.0, 7.5]])]);
    put(
        '#',
        6.0,
        vec![pl(&[[2.0, 0.0], [2.5, 9.0]]), pl(&[[4.0, 0.0], [4.5, 9.0]]), pl(&[[0.0, 3.0], [6.0, 3.0]]), pl(&[[0.0, 6.0], [6.0, 6.0]])],
    );
    put('%', 6.0, vec![ring(1.25, 7.5, 1.25, 1.5), ring(4.75, 1.5, 1.25, 1.5), pl(&[[0.0, 0.0], [6.0, 9.0]])]);
    put(
        '&',
        6.5,
        vec![pl(&[
            [6.5, 0.0],
            [1.2, 6.0],
            [1.2, 7.8],
            [2.5, 9.0],
            [3.8, 7.8],
            [3.8, 6.6],
            [0.0, 3.5],
            [0.0, 1.2],
            [1.3, 0.0],
            [3.3, 0.0],
            [6.0, 3.5],
        ])],
    );
    put('@', 8.0, vec![ring(4.0, 4.5, 1.8, 2.0), cat(vec![pl(&[[5.8, 6.5], [5.8, 3.0], [6.8, 2.6]]), e(4.0, 4.5, 4.0, 4.5, 10.0, 300.0)])]);
    let mut dollar = s_upper();
    dollar.push(pl(&[[3.0, 10.0], [3.0, -1.0]]));
    put('$', 6.0, dollar);
    put('°', 2.5, vec![ring(1.25, 7.75, 1.25, 1.25)]);
    put('±', 5.0, vec![pl(&[[0.0, 5.5], [5.0, 5.5]]), pl(&[[2.5, 3.0], [2.5, 8.0]]), pl(&[[0.0, 1.0], [5.0, 1.0]])]);
    put('Ø', 7.0, vec![ring(3.5, 4.5, 3.5, 4.5), pl(&[[0.0, -0.5], [7.0, 9.5]])]);
    put('ø', 5.0, vec![ring(2.5, 3.0, 2.5, 3.0), pl(&[[0.0, -0.5], [5.0, 6.5]])]);
    put('×', 5.0, vec![pl(&[[0.5, 2.0], [4.5, 7.0]]), pl(&[[0.5, 7.0], [4.5, 2.0]])]);
    put('²', 3.0, scale_y(vec![pl(&[[0.0, 7.0], [0.3, 8.7], [1.5, 9.4], [2.7, 8.7], [3.0, 7.8], [0.0, 5.5], [3.0, 5.5]])], 1.0, 0.0));
    put('³', 3.0, vec![pl(&[[0.0, 9.4], [3.0, 9.4], [1.5, 7.8], [3.0, 7.0], [3.0, 6.0], [1.5, 5.5], [0.0, 6.0]])]);
    // a few look-alikes, so common Latin-1 letters still read
    let alikes = [
        ('À', 'A'),
        ('Á', 'A'),
        ('Â', 'A'),
        ('Ä', 'A'),
        ('É', 'E'),
        ('È', 'E'),
        ('Ê', 'E'),
        ('Í', 'I'),
        ('Ó', 'O'),
        ('Ö', 'O'),
        ('Ú', 'U'),
        ('Ü', 'U'),
        ('Ñ', 'N'),
        ('Ç', 'C'),
        ('à', 'a'),
        ('á', 'a'),
        ('â', 'a'),
        ('ä', 'a'),
        ('é', 'e'),
        ('è', 'e'),
        ('ê', 'e'),
        ('í', 'i'),
        ('ó', 'o'),
        ('ö', 'o'),
        ('ú', 'u'),
        ('ü', 'u'),
        ('ñ', 'n'),
        ('ç', 'c'),
        ('ß', 'B'),
    ];
    for (k, base) in alikes {
        if !g.contains_key(&k)
            && let Some(v) = g.get(&base).cloned()
        {
            g.insert(k, v);
        }
    }
    g
}

/// The box drawn for a character the font has no glyph for.
fn box_glyph() -> &'static Glyph {
    static B: OnceLock<Glyph> = OnceLock::new();
    B.get_or_init(|| (4.0, vec![vec![[0.0, 0.0], [4.0, 0.0], [4.0, 7.0], [0.0, 7.0], [0.0, 0.0]]]))
}

/// What `textStrokes` returns.
#[derive(Clone, Debug, Default)]
pub struct Strokes {
    pub lines: Vec<Vec<P2>>,
    pub width: f64,
    pub missing: f64,
}

/// `textStrokes(text, {h, wf, oblique})`.
pub fn text_strokes(text: &str, h: f64, wf: f64, oblique: f64) -> Strokes {
    let s = h / 9.0;
    let sx = s * if wf != 0.0 && !wf.is_nan() { wf } else { 1.0 };
    let sh = tan(if oblique != 0.0 && !oblique.is_nan() { oblique } else { 0.0 });
    let g = glyphs();
    let mut lines = Vec::new();
    let mut x = 0.0;
    let mut missing = 0.0;
    for ch in text.chars() {
        let gl: &Glyph = match g.get(&ch) {
            Some(v) => v,
            None if crate::num::is_space(ch) => g.get(&' ').unwrap_or(box_glyph()),
            None => {
                missing += 1.0;
                box_glyph()
            }
        };
        for l in &gl.1 {
            lines.push(l.iter().map(|&[px, py]| [x + px * sx + py * s * sh, py * s]).collect());
        }
        x += (gl.0 + GAP) * sx;
    }
    Strokes { lines, width: (0.0f64).max(x - GAP * sx), missing }
}

/// Just the width of `textStrokes(text, {h, wf, oblique}).width` (no lines built).
pub fn text_width(text: &str, h: f64, wf: f64) -> f64 {
    let s = h / 9.0;
    let sx = s * if wf != 0.0 && !wf.is_nan() { wf } else { 1.0 };
    let g = glyphs();
    let mut x = 0.0;
    for ch in text.chars() {
        let w = match g.get(&ch) {
            Some(v) => v.0,
            None if crate::num::is_space(ch) => 4.0,
            None => 4.0,
        };
        x += (w + GAP) * sx;
    }
    (0.0f64).max(x - GAP * sx)
}

/// `cleanDxfText(s)`: %%nnn (a character code), %%d °, %%p ±, %%c Ø, %%% %, %%u / %%o dropped.
pub fn clean_dxf_text(s: &str) -> String {
    if !s.contains("%%") {
        return s.to_string();
    }
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    let mut last = 0usize;
    while i < b.len() {
        if b.get(i) == Some(&b'%') && b.get(i.saturating_add(1)) == Some(&b'%') {
            let c0 = b.get(i.saturating_add(2)).copied();
            let three = b.get(i.saturating_add(2)..i.saturating_add(5));
            let rep: Option<(String, usize)> = if let Some(d) = three.filter(|d| d.iter().all(u8::is_ascii_digit)) {
                let n = d.iter().fold(0u32, |a, &c| a.saturating_mul(10).saturating_add(u32::from(c.saturating_sub(b'0'))));
                // String.fromCharCode of 0..999: one UTF-16 unit, never a surrogate
                Some((char::from_u32(n).unwrap_or('\u{FFFD}').to_string(), 5))
            } else {
                match c0 {
                    Some(b'd' | b'D') => Some(("°".into(), 3)),
                    Some(b'p' | b'P') => Some(("±".into(), 3)),
                    Some(b'c' | b'C') => Some(("Ø".into(), 3)),
                    Some(b'%') => Some(("%".into(), 3)),
                    Some(b'u' | b'U' | b'o' | b'O') => Some((String::new(), 3)),
                    _ => None,
                }
            };
            if let Some((r, n)) = rep {
                out.push_str(s.get(last..i).unwrap_or(""));
                out.push_str(&r);
                i = i.saturating_add(n);
                last = i;
                continue;
            }
        }
        i = i.saturating_add(1);
    }
    out.push_str(s.get(last..).unwrap_or(""));
    out
}

/// Replace every non-overlapping match found by `at(s, i)` (Some((replacement, length)) when a match starts at byte
/// i), scanning left to right as a global regex does.
fn replace_all(s: &str, mut at: impl FnMut(&str, usize) -> Option<(String, usize)>) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    let mut last = 0usize;
    while i < s.len() {
        if !s.is_char_boundary(i) {
            i = i.saturating_add(1);
            continue;
        }
        if let Some((r, n)) = at(s, i) {
            out.push_str(s.get(last..i).unwrap_or(""));
            out.push_str(&r);
            i = i.saturating_add(n.max(1));
            last = i;
        } else {
            i = i.saturating_add(1);
        }
    }
    out.push_str(s.get(last..).unwrap_or(""));
    out
}

/// `cleanMtext(s)`: MTEXT value → plain text with "\n" between lines.
pub fn clean_mtext(s: &str) -> String {
    // \U+XXXX → the UTF-16 unit (pairs of them join; a lone surrogate becomes U+FFFD: Spec gap, JS-engine quirk —
    // either way the font draws it as a box)
    let t1 = if s.contains("\\U+") {
        let mut units: Vec<u16> = Vec::with_capacity(s.len());
        let b = s.as_bytes();
        let mut i = 0usize;
        let mut last = 0usize;
        while i < b.len() {
            if b.get(i..i.saturating_add(3)) == Some(b"\\U+")
                && let Some(h) = b.get(i.saturating_add(3)..i.saturating_add(7)).filter(|h| h.iter().all(u8::is_ascii_hexdigit))
            {
                units.extend(s.get(last..i).unwrap_or("").encode_utf16());
                let v = h.iter().fold(0u16, |a, &c| {
                    a.wrapping_mul(16).wrapping_add(u16::try_from(char::from(c).to_digit(16).unwrap_or(0)).unwrap_or(0))
                });
                units.push(v);
                i = i.saturating_add(7);
                last = i;
                continue;
            }
            i = i.saturating_add(1);
        }
        units.extend(s.get(last..).unwrap_or("").encode_utf16());
        String::from_utf16_lossy(&units)
    } else {
        s.to_string()
    };
    // stacked fraction a^b, a/b, a#b: \S…[^/#]…;
    let t2 = replace_all(&t1, |s, i| {
        let rest = s.get(i..)?.strip_prefix("\\S")?;
        let semi = rest.find(';')?;
        let k = rest.get(..semi)?.find(['^', '/', '#'])?;
        Some((format!("{}/{}", rest.get(..k)?, rest.get(k.saturating_add(1)..semi)?), semi.saturating_add(3)))
    });
    // format codes with a value: \f…; \H…; etc. (\P; is a new line)
    let t3 = replace_all(&t2, |s, i| {
        let rest = s.get(i..)?;
        let mut ch = rest.chars();
        if ch.next()? != '\\' {
            return None;
        }
        let code = ch.next()?;
        if !"fFcCHhWwTtQqAapP".contains(code) {
            return None;
        }
        let body = rest.get(2..)?;
        let stop = body.find([';', '\\'])?;
        if body.as_bytes().get(stop) != Some(&b';') {
            return None;
        }
        let m = rest.get(..stop.saturating_add(3))?;
        Some((if m == "\\P;" { "\n".into() } else { String::new() }, m.len()))
    });
    let t4 = t3.replace("\\P", "\n").replace("\\~", " ");
    let t5 = replace_all(&t4, |s, i| {
        let rest = s.get(i..)?;
        let mut ch = rest.chars();
        if ch.next()? != '\\' {
            return None;
        }
        let c = ch.next()?;
        "LlOoKkNX".contains(c).then(|| (String::new(), 2))
    });
    let t6 = t5.replace("\\\\", "\u{1}").replace("\\{", "\u{2}").replace("\\}", "\u{3}");
    let t7: String = t6.chars().filter(|&c| c != '{' && c != '}').collect();
    let t8 = t7.replace('\u{1}', "\\").replace('\u{2}', "{").replace('\u{3}', "}");
    clean_dxf_text(&t8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes() {
        assert_eq!(clean_dxf_text("45%%d %%p1 %%c5 100%%% %%uA%%065"), "45° ±1 Ø5 100% AA");
        assert_eq!(clean_mtext("{\\fArial|b1;Hello}\\PWorld \\S1/2; a\\~b \\\\x"), "Hello\nWorld 1/2 a b \\x");
        assert_eq!(clean_mtext("\\U+00E9t\\U+00E9"), "été");
    }

    #[test]
    fn widths() {
        let s = text_strokes("Lot 4", 1.0, 1.0, 0.0);
        assert!(s.width > 0.0 && s.missing == 0.0 && !s.lines.is_empty());
        assert_eq!(text_strokes("\u{263a}", 9.0, 1.0, 0.0).missing, 1.0);
        assert_eq!(text_width("Lot 4", 1.0, 1.0), s.width);
    }
}
