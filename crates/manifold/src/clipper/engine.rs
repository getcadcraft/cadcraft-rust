// Clipper2 clipper.engine.cpp (commit 46f6391), Copyright Angus Johnson 2010-2026, Boost Software License 1.0
// (https://www.boost.org/LICENSE_1_0.txt; the full text is at the top of clipper/mod.rs). Rust port 2026 CadCraft.
//
//! The Vatti clipper (`ClipperBase` + `ClipperD`'s path output), closed paths only, no polytree.
//!
//! The C++ links vertices, active edges, output points and output records with raw pointers; here each lives in an
//! arena (`Vec`) and links are indices (`NIL` = null). Nothing is freed while a clipper runs, so a pointer the C++
//! would leave dangling simply points at a dead record (the C++ never follows those on valid input). Every walk along
//! a linked list carries a [`Guard`] bounded by its arena, and every arena access is checked: on inconsistent
//! structures (hostile input) the operation fails with [`Fault`] instead of looping or reading out of bounds.
//!
//! Integer coordinates are within Clipper's range (±2^61) after scaling, so their sums and differences never overflow;
//! derived values (rounded projections) use wrapping arithmetic where the C++ would be undefined.
#![allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

use std::collections::BinaryHeap;

use super::sort::libcxx_sort;
use super::{
    ClipType, FillRule, P64, PD, Path64, PathD, cross_product_sign, dot_product, get_closest_point_on_segment, get_line_intersect_pt,
    is_collinear, nearbyint_i64, perpendic_dist_from_line_sqrd, segments_intersect,
};
use crate::util::{Fault, Guard, R};

const NIL: usize = usize::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PathType {
    Subject,
    Clip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JoinWith {
    NoJoin,
    Left,
    Right,
}

const VF_OPEN_START: u32 = 1;
const VF_OPEN_END: u32 = 2;
const VF_LOCAL_MAX: u32 = 4;
const VF_LOCAL_MIN: u32 = 8;

#[derive(Clone, Copy, Debug)]
struct Vertex {
    pt: P64,
    next: usize,
    prev: usize,
    flags: u32,
}

#[derive(Clone, Copy, Debug)]
struct LocalMinima {
    vertex: usize,
    polytype: PathType,
    is_open: bool,
}

#[derive(Clone, Copy, Debug)]
struct OutPt {
    pt: P64,
    next: usize,
    prev: usize,
    outrec: usize,
    horz: bool,
}

#[derive(Clone, Debug)]
struct OutRec {
    idx: usize,
    owner: usize,
    front_edge: usize,
    back_edge: usize,
    pts: usize,
    is_open: bool,
}

#[derive(Clone, Copy, Debug)]
struct Active {
    bot: P64,
    top: P64,
    curr_x: i64,
    dx: f64,
    wind_dx: i32,
    wind_cnt: i32,
    wind_cnt2: i32,
    outrec: usize,
    prev_in_ael: usize,
    next_in_ael: usize,
    prev_in_sel: usize,
    next_in_sel: usize,
    jump: usize,
    vertex_top: usize,
    local_min: usize,
    is_left_bound: bool,
    join_with: JoinWith,
}

impl Default for Active {
    fn default() -> Self {
        Active {
            bot: P64::default(),
            top: P64::default(),
            curr_x: 0,
            dx: 0.0,
            wind_dx: 1,
            wind_cnt: 0,
            wind_cnt2: 0,
            outrec: NIL,
            prev_in_ael: NIL,
            next_in_ael: NIL,
            prev_in_sel: NIL,
            next_in_sel: NIL,
            jump: NIL,
            vertex_top: NIL,
            local_min: NIL,
            is_left_bound: false,
            join_with: JoinWith::NoJoin,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct IntersectNode {
    pt: P64,
    edge1: usize,
    edge2: usize,
}

#[derive(Clone, Copy, Debug)]
struct HorzSegment {
    left_op: usize,
    right_op: usize,
    left_to_right: bool,
}

#[derive(Clone, Copy, Debug)]
struct HorzJoin {
    op1: usize,
    op2: usize,
}

/// `GetDx`.
#[inline]
fn get_dx(pt1: P64, pt2: P64) -> f64 {
    let dy = (pt2.y - pt1.y) as f64;
    if dy != 0.0 {
        (pt2.x - pt1.x) as f64 / dy
    } else if pt2.x > pt1.x {
        -f64::MAX
    } else {
        f64::MAX
    }
}

#[inline]
fn pts_really_close(a: P64, b: P64) -> bool {
    a.x.wrapping_sub(b.x).unsigned_abs() < 2 && a.y.wrapping_sub(b.y).unsigned_abs() < 2
}

#[inline]
fn area_triangle(p1: P64, p2: P64, p3: P64) -> f64 {
    (p3.y + p1.y) as f64 * (p3.x - p1.x) as f64 + (p1.y + p2.y) as f64 * (p1.x - p2.x) as f64 + (p2.y + p3.y) as f64 * (p2.x - p3.x) as f64
}

/// `ClipperBase` / `ClipperD` with its arenas.
pub(crate) struct Clipper {
    cliptype: ClipType,
    fillrule: FillRule,
    bot_y: i64,
    minima_sorted: bool,
    actives: usize,
    sel: usize,
    verts: Vec<Vertex>,
    locmins: Vec<LocalMinima>,
    minima_list: Vec<usize>,
    cur_locmin: usize,
    scanlines: BinaryHeap<i64>,
    intersect_nodes: Vec<IntersectNode>,
    horz_segs: Vec<HorzSegment>,
    horz_joins: Vec<HorzJoin>,
    actv: Vec<Active>,
    ops: Vec<OutPt>,
    outrecs: Vec<OutRec>,
    preserve_collinear: bool,
    reverse_solution: bool,
    succeeded: bool,
}

macro_rules! getter {
    ($name:ident, $name_mut:ident, $field:ident, $t:ty) => {
        #[inline]
        fn $name(&self, i: usize) -> R<&$t> {
            self.$field.get(i).ok_or(Fault::Index)
        }
        #[inline]
        fn $name_mut(&mut self, i: usize) -> R<&mut $t> {
            self.$field.get_mut(i).ok_or(Fault::Index)
        }
    };
}

impl Clipper {
    pub(crate) fn new() -> Clipper {
        Clipper {
            cliptype: ClipType::NoClip,
            fillrule: FillRule::EvenOdd,
            bot_y: 0,
            minima_sorted: false,
            actives: NIL,
            sel: NIL,
            verts: Vec::new(),
            locmins: Vec::new(),
            minima_list: Vec::new(),
            cur_locmin: 0,
            scanlines: BinaryHeap::new(),
            intersect_nodes: Vec::new(),
            horz_segs: Vec::new(),
            horz_joins: Vec::new(),
            actv: Vec::new(),
            ops: Vec::new(),
            outrecs: Vec::new(),
            preserve_collinear: true,
            reverse_solution: false,
            succeeded: true,
        }
    }

    getter!(v, v_mut, verts, Vertex);
    getter!(a, a_mut, actv, Active);
    getter!(op, op_mut, ops, OutPt);
    getter!(or, or_mut, outrecs, OutRec);

    fn lm(&self, i: usize) -> R<&LocalMinima> {
        self.locmins.get(i).ok_or(Fault::Index)
    }

    fn guard_ops(&self) -> Guard {
        Guard::new(self.ops.len())
    }
    fn guard_ael(&self) -> Guard {
        Guard::new(self.actv.len())
    }
    fn guard_or(&self) -> Guard {
        Guard::new(self.outrecs.len())
    }
    fn guard_v(&self) -> Guard {
        Guard::new(self.verts.len())
    }

    // ------------------------------------------------------------------ small helpers on actives

    fn is_open(&self, e: usize) -> R<bool> {
        let lm = self.a(e)?.local_min;
        Ok(self.lm(lm)?.is_open)
    }
    fn is_hot(&self, e: usize) -> R<bool> {
        Ok(self.a(e)?.outrec != NIL)
    }
    fn is_open_end_v(&self, v: usize) -> R<bool> {
        Ok(self.v(v)?.flags & (VF_OPEN_START | VF_OPEN_END) != 0)
    }
    fn is_open_end(&self, e: usize) -> R<bool> {
        self.is_open_end_v(self.a(e)?.vertex_top)
    }
    fn polytype(&self, e: usize) -> R<PathType> {
        Ok(self.lm(self.a(e)?.local_min)?.polytype)
    }
    fn is_same_polytype(&self, e1: usize, e2: usize) -> R<bool> {
        Ok(self.polytype(e1)? == self.polytype(e2)?)
    }
    fn is_horizontal(&self, e: usize) -> R<bool> {
        let a = self.a(e)?;
        Ok(a.top.y == a.bot.y)
    }
    fn is_front(&self, e: usize) -> R<bool> {
        let or = self.a(e)?.outrec;
        Ok(e == self.or(or)?.front_edge)
    }
    fn is_maxima_v(&self, v: usize) -> R<bool> {
        Ok(self.v(v)?.flags & VF_LOCAL_MAX != 0)
    }
    fn is_maxima(&self, e: usize) -> R<bool> {
        self.is_maxima_v(self.a(e)?.vertex_top)
    }
    fn is_joined(&self, e: usize) -> R<bool> {
        Ok(self.a(e)?.join_with != JoinWith::NoJoin)
    }
    fn set_dx(&mut self, e: usize) -> R<()> {
        let a = self.a_mut(e)?;
        a.dx = get_dx(a.bot, a.top);
        Ok(())
    }
    fn next_vertex(&self, e: usize) -> R<usize> {
        let a = self.a(e)?;
        let v = self.v(a.vertex_top)?;
        Ok(if a.wind_dx > 0 { v.next } else { v.prev })
    }
    fn prev_prev_vertex(&self, e: usize) -> R<usize> {
        let a = self.a(e)?;
        let v = self.v(a.vertex_top)?;
        Ok(if a.wind_dx > 0 { self.v(v.prev)?.prev } else { self.v(v.next)?.next })
    }
    fn top_x(&self, e: usize, current_y: i64) -> R<i64> {
        let a = self.a(e)?;
        Ok(if current_y == a.top.y || a.top.x == a.bot.x {
            a.top.x
        } else if current_y == a.bot.y {
            a.bot.x
        } else {
            a.bot.x.wrapping_add(nearbyint_i64(a.dx * current_y.wrapping_sub(a.bot.y) as f64))
        })
    }
    fn get_prev_hot_edge(&self, e: usize) -> R<usize> {
        let mut prev = self.a(e)?.prev_in_ael;
        let mut g = self.guard_ael();
        while prev != NIL && (self.is_open(prev)? || !self.is_hot(prev)?) {
            g.tick()?;
            prev = self.a(prev)?.prev_in_ael;
        }
        Ok(prev)
    }
    fn get_curr_y_maxima_vertex(&self, e: usize) -> R<usize> {
        let a = *self.a(e)?;
        let mut result = a.vertex_top;
        let mut g = self.guard_v();
        if a.wind_dx > 0 {
            while self.v(self.v(result)?.next)?.pt.y == self.v(result)?.pt.y {
                g.tick()?;
                result = self.v(result)?.next;
            }
        } else {
            while self.v(self.v(result)?.prev)?.pt.y == self.v(result)?.pt.y {
                g.tick()?;
                result = self.v(result)?.prev;
            }
        }
        Ok(if self.is_maxima_v(result)? { result } else { NIL })
    }
    fn get_maxima_pair(&self, e: usize) -> R<usize> {
        let vt = self.a(e)?.vertex_top;
        let mut e2 = self.a(e)?.next_in_ael;
        let mut g = self.guard_ael();
        while e2 != NIL {
            g.tick()?;
            if self.a(e2)?.vertex_top == vt {
                return Ok(e2);
            }
            e2 = self.a(e2)?.next_in_ael;
        }
        Ok(NIL)
    }

    // ------------------------------------------------------------------ output points and records

    fn new_op(&mut self, pt: P64, outrec: usize) -> usize {
        let i = self.ops.len();
        self.ops.push(OutPt { pt, next: i, prev: i, outrec, horz: false });
        i
    }

    fn duplicate_op(&mut self, op: usize, insert_after: bool) -> R<usize> {
        let o = *self.op(op)?;
        let r = self.new_op(o.pt, o.outrec);
        if insert_after {
            self.op_mut(r)?.next = o.next;
            self.op_mut(o.next)?.prev = r;
            self.op_mut(r)?.prev = op;
            self.op_mut(op)?.next = r;
        } else {
            self.op_mut(r)?.prev = o.prev;
            self.op_mut(o.prev)?.next = r;
            self.op_mut(r)?.next = op;
            self.op_mut(op)?.prev = r;
        }
        Ok(r)
    }

    fn dispose_op(&mut self, op: usize) -> R<usize> {
        let o = *self.op(op)?;
        self.op_mut(o.prev)?.next = o.next;
        self.op_mut(o.next)?.prev = o.prev;
        Ok(o.next)
    }

    fn dispose_out_pts(&mut self, outrec: usize) -> R<()> {
        self.or_mut(outrec)?.pts = NIL;
        Ok(())
    }

    fn new_outrec(&mut self) -> usize {
        let i = self.outrecs.len();
        self.outrecs.push(OutRec { idx: i, owner: NIL, front_edge: NIL, back_edge: NIL, pts: NIL, is_open: false });
        i
    }

    fn set_sides(&mut self, outrec: usize, start: usize, end: usize) -> R<()> {
        let o = self.or_mut(outrec)?;
        o.front_edge = start;
        o.back_edge = end;
        Ok(())
    }

    fn swap_outrecs(&mut self, e1: usize, e2: usize) -> R<()> {
        let or1 = self.a(e1)?.outrec;
        let or2 = self.a(e2)?.outrec;
        if or1 == or2 {
            let o = self.or_mut(or1)?;
            core::mem::swap(&mut o.front_edge, &mut o.back_edge);
            return Ok(());
        }
        if or1 != NIL {
            let o = self.or_mut(or1)?;
            if e1 == o.front_edge {
                o.front_edge = e2;
            } else {
                o.back_edge = e2;
            }
        }
        if or2 != NIL {
            let o = self.or_mut(or2)?;
            if e2 == o.front_edge {
                o.front_edge = e1;
            } else {
                o.back_edge = e1;
            }
        }
        self.a_mut(e1)?.outrec = or2;
        self.a_mut(e2)?.outrec = or1;
        Ok(())
    }

    fn area_op(&self, op: usize) -> R<f64> {
        let mut result = 0.0;
        let mut op2 = op;
        let mut g = self.guard_ops();
        loop {
            g.tick()?;
            let o = self.op(op2)?;
            let p = self.op(o.prev)?;
            result += (p.pt.y + o.pt.y) as f64 * (p.pt.x - o.pt.x) as f64;
            op2 = o.next;
            if op2 == op {
                break;
            }
        }
        Ok(result * 0.5)
    }

    fn get_real_outrec(&self, mut outrec: usize) -> R<usize> {
        let mut g = self.guard_or();
        while outrec != NIL && self.or(outrec)?.pts == NIL {
            g.tick()?;
            outrec = self.or(outrec)?.owner;
        }
        Ok(outrec)
    }

    fn uncouple_outrec(&mut self, e: usize) -> R<()> {
        let outrec = self.a(e)?.outrec;
        if outrec == NIL {
            return Ok(());
        }
        let o = self.or(outrec)?.clone();
        self.a_mut(o.front_edge)?.outrec = NIL;
        self.a_mut(o.back_edge)?.outrec = NIL;
        let o = self.or_mut(outrec)?;
        o.front_edge = NIL;
        o.back_edge = NIL;
        Ok(())
    }

    fn is_very_small_triangle(&self, op: usize) -> R<bool> {
        let o = *self.op(op)?;
        let next = *self.op(o.next)?;
        let prev = *self.op(o.prev)?;
        Ok(next.next == o.prev
            && (pts_really_close(prev.pt, next.pt) || pts_really_close(o.pt, next.pt) || pts_really_close(o.pt, prev.pt)))
    }

    fn is_valid_closed_path(&self, op: usize) -> R<bool> {
        if op == NIL {
            return Ok(false);
        }
        let o = *self.op(op)?;
        Ok(o.next != op && o.next != o.prev && !self.is_very_small_triangle(op)?)
    }

    fn swap_front_back_sides(&mut self, outrec: usize) -> R<()> {
        let pts = self.or(outrec)?.pts;
        let next = self.op(pts)?.next;
        let o = self.or_mut(outrec)?;
        core::mem::swap(&mut o.front_edge, &mut o.back_edge);
        o.pts = next;
        Ok(())
    }

    fn set_owner(&mut self, outrec: usize, new_owner: usize) -> R<()> {
        let no = self.or(new_owner)?.owner;
        let real = self.get_real_outrec(no)?;
        self.or_mut(new_owner)?.owner = real;
        let mut tmp = new_owner;
        let mut g = self.guard_or();
        while tmp != NIL && tmp != outrec {
            g.tick()?;
            tmp = self.or(tmp)?.owner;
        }
        if tmp != NIL {
            let oo = self.or(outrec)?.owner;
            self.or_mut(new_owner)?.owner = oo;
        }
        self.or_mut(outrec)?.owner = new_owner;
        Ok(())
    }

    // ------------------------------------------------------------------ adding paths

    fn add_loc_min(&mut self, vert: usize, polytype: PathType, is_open: bool) -> R<()> {
        if self.v(vert)?.flags & VF_LOCAL_MIN != 0 {
            return Ok(());
        }
        self.v_mut(vert)?.flags |= VF_LOCAL_MIN;
        self.minima_list.push(self.locmins.len());
        self.locmins.push(LocalMinima { vertex: vert, polytype, is_open });
        Ok(())
    }

    /// `AddPaths` / `AddPaths_` (closed paths).
    pub(crate) fn add_paths(&mut self, paths: &[Path64], polytype: PathType) -> R<()> {
        let is_open = false;
        self.minima_sorted = false;
        let total: usize = paths.iter().map(Vec::len).sum();
        if total == 0 {
            return Ok(());
        }
        let base = self.verts.len();
        self.verts.resize(base + total, Vertex { pt: P64::default(), next: NIL, prev: NIL, flags: 0 });
        let mut v = base;
        for path in paths {
            let v0 = v;
            let mut curr_v = v;
            let mut prev_v = NIL;
            if path.is_empty() {
                continue;
            }
            self.v_mut(v)?.prev = NIL;
            let mut cnt = 0usize;
            for &pt in path {
                if prev_v != NIL {
                    if self.v(prev_v)?.pt == pt {
                        continue;
                    }
                    self.v_mut(prev_v)?.next = curr_v;
                }
                let cv = self.v_mut(curr_v)?;
                cv.prev = prev_v;
                cv.pt = pt;
                cv.flags = 0;
                prev_v = curr_v;
                curr_v += 1;
                cnt += 1;
            }
            if prev_v == NIL || self.v(prev_v)?.prev == NIL {
                continue;
            }
            if !is_open && self.v(prev_v)?.pt == self.v(v0)?.pt {
                prev_v = self.v(prev_v)?.prev;
            }
            self.v_mut(prev_v)?.next = v0;
            self.v_mut(v0)?.prev = prev_v;
            v = curr_v;
            if cnt < 2 || (cnt == 2 && !is_open) {
                continue;
            }
            // closed path
            let mut pv = self.v(v0)?.prev;
            let mut g = self.guard_v();
            while pv != v0 && self.v(pv)?.pt.y == self.v(v0)?.pt.y {
                g.tick()?;
                pv = self.v(pv)?.prev;
            }
            if pv == v0 {
                continue;
            }
            let mut going_up = self.v(pv)?.pt.y > self.v(v0)?.pt.y;
            let going_up0 = going_up;
            let mut prev = v0;
            let mut curr = self.v(v0)?.next;
            let mut g = self.guard_v();
            while curr != v0 {
                g.tick()?;
                let cy = self.v(curr)?.pt.y;
                let py = self.v(prev)?.pt.y;
                if cy > py && going_up {
                    self.v_mut(prev)?.flags |= VF_LOCAL_MAX;
                    going_up = false;
                } else if cy < py && !going_up {
                    going_up = true;
                    self.add_loc_min(prev, polytype, is_open)?;
                }
                prev = curr;
                curr = self.v(curr)?.next;
            }
            if going_up != going_up0 {
                if going_up0 {
                    self.add_loc_min(prev, polytype, false)?;
                } else {
                    self.v_mut(prev)?.flags |= VF_LOCAL_MAX;
                }
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------------ execution

    fn reset(&mut self) -> R<()> {
        if !self.minima_sorted {
            // stable_sort with LocMinSorter: higher y first, then lower x
            let mut keys = Vec::with_capacity(self.minima_list.len());
            for &i in &self.minima_list {
                keys.push((self.v(self.lm(i)?.vertex)?.pt, i));
            }
            keys.sort_by(|a, b| b.0.y.cmp(&a.0.y).then(a.0.x.cmp(&b.0.x)));
            self.minima_list = keys.into_iter().map(|k| k.1).collect();
            self.minima_sorted = true;
        }
        for k in (0..self.minima_list.len()).rev() {
            let i = *self.minima_list.get(k).ok_or(Fault::Index)?;
            let y = self.v(self.lm(i)?.vertex)?.pt.y;
            self.scanlines.push(y);
        }
        self.cur_locmin = 0;
        self.actives = NIL;
        self.sel = NIL;
        self.succeeded = true;
        Ok(())
    }

    fn pop_scanline(&mut self) -> Option<i64> {
        let y = self.scanlines.pop()?;
        while self.scanlines.peek() == Some(&y) {
            self.scanlines.pop();
        }
        Some(y)
    }

    fn pop_local_minima(&mut self, y: i64) -> R<Option<usize>> {
        let Some(&lm) = self.minima_list.get(self.cur_locmin) else { return Ok(None) };
        if self.v(self.lm(lm)?.vertex)?.pt.y != y {
            return Ok(None);
        }
        self.cur_locmin += 1;
        Ok(Some(lm))
    }

    fn is_contributing_closed(&self, e: usize) -> R<bool> {
        let a = *self.a(e)?;
        match self.fillrule {
            FillRule::EvenOdd => {}
            FillRule::NonZero => {
                if a.wind_cnt.unsigned_abs() != 1 {
                    return Ok(false);
                }
            }
            FillRule::Positive => {
                if a.wind_cnt != 1 {
                    return Ok(false);
                }
            }
            FillRule::Negative => {
                if a.wind_cnt != -1 {
                    return Ok(false);
                }
            }
        }
        Ok(match self.cliptype {
            ClipType::NoClip => false,
            ClipType::Intersection => match self.fillrule {
                FillRule::Positive => a.wind_cnt2 > 0,
                FillRule::Negative => a.wind_cnt2 < 0,
                _ => a.wind_cnt2 != 0,
            },
            ClipType::Union => match self.fillrule {
                FillRule::Positive => a.wind_cnt2 <= 0,
                FillRule::Negative => a.wind_cnt2 >= 0,
                _ => a.wind_cnt2 == 0,
            },
            ClipType::Difference => {
                let result = match self.fillrule {
                    FillRule::Positive => a.wind_cnt2 <= 0,
                    FillRule::Negative => a.wind_cnt2 >= 0,
                    _ => a.wind_cnt2 == 0,
                };
                if self.polytype(e)? == PathType::Subject { result } else { !result }
            }
            ClipType::Xor => true,
        })
    }

    fn set_wind_count_for_closed_path_edge(&mut self, e: usize) -> R<()> {
        let pt = self.polytype(e)?;
        let mut e2 = self.a(e)?.prev_in_ael;
        let mut g = self.guard_ael();
        while e2 != NIL && (self.polytype(e2)? != pt || self.is_open(e2)?) {
            g.tick()?;
            e2 = self.a(e2)?.prev_in_ael;
        }
        let e_wind_dx = self.a(e)?.wind_dx;
        if e2 == NIL {
            self.a_mut(e)?.wind_cnt = e_wind_dx;
            e2 = self.actives;
        } else if self.fillrule == FillRule::EvenOdd {
            let w2 = self.a(e2)?.wind_cnt2;
            let ae = self.a_mut(e)?;
            ae.wind_cnt = e_wind_dx;
            ae.wind_cnt2 = w2;
            e2 = self.a(e2)?.next_in_ael;
        } else {
            let b = *self.a(e2)?;
            let is_open_e = self.is_open(e)?;
            let wc = if b.wind_cnt * b.wind_dx < 0 {
                if b.wind_cnt.unsigned_abs() > 1 {
                    if b.wind_dx * e_wind_dx < 0 { b.wind_cnt } else { b.wind_cnt + e_wind_dx }
                } else if is_open_e {
                    1
                } else {
                    e_wind_dx
                }
            } else if b.wind_dx * e_wind_dx < 0 {
                b.wind_cnt
            } else {
                b.wind_cnt + e_wind_dx
            };
            let ae = self.a_mut(e)?;
            ae.wind_cnt = wc;
            ae.wind_cnt2 = b.wind_cnt2;
            e2 = b.next_in_ael;
        }
        let mut g = self.guard_ael();
        if self.fillrule == FillRule::EvenOdd {
            while e2 != e {
                g.tick()?;
                if self.polytype(e2)? != pt && !self.is_open(e2)? {
                    let ae = self.a_mut(e)?;
                    ae.wind_cnt2 = if ae.wind_cnt2 == 0 { 1 } else { 0 };
                }
                e2 = self.a(e2)?.next_in_ael;
            }
        } else {
            while e2 != e {
                g.tick()?;
                if self.polytype(e2)? != pt && !self.is_open(e2)? {
                    let d = self.a(e2)?.wind_dx;
                    let ae = self.a_mut(e)?;
                    ae.wind_cnt2 = ae.wind_cnt2.wrapping_add(d);
                }
                e2 = self.a(e2)?.next_in_ael;
            }
        }
        Ok(())
    }

    fn is_valid_ael_order(&self, resident: usize, newcomer: usize) -> R<bool> {
        let r = *self.a(resident)?;
        let n = *self.a(newcomer)?;
        if n.curr_x != r.curr_x {
            return Ok(n.curr_x > r.curr_x);
        }
        let i = cross_product_sign(r.top, n.bot, n.top);
        if i != 0 {
            return Ok(i < 0);
        }
        if !self.is_maxima(resident)? && r.top.y > n.top.y {
            let nv = self.v(self.next_vertex(resident)?)?.pt;
            return Ok(cross_product_sign(n.bot, r.top, nv) <= 0);
        } else if !self.is_maxima(newcomer)? && n.top.y > r.top.y {
            let nv = self.v(self.next_vertex(newcomer)?)?.pt;
            return Ok(cross_product_sign(n.bot, n.top, nv) >= 0);
        }
        let y = n.bot.y;
        let newcomer_is_left = n.is_left_bound;
        let r_lm_y = self.v(self.lm(r.local_min)?.vertex)?.pt.y;
        if r.bot.y != y || r_lm_y != y {
            return Ok(n.is_left_bound);
        } else if r.is_left_bound != newcomer_is_left {
            return Ok(newcomer_is_left);
        }
        let rpp = self.v(self.prev_prev_vertex(resident)?)?.pt;
        if is_collinear(rpp, r.bot, r.top) {
            return Ok(true);
        }
        let npp = self.v(self.prev_prev_vertex(newcomer)?)?.pt;
        Ok((cross_product_sign(rpp, n.bot, npp) > 0) == newcomer_is_left)
    }

    fn insert_left_edge(&mut self, e: usize) -> R<()> {
        if self.actives == NIL {
            let ae = self.a_mut(e)?;
            ae.prev_in_ael = NIL;
            ae.next_in_ael = NIL;
            self.actives = e;
        } else if !self.is_valid_ael_order(self.actives, e)? {
            let act = self.actives;
            let ae = self.a_mut(e)?;
            ae.prev_in_ael = NIL;
            ae.next_in_ael = act;
            self.a_mut(act)?.prev_in_ael = e;
            self.actives = e;
        } else {
            let mut e2 = self.actives;
            let mut g = self.guard_ael();
            loop {
                g.tick()?;
                let nx = self.a(e2)?.next_in_ael;
                if nx != NIL && self.is_valid_ael_order(nx, e)? {
                    e2 = nx;
                } else {
                    break;
                }
            }
            if self.a(e2)?.join_with == JoinWith::Right {
                e2 = self.a(e2)?.next_in_ael;
            }
            if e2 == NIL {
                return Ok(());
            }
            let nx = self.a(e2)?.next_in_ael;
            self.a_mut(e)?.next_in_ael = nx;
            if nx != NIL {
                self.a_mut(nx)?.prev_in_ael = e;
            }
            self.a_mut(e)?.prev_in_ael = e2;
            self.a_mut(e2)?.next_in_ael = e;
        }
        Ok(())
    }

    fn insert_right_edge(&mut self, e: usize, e2: usize) -> R<()> {
        let nx = self.a(e)?.next_in_ael;
        self.a_mut(e2)?.next_in_ael = nx;
        if nx != NIL {
            self.a_mut(nx)?.prev_in_ael = e2;
        }
        self.a_mut(e2)?.prev_in_ael = e;
        self.a_mut(e)?.next_in_ael = e2;
        Ok(())
    }

    fn new_active(&mut self, lm: usize, vertex_top: usize, wind_dx: i32) -> R<usize> {
        let lmv = self.lm(lm)?.vertex;
        let bot = self.v(lmv)?.pt;
        let top = self.v(vertex_top)?.pt;
        let i = self.actv.len();
        self.actv.push(Active { bot, top, curr_x: bot.x, wind_dx, vertex_top, local_min: lm, ..Active::default() });
        self.set_dx(i)?;
        Ok(i)
    }

    fn insert_local_minima_into_ael(&mut self, bot_y: i64) -> R<()> {
        let mut g = Guard::new(self.minima_list.len());
        while let Some(lm) = self.pop_local_minima(bot_y)? {
            g.tick()?;
            let lmv = self.lm(lm)?.vertex;
            let vflags = self.v(lmv)?.flags;
            let mut left_bound = if vflags & VF_OPEN_START != 0 {
                NIL
            } else {
                let vt = self.v(lmv)?.prev;
                self.new_active(lm, vt, -1)?
            };
            let mut right_bound = if vflags & VF_OPEN_END != 0 {
                NIL
            } else {
                let vt = self.v(lmv)?.next;
                self.new_active(lm, vt, 1)?
            };
            if left_bound != NIL && right_bound != NIL {
                if self.is_horizontal(left_bound)? {
                    if self.a(left_bound)?.dx == -f64::MAX {
                        core::mem::swap(&mut left_bound, &mut right_bound);
                    }
                } else if self.is_horizontal(right_bound)? {
                    if self.a(right_bound)?.dx == f64::MAX {
                        core::mem::swap(&mut left_bound, &mut right_bound);
                    }
                } else if self.a(left_bound)?.dx < self.a(right_bound)?.dx {
                    core::mem::swap(&mut left_bound, &mut right_bound);
                }
            } else if left_bound == NIL {
                left_bound = right_bound;
                right_bound = NIL;
            }
            self.a_mut(left_bound)?.is_left_bound = true;
            self.insert_left_edge(left_bound)?;
            // closed paths only
            self.set_wind_count_for_closed_path_edge(left_bound)?;
            let contributing = self.is_contributing_closed(left_bound)?;
            if right_bound != NIL {
                let lb = *self.a(left_bound)?;
                {
                    let rb = self.a_mut(right_bound)?;
                    rb.is_left_bound = false;
                    rb.wind_cnt = lb.wind_cnt;
                    rb.wind_cnt2 = lb.wind_cnt2;
                }
                self.insert_right_edge(left_bound, right_bound)?;
                if contributing {
                    self.add_local_min_poly(left_bound, right_bound, lb.bot, true)?;
                    if !self.is_horizontal(left_bound)? {
                        self.check_join_left(left_bound, lb.bot, false)?;
                    }
                }
                let mut g2 = self.guard_ael();
                loop {
                    g2.tick()?;
                    let nx = self.a(right_bound)?.next_in_ael;
                    if nx != NIL && self.is_valid_ael_order(nx, right_bound)? {
                        let bot = self.a(right_bound)?.bot;
                        self.intersect_edges(right_bound, nx, bot)?;
                        self.swap_positions_in_ael(right_bound, nx)?;
                    } else {
                        break;
                    }
                }
                if self.is_horizontal(right_bound)? {
                    self.push_horz(right_bound)?;
                } else {
                    let bot = self.a(right_bound)?.bot;
                    self.check_join_right(right_bound, bot, false)?;
                    let ty = self.a(right_bound)?.top.y;
                    self.scanlines.push(ty);
                }
            } else if contributing {
                // StartOpenPath: open paths only (never here)
                return Err(Fault::Index);
            }
            if self.is_horizontal(left_bound)? {
                self.push_horz(left_bound)?;
            } else {
                let ty = self.a(left_bound)?.top.y;
                self.scanlines.push(ty);
            }
        }
        Ok(())
    }

    fn push_horz(&mut self, e: usize) -> R<()> {
        let s = self.sel;
        self.a_mut(e)?.next_in_sel = s;
        self.sel = e;
        Ok(())
    }

    fn pop_horz(&mut self) -> R<Option<usize>> {
        let e = self.sel;
        if e == NIL {
            return Ok(None);
        }
        self.sel = self.a(e)?.next_in_sel;
        Ok(Some(e))
    }

    fn add_local_min_poly(&mut self, e1: usize, e2: usize, pt: P64, is_new: bool) -> R<usize> {
        let outrec = self.new_outrec();
        self.a_mut(e1)?.outrec = outrec;
        self.a_mut(e2)?.outrec = outrec;
        if self.is_open(e1)? {
            self.or_mut(outrec)?.owner = NIL;
            self.or_mut(outrec)?.is_open = true;
            if self.a(e1)?.wind_dx > 0 {
                self.set_sides(outrec, e1, e2)?;
            } else {
                self.set_sides(outrec, e2, e1)?;
            }
        } else {
            let prev_hot = self.get_prev_hot_edge(e1)?;
            if prev_hot != NIL {
                // using_polytree_ is false: no SetOwner here
                let por = self.a(prev_hot)?.outrec;
                let ascending = prev_hot == self.or(por)?.front_edge;
                if ascending == is_new {
                    self.set_sides(outrec, e2, e1)?;
                } else {
                    self.set_sides(outrec, e1, e2)?;
                }
            } else {
                self.or_mut(outrec)?.owner = NIL;
                if is_new {
                    self.set_sides(outrec, e1, e2)?;
                } else {
                    self.set_sides(outrec, e2, e1)?;
                }
            }
        }
        let op = self.new_op(pt, outrec);
        self.or_mut(outrec)?.pts = op;
        Ok(op)
    }

    fn add_local_max_poly(&mut self, e1: usize, e2: usize, pt: P64) -> R<usize> {
        if self.is_joined(e1)? {
            self.split(e1, pt)?;
        }
        if self.is_joined(e2)? {
            self.split(e2, pt)?;
        }
        if self.is_front(e1)? == self.is_front(e2)? {
            if self.is_open_end(e1)? {
                let o = self.a(e1)?.outrec;
                self.swap_front_back_sides(o)?;
            } else if self.is_open_end(e2)? {
                let o = self.a(e2)?.outrec;
                self.swap_front_back_sides(o)?;
            } else {
                self.succeeded = false;
                return Ok(NIL);
            }
        }
        let mut result = self.add_out_pt(e1, pt)?;
        let or1 = self.a(e1)?.outrec;
        let or2 = self.a(e2)?.outrec;
        if or1 == or2 {
            self.or_mut(or1)?.pts = result;
            self.uncouple_outrec(e1)?;
            result = self.or(or1)?.pts;
            let owner = self.or(or1)?.owner;
            if owner != NIL && self.or(owner)?.front_edge == NIL {
                let real = self.get_real_outrec(owner)?;
                self.or_mut(or1)?.owner = real;
            }
        } else if self.is_open(e1)? {
            if self.a(e1)?.wind_dx < 0 {
                self.join_outrec_paths(e1, e2)?;
            } else {
                self.join_outrec_paths(e2, e1)?;
            }
        } else if self.or(or1)?.idx < self.or(or2)?.idx {
            self.join_outrec_paths(e1, e2)?;
        } else {
            self.join_outrec_paths(e2, e1)?;
        }
        Ok(result)
    }

    fn join_outrec_paths(&mut self, e1: usize, e2: usize) -> R<()> {
        let or1 = self.a(e1)?.outrec;
        let or2 = self.a(e2)?.outrec;
        let p1_st = self.or(or1)?.pts;
        let p2_st = self.or(or2)?.pts;
        let p1_end = self.op(p1_st)?.next;
        let p2_end = self.op(p2_st)?.next;
        if self.is_front(e1)? {
            self.op_mut(p2_end)?.prev = p1_st;
            self.op_mut(p1_st)?.next = p2_end;
            self.op_mut(p2_st)?.next = p1_end;
            self.op_mut(p1_end)?.prev = p2_st;
            self.or_mut(or1)?.pts = p2_st;
            let fe = self.or(or2)?.front_edge;
            self.or_mut(or1)?.front_edge = fe;
            if fe != NIL {
                self.a_mut(fe)?.outrec = or1;
            }
        } else {
            self.op_mut(p1_end)?.prev = p2_st;
            self.op_mut(p2_st)?.next = p1_end;
            self.op_mut(p1_st)?.next = p2_end;
            self.op_mut(p2_end)?.prev = p1_st;
            let be = self.or(or2)?.back_edge;
            self.or_mut(or1)?.back_edge = be;
            if be != NIL {
                self.a_mut(be)?.outrec = or1;
            }
        }
        {
            let o2 = self.or_mut(or2)?;
            o2.front_edge = NIL;
            o2.back_edge = NIL;
            o2.pts = NIL;
        }
        if self.is_open_end(e1)? {
            let p = self.or(or1)?.pts;
            self.or_mut(or2)?.pts = p;
            self.or_mut(or1)?.pts = NIL;
        } else {
            self.set_owner(or2, or1)?;
        }
        self.a_mut(e1)?.outrec = NIL;
        self.a_mut(e2)?.outrec = NIL;
        Ok(())
    }

    fn add_out_pt(&mut self, e: usize, pt: P64) -> R<usize> {
        let outrec = self.a(e)?.outrec;
        let to_front = self.is_front(e)?;
        let op_front = self.or(outrec)?.pts;
        let op_back = self.op(op_front)?.next;
        if to_front {
            if pt == self.op(op_front)?.pt {
                return Ok(op_front);
            }
        } else if pt == self.op(op_back)?.pt {
            return Ok(op_back);
        }
        let new_op = self.new_op(pt, outrec);
        self.op_mut(op_back)?.prev = new_op;
        {
            let n = self.op_mut(new_op)?;
            n.prev = op_front;
            n.next = op_back;
        }
        self.op_mut(op_front)?.next = new_op;
        if to_front {
            self.or_mut(outrec)?.pts = new_op;
        }
        Ok(new_op)
    }

    fn clean_collinear(&mut self, outrec: usize) -> R<()> {
        let outrec = self.get_real_outrec(outrec)?;
        if outrec == NIL || self.or(outrec)?.is_open {
            return Ok(());
        }
        if !self.is_valid_closed_path(self.or(outrec)?.pts)? {
            return self.dispose_out_pts(outrec);
        }
        let mut start_op = self.or(outrec)?.pts;
        let mut op2 = start_op;
        let mut g = Guard::new(self.ops.len().saturating_mul(self.ops.len()));
        loop {
            g.tick()?;
            let o = *self.op(op2)?;
            let prev = self.op(o.prev)?.pt;
            let next = self.op(o.next)?.pt;
            if is_collinear(prev, o.pt, next)
                && (o.pt == prev || o.pt == next || !self.preserve_collinear || dot_product(prev, o.pt, next) < 0.0)
            {
                if op2 == self.or(outrec)?.pts {
                    self.or_mut(outrec)?.pts = o.prev;
                }
                op2 = self.dispose_op(op2)?;
                if !self.is_valid_closed_path(op2)? {
                    return self.dispose_out_pts(outrec);
                }
                start_op = op2;
                continue;
            }
            op2 = o.next;
            if op2 == start_op {
                break;
            }
        }
        self.fix_self_intersects(outrec)
    }

    fn do_split_op(&mut self, outrec: usize, split_op: usize) -> R<()> {
        let s = *self.op(split_op)?;
        let prev_op = s.prev;
        let snext = s.next;
        let next_next_op = self.op(snext)?.next;
        self.or_mut(outrec)?.pts = prev_op;
        let prev_pt = self.op(prev_op)?.pt;
        let snext_pt = self.op(snext)?.pt;
        let nn_pt = self.op(next_next_op)?.pt;
        let ip = get_line_intersect_pt(prev_pt, s.pt, snext_pt, nn_pt).unwrap_or_default();
        let area1 = self.area_op(prev_op)?;
        let abs_area1 = area1.abs();
        if abs_area1 < 2.0 {
            return self.dispose_out_pts(outrec);
        }
        let area2 = area_triangle(ip, s.pt, snext_pt);
        let abs_area2 = area2.abs();
        if ip == prev_pt || ip == nn_pt {
            self.op_mut(next_next_op)?.prev = prev_op;
            self.op_mut(prev_op)?.next = next_next_op;
        } else {
            let por = self.op(prev_op)?.outrec;
            let new_op2 = self.new_op(ip, por);
            {
                let n = self.op_mut(new_op2)?;
                n.prev = prev_op;
                n.next = next_next_op;
            }
            self.op_mut(next_next_op)?.prev = new_op2;
            self.op_mut(prev_op)?.next = new_op2;
        }
        if abs_area2 >= 1.0 && (abs_area2 > abs_area1 || (area2 > 0.0) == (area1 > 0.0)) {
            let new_or = self.new_outrec();
            let owner = self.or(outrec)?.owner;
            self.or_mut(new_or)?.owner = owner;
            self.op_mut(split_op)?.outrec = new_or;
            self.op_mut(snext)?.outrec = new_or;
            let new_op = self.new_op(ip, new_or);
            {
                let n = self.op_mut(new_op)?;
                n.prev = snext;
                n.next = split_op;
            }
            self.or_mut(new_or)?.pts = new_op;
            self.op_mut(split_op)?.prev = new_op;
            self.op_mut(snext)?.next = new_op;
        }
        // else: the two points are deleted in the C++ (unreachable from the result)
        Ok(())
    }

    fn fix_self_intersects(&mut self, outrec: usize) -> R<()> {
        let mut op2 = self.or(outrec)?.pts;
        {
            let o = *self.op(op2)?;
            if o.prev == self.op(o.next)?.next {
                return Ok(());
            }
        }
        let mut g = Guard::new(self.ops.len().saturating_mul(self.ops.len()));
        loop {
            g.tick()?;
            let o = *self.op(op2)?;
            let next = *self.op(o.next)?;
            let prev_pt = self.op(o.prev)?.pt;
            let nn_pt = self.op(next.next)?.pt;
            if segments_intersect(prev_pt, o.pt, next.pt, nn_pt) {
                let pts = self.or(outrec)?.pts;
                if op2 == pts || o.next == pts {
                    let pp = self.op(pts)?.prev;
                    self.or_mut(outrec)?.pts = pp;
                }
                self.do_split_op(outrec, op2)?;
                let pts = self.or(outrec)?.pts;
                if pts == NIL {
                    break;
                }
                op2 = pts;
                let o = *self.op(op2)?;
                if o.prev == self.op(o.next)?.next {
                    break;
                }
                continue;
            }
            op2 = o.next;
            if op2 == self.or(outrec)?.pts {
                break;
            }
        }
        Ok(())
    }

    fn trim_horz(&mut self, horz: usize, preserve_collinear: bool) -> R<()> {
        let mut was_trimmed = false;
        let mut pt = self.v(self.next_vertex(horz)?)?.pt;
        let mut g = self.guard_v();
        while pt.y == self.a(horz)?.top.y {
            g.tick()?;
            let h = *self.a(horz)?;
            if preserve_collinear && ((pt.x < h.top.x) != (h.bot.x < h.top.x)) {
                break;
            }
            let nv = self.next_vertex(horz)?;
            {
                let hm = self.a_mut(horz)?;
                hm.vertex_top = nv;
                hm.top = pt;
            }
            was_trimmed = true;
            if self.is_maxima(horz)? {
                break;
            }
            pt = self.v(self.next_vertex(horz)?)?.pt;
        }
        if was_trimmed {
            self.set_dx(horz)?;
        }
        Ok(())
    }

    fn update_edge_into_ael(&mut self, e: usize) -> R<()> {
        let nv = self.next_vertex(e)?;
        let top = self.v(nv)?.pt;
        {
            let a = self.a_mut(e)?;
            a.bot = a.top;
            a.vertex_top = nv;
            a.top = top;
            a.curr_x = a.bot.x;
        }
        self.set_dx(e)?;
        if self.is_joined(e)? {
            let bot = self.a(e)?.bot;
            self.split(e, bot)?;
        }
        if self.is_horizontal(e)? {
            if !self.is_open(e)? {
                self.trim_horz(e, self.preserve_collinear)?;
            }
            return Ok(());
        }
        self.scanlines.push(top.y);
        let bot = self.a(e)?.bot;
        self.check_join_left(e, bot, false)?;
        self.check_join_right(e, bot, true)
    }

    fn intersect_edges(&mut self, e1: usize, e2: usize, pt: P64) -> R<()> {
        // closed paths only (has_open_paths_ is false)
        if self.is_joined(e1)? {
            self.split(e1, pt)?;
        }
        if self.is_joined(e2)? {
            self.split(e2, pt)?;
        }
        let a1 = *self.a(e1)?;
        let a2 = *self.a(e2)?;
        let same = self.polytype(e1)? == self.polytype(e2)?;
        let (mut w1, mut w2, mut c1, mut c2) = (a1.wind_cnt, a2.wind_cnt, a1.wind_cnt2, a2.wind_cnt2);
        if same {
            if self.fillrule == FillRule::EvenOdd {
                core::mem::swap(&mut w1, &mut w2);
            } else {
                if w1.wrapping_add(a2.wind_dx) == 0 {
                    w1 = w1.wrapping_neg();
                } else {
                    w1 = w1.wrapping_add(a2.wind_dx);
                }
                if w2.wrapping_sub(a1.wind_dx) == 0 {
                    w2 = w2.wrapping_neg();
                } else {
                    w2 = w2.wrapping_sub(a1.wind_dx);
                }
            }
        } else if self.fillrule != FillRule::EvenOdd {
            c1 = c1.wrapping_add(a2.wind_dx);
            c2 = c2.wrapping_sub(a1.wind_dx);
        } else {
            c1 = if c1 == 0 { 1 } else { 0 };
            c2 = if c2 == 0 { 1 } else { 0 };
        }
        {
            let x = self.a_mut(e1)?;
            x.wind_cnt = w1;
            x.wind_cnt2 = c1;
        }
        {
            let x = self.a_mut(e2)?;
            x.wind_cnt = w2;
            x.wind_cnt2 = c2;
        }
        let (old1, old2): (i64, i64) = match self.fillrule {
            FillRule::EvenOdd | FillRule::NonZero => (i64::from(w1).abs(), i64::from(w2).abs()),
            FillRule::Positive => (i64::from(w1), i64::from(w2)),
            FillRule::Negative => (-i64::from(w1), -i64::from(w2)),
        };
        let e1_in01 = old1 == 0 || old1 == 1;
        let e2_in01 = old2 == 0 || old2 == 1;
        let hot1 = self.is_hot(e1)?;
        let hot2 = self.is_hot(e2)?;
        if (!hot1 && !e1_in01) || (!hot2 && !e2_in01) {
            return Ok(());
        }
        if hot1 && hot2 {
            if (old1 != 0 && old1 != 1) || (old2 != 0 && old2 != 1) || (!same && self.cliptype != ClipType::Xor) {
                self.add_local_max_poly(e1, e2, pt)?;
            } else if self.is_front(e1)? || self.a(e1)?.outrec == self.a(e2)?.outrec {
                self.add_local_max_poly(e1, e2, pt)?;
                self.add_local_min_poly(e1, e2, pt, false)?;
            } else {
                self.add_out_pt(e1, pt)?;
                self.add_out_pt(e2, pt)?;
                self.swap_outrecs(e1, e2)?;
            }
        } else if hot1 {
            self.add_out_pt(e1, pt)?;
            self.swap_outrecs(e1, e2)?;
        } else if hot2 {
            self.add_out_pt(e2, pt)?;
            self.swap_outrecs(e1, e2)?;
        } else {
            let (wc1, wc2): (i64, i64) = match self.fillrule {
                FillRule::EvenOdd | FillRule::NonZero => (i64::from(c1).abs(), i64::from(c2).abs()),
                FillRule::Positive => (i64::from(c1), i64::from(c2)),
                FillRule::Negative => (-i64::from(c1), -i64::from(c2)),
            };
            if !same {
                self.add_local_min_poly(e1, e2, pt, false)?;
            } else if old1 == 1 && old2 == 1 {
                match self.cliptype {
                    ClipType::Union => {
                        if wc1 <= 0 && wc2 <= 0 {
                            self.add_local_min_poly(e1, e2, pt, false)?;
                        }
                    }
                    ClipType::Difference => {
                        let pt1 = self.polytype(e1)?;
                        if (pt1 == PathType::Clip && wc1 > 0 && wc2 > 0) || (pt1 == PathType::Subject && wc1 <= 0 && wc2 <= 0) {
                            self.add_local_min_poly(e1, e2, pt, false)?;
                        }
                    }
                    ClipType::Xor => {
                        self.add_local_min_poly(e1, e2, pt, false)?;
                    }
                    _ => {
                        if wc1 > 0 && wc2 > 0 {
                            self.add_local_min_poly(e1, e2, pt, false)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn delete_from_ael(&mut self, e: usize) -> R<()> {
        let a = *self.a(e)?;
        if a.prev_in_ael == NIL && a.next_in_ael == NIL && e != self.actives {
            return Ok(());
        }
        if a.prev_in_ael != NIL {
            self.a_mut(a.prev_in_ael)?.next_in_ael = a.next_in_ael;
        } else {
            self.actives = a.next_in_ael;
        }
        if a.next_in_ael != NIL {
            self.a_mut(a.next_in_ael)?.prev_in_ael = a.prev_in_ael;
        }
        // the C++ deletes the edge: a dead edge never reaches the AEL again
        let d = self.a_mut(e)?;
        d.prev_in_ael = NIL;
        d.next_in_ael = NIL;
        Ok(())
    }

    fn adjust_curr_x_and_copy_to_sel(&mut self, top_y: i64) -> R<()> {
        let mut e = self.actives;
        self.sel = e;
        let mut g = self.guard_ael();
        while e != NIL {
            g.tick()?;
            let tx = self.top_x(e, top_y)?;
            let a = self.a_mut(e)?;
            a.prev_in_sel = a.prev_in_ael;
            a.next_in_sel = a.next_in_ael;
            a.jump = a.next_in_sel;
            a.curr_x = tx;
            e = a.next_in_ael;
        }
        Ok(())
    }

    fn execute_internal(&mut self, ct: ClipType, fr: FillRule) -> R<bool> {
        self.cliptype = ct;
        self.fillrule = fr;
        self.reset()?;
        let Some(mut y) = self.pop_scanline() else { return Ok(true) };
        if ct == ClipType::NoClip {
            return Ok(true);
        }
        // each scanline pops one y; every y was pushed by a vertex or an edge top
        let mut g = Guard::new(self.verts.len().saturating_mul(4).saturating_add(self.actv.len()));
        while self.succeeded {
            g.tick()?;
            self.insert_local_minima_into_ael(y)?;
            let mut gh = Guard::new(self.verts.len().saturating_mul(2));
            while let Some(e) = self.pop_horz()? {
                gh.tick()?;
                self.do_horizontal(e)?;
            }
            if !self.horz_segs.is_empty() {
                self.convert_horz_segs_to_joins()?;
                self.horz_segs.clear();
            }
            self.bot_y = y;
            let Some(ny) = self.pop_scanline() else { break };
            y = ny;
            self.do_intersections(y)?;
            self.do_top_of_scanbeam(y)?;
            let mut gh = Guard::new(self.verts.len().saturating_mul(2));
            while let Some(e) = self.pop_horz()? {
                gh.tick()?;
                self.do_horizontal(e)?;
            }
        }
        if self.succeeded {
            self.process_horz_joins()?;
        }
        Ok(self.succeeded)
    }

    fn fix_outrec_pts(&mut self, outrec: usize) -> R<()> {
        let start = self.or(outrec)?.pts;
        let mut op = start;
        let mut g = self.guard_ops();
        loop {
            g.tick()?;
            self.op_mut(op)?.outrec = outrec;
            op = self.op(op)?.next;
            if op == start {
                break;
            }
        }
        Ok(())
    }

    fn update_horz_segment(&mut self, hsi: usize) -> R<bool> {
        let hs = *self.horz_segs.get(hsi).ok_or(Fault::Index)?;
        let op = hs.left_op;
        let outrec = self.get_real_outrec(self.op(op)?.outrec)?;
        let outrec_has_edges = self.or(outrec)?.front_edge != NIL;
        let curr_y = self.op(op)?.pt.y;
        let mut op_p = op;
        let mut op_n = op;
        let mut g = self.guard_ops();
        if outrec_has_edges {
            let op_a = self.or(outrec)?.pts;
            let op_z = self.op(op_a)?.next;
            while op_p != op_z && self.op(self.op(op_p)?.prev)?.pt.y == curr_y {
                g.tick()?;
                op_p = self.op(op_p)?.prev;
            }
            while op_n != op_a && self.op(self.op(op_n)?.next)?.pt.y == curr_y {
                g.tick()?;
                op_n = self.op(op_n)?.next;
            }
        } else {
            while self.op(op_p)?.prev != op_n && self.op(self.op(op_p)?.prev)?.pt.y == curr_y {
                g.tick()?;
                op_p = self.op(op_p)?.prev;
            }
            while self.op(op_n)?.next != op_p && self.op(self.op(op_n)?.next)?.pt.y == curr_y {
                g.tick()?;
                op_n = self.op(op_n)?.next;
            }
        }
        // SetHorzSegHeadingForward
        let pp = self.op(op_p)?.pt;
        let pn = self.op(op_n)?.pt;
        let mut h = hs;
        let heading = if pp.x == pn.x {
            false
        } else {
            if pp.x < pn.x {
                h.left_op = op_p;
                h.right_op = op_n;
                h.left_to_right = true;
            } else {
                h.left_op = op_n;
                h.right_op = op_p;
                h.left_to_right = false;
            }
            true
        };
        let result = heading && !self.op(h.left_op)?.horz;
        if result {
            self.op_mut(h.left_op)?.horz = true;
        } else {
            h.right_op = NIL;
        }
        *self.horz_segs.get_mut(hsi).ok_or(Fault::Index)? = h;
        Ok(result)
    }

    fn convert_horz_segs_to_joins(&mut self) -> R<()> {
        let mut j = 0usize;
        for i in 0..self.horz_segs.len() {
            if self.update_horz_segment(i)? {
                j += 1;
            }
        }
        if j < 2 {
            return Ok(());
        }
        // stable_sort with HorzSegSorter
        let mut keyed = Vec::with_capacity(self.horz_segs.len());
        for hs in &self.horz_segs {
            let x = if hs.right_op != NIL { Some(self.op(hs.left_op)?.pt.x) } else { None };
            keyed.push((x, *hs));
        }
        keyed.sort_by(|a, b| match (a.0, b.0) {
            (Some(_), None) => core::cmp::Ordering::Less,
            (None, Some(_)) => core::cmp::Ordering::Greater,
            (None, None) => core::cmp::Ordering::Equal,
            (Some(x1), Some(x2)) => x1.cmp(&x2),
        });
        self.horz_segs = keyed.into_iter().map(|k| k.1).collect();
        let hs_end = j;
        let mut g = Guard::new(self.ops.len().saturating_mul(4).saturating_add(j.saturating_mul(j)));
        for i1 in 0..hs_end - 1 {
            for i2 in i1 + 1..hs_end {
                let h1 = *self.horz_segs.get(i1).ok_or(Fault::Index)?;
                let h2 = *self.horz_segs.get(i2).ok_or(Fault::Index)?;
                if self.op(h2.left_op)?.pt.x >= self.op(h1.right_op)?.pt.x
                    || h2.left_to_right == h1.left_to_right
                    || self.op(h2.right_op)?.pt.x <= self.op(h1.left_op)?.pt.x
                {
                    continue;
                }
                let curr_y = self.op(h1.left_op)?.pt.y;
                let (mut l1, mut l2) = (h1.left_op, h2.left_op);
                if h1.left_to_right {
                    loop {
                        g.tick()?;
                        let n = self.op(self.op(l1)?.next)?;
                        if n.pt.y == curr_y && n.pt.x <= self.op(l2)?.pt.x {
                            l1 = self.op(l1)?.next;
                        } else {
                            break;
                        }
                    }
                    loop {
                        g.tick()?;
                        let p = self.op(self.op(l2)?.prev)?;
                        if p.pt.y == curr_y && p.pt.x <= self.op(l1)?.pt.x {
                            l2 = self.op(l2)?.prev;
                        } else {
                            break;
                        }
                    }
                    self.horz_segs.get_mut(i1).ok_or(Fault::Index)?.left_op = l1;
                    self.horz_segs.get_mut(i2).ok_or(Fault::Index)?.left_op = l2;
                    let op1 = self.duplicate_op(l1, true)?;
                    let op2 = self.duplicate_op(l2, false)?;
                    self.horz_joins.push(HorzJoin { op1, op2 });
                } else {
                    loop {
                        g.tick()?;
                        let p = self.op(self.op(l1)?.prev)?;
                        if p.pt.y == curr_y && p.pt.x <= self.op(l2)?.pt.x {
                            l1 = self.op(l1)?.prev;
                        } else {
                            break;
                        }
                    }
                    loop {
                        g.tick()?;
                        let n = self.op(self.op(l2)?.next)?;
                        if n.pt.y == curr_y && n.pt.x <= self.op(l1)?.pt.x {
                            l2 = self.op(l2)?.next;
                        } else {
                            break;
                        }
                    }
                    self.horz_segs.get_mut(i1).ok_or(Fault::Index)?.left_op = l1;
                    self.horz_segs.get_mut(i2).ok_or(Fault::Index)?.left_op = l2;
                    let op1 = self.duplicate_op(l2, true)?;
                    let op2 = self.duplicate_op(l1, false)?;
                    self.horz_joins.push(HorzJoin { op1, op2 });
                }
            }
        }
        Ok(())
    }

    fn process_horz_joins(&mut self) -> R<()> {
        let joins = core::mem::take(&mut self.horz_joins);
        for j in &joins {
            let or1 = self.get_real_outrec(self.op(j.op1)?.outrec)?;
            let mut or2 = self.get_real_outrec(self.op(j.op2)?.outrec)?;
            let op1b = self.op(j.op1)?.next;
            let op2b = self.op(j.op2)?.prev;
            self.op_mut(j.op1)?.next = j.op2;
            self.op_mut(j.op2)?.prev = j.op1;
            self.op_mut(op1b)?.prev = op2b;
            self.op_mut(op2b)?.next = op1b;
            if or1 == or2 {
                or2 = self.new_outrec();
                self.or_mut(or2)?.pts = op1b;
                self.fix_outrec_pts(or2)?;
                let p1 = self.or(or1)?.pts;
                if self.op(p1)?.outrec == or2 {
                    self.or_mut(or1)?.pts = j.op1;
                    self.op_mut(j.op1)?.outrec = or1;
                }
                self.or_mut(or2)?.owner = or1;
            } else {
                self.or_mut(or2)?.pts = NIL;
                self.or_mut(or2)?.owner = or1;
            }
        }
        self.horz_joins = joins;
        Ok(())
    }

    fn do_intersections(&mut self, top_y: i64) -> R<()> {
        if self.build_intersect_list(top_y)? {
            self.process_intersect_list()?;
            self.intersect_nodes.clear();
        }
        Ok(())
    }

    fn add_new_intersect_node(&mut self, e1: usize, e2: usize, top_y: i64) -> R<()> {
        let a1 = *self.a(e1)?;
        let a2 = *self.a(e2)?;
        let mut ip = get_line_intersect_pt(a1.bot, a1.top, a2.bot, a2.top).unwrap_or(P64 { x: a1.curr_x, y: top_y });
        if ip.y > self.bot_y || ip.y < top_y {
            let abs_dx1 = a1.dx.abs();
            let abs_dx2 = a2.dx.abs();
            if abs_dx1 > 100.0 && abs_dx2 > 100.0 {
                if abs_dx1 > abs_dx2 {
                    ip = get_closest_point_on_segment(ip, a1.bot, a1.top);
                } else {
                    ip = get_closest_point_on_segment(ip, a2.bot, a2.top);
                }
            } else if abs_dx1 > 100.0 {
                ip = get_closest_point_on_segment(ip, a1.bot, a1.top);
            } else if abs_dx2 > 100.0 {
                ip = get_closest_point_on_segment(ip, a2.bot, a2.top);
            } else {
                if ip.y < top_y {
                    ip.y = top_y;
                } else {
                    ip.y = self.bot_y;
                }
                if abs_dx1 < abs_dx2 {
                    ip.x = self.top_x(e1, ip.y)?;
                } else {
                    ip.x = self.top_x(e2, ip.y)?;
                }
            }
        }
        self.intersect_nodes.push(IntersectNode { pt: ip, edge1: e1, edge2: e2 });
        Ok(())
    }

    fn extract_from_sel(&mut self, ae: usize) -> R<usize> {
        let a = *self.a(ae)?;
        let res = a.next_in_sel;
        if res != NIL {
            self.a_mut(res)?.prev_in_sel = a.prev_in_sel;
        }
        self.a_mut(a.prev_in_sel)?.next_in_sel = res;
        Ok(res)
    }

    fn insert1_before2_in_sel(&mut self, ae1: usize, ae2: usize) -> R<()> {
        let p = self.a(ae2)?.prev_in_sel;
        self.a_mut(ae1)?.prev_in_sel = p;
        if p != NIL {
            self.a_mut(p)?.next_in_sel = ae1;
        }
        self.a_mut(ae1)?.next_in_sel = ae2;
        self.a_mut(ae2)?.prev_in_sel = ae1;
        Ok(())
    }

    fn build_intersect_list(&mut self, top_y: i64) -> R<bool> {
        if self.actives == NIL || self.a(self.actives)?.next_in_ael == NIL {
            return Ok(false);
        }
        self.adjust_curr_x_and_copy_to_sel(top_y)?;
        let n = self.actv.len();
        let mut g = Guard::new(n.saturating_mul(n).saturating_mul(4).saturating_add(64));
        let mut left = self.sel;
        while left != NIL && self.a(left)?.jump != NIL {
            g.tick()?;
            let mut prev_base = NIL;
            while left != NIL && self.a(left)?.jump != NIL {
                g.tick()?;
                let mut curr_base = left;
                let mut right = self.a(left)?.jump;
                let mut l_end = right;
                let r_end = self.a(right)?.jump;
                self.a_mut(left)?.jump = r_end;
                while left != l_end && right != r_end {
                    g.tick()?;
                    if self.a(right)?.curr_x < self.a(left)?.curr_x {
                        let mut tmp = self.a(right)?.prev_in_sel;
                        loop {
                            g.tick()?;
                            self.add_new_intersect_node(tmp, right, top_y)?;
                            if tmp == left {
                                break;
                            }
                            tmp = self.a(tmp)?.prev_in_sel;
                        }
                        tmp = right;
                        right = self.extract_from_sel(tmp)?;
                        l_end = right;
                        self.insert1_before2_in_sel(tmp, left)?;
                        if left == curr_base {
                            curr_base = tmp;
                            self.a_mut(curr_base)?.jump = r_end;
                            if prev_base == NIL {
                                self.sel = curr_base;
                            } else {
                                self.a_mut(prev_base)?.jump = curr_base;
                            }
                        }
                    } else {
                        left = self.a(left)?.next_in_sel;
                    }
                }
                prev_base = curr_base;
                left = r_end;
            }
            left = self.sel;
        }
        Ok(!self.intersect_nodes.is_empty())
    }

    fn edges_adjacent_in_ael(&self, node: &IntersectNode) -> R<bool> {
        let a = self.a(node.edge1)?;
        Ok(a.next_in_ael == node.edge2 || a.prev_in_ael == node.edge2)
    }

    fn process_intersect_list(&mut self) -> R<()> {
        let mut nodes = core::mem::take(&mut self.intersect_nodes);
        libcxx_sort(&mut nodes, |a, b| if a.pt.y == b.pt.y { a.pt.x < b.pt.x } else { a.pt.y > b.pt.y })?;
        for i in 0..nodes.len() {
            if !self.edges_adjacent_in_ael(nodes.get(i).ok_or(Fault::Index)?)? {
                let mut i2 = i + 1;
                loop {
                    let n2 = nodes.get(i2).ok_or(Fault::Index)?;
                    if self.edges_adjacent_in_ael(n2)? {
                        break;
                    }
                    i2 += 1;
                }
                nodes.swap(i, i2);
            }
            let node = *nodes.get(i).ok_or(Fault::Index)?;
            self.intersect_edges(node.edge1, node.edge2, node.pt)?;
            self.swap_positions_in_ael(node.edge1, node.edge2)?;
            self.a_mut(node.edge1)?.curr_x = node.pt.x;
            self.a_mut(node.edge2)?.curr_x = node.pt.x;
            self.check_join_left(node.edge2, node.pt, true)?;
            self.check_join_right(node.edge1, node.pt, true)?;
        }
        self.intersect_nodes = nodes;
        Ok(())
    }

    fn swap_positions_in_ael(&mut self, e1: usize, e2: usize) -> R<()> {
        let next = self.a(e2)?.next_in_ael;
        if next != NIL {
            self.a_mut(next)?.prev_in_ael = e1;
        }
        let prev = self.a(e1)?.prev_in_ael;
        if prev != NIL {
            self.a_mut(prev)?.next_in_ael = e2;
        }
        {
            let b = self.a_mut(e2)?;
            b.prev_in_ael = prev;
            b.next_in_ael = e1;
        }
        {
            let a = self.a_mut(e1)?;
            a.prev_in_ael = e2;
            a.next_in_ael = next;
        }
        if prev == NIL {
            self.actives = e2;
        }
        Ok(())
    }

    fn get_last_op(&self, hot_edge: usize) -> R<usize> {
        let outrec = self.a(hot_edge)?.outrec;
        let o = self.or(outrec)?;
        let mut result = o.pts;
        if hot_edge != o.front_edge {
            result = self.op(result)?.next;
        }
        Ok(result)
    }

    fn add_trial_horz_join(&mut self, op: usize) -> R<()> {
        let or = self.op(op)?.outrec;
        if self.or(or)?.is_open {
            return Ok(());
        }
        self.horz_segs.push(HorzSegment { left_op: op, right_op: NIL, left_to_right: true });
        Ok(())
    }

    /// `ResetHorzDirection` → (is_left_to_right, horz_left, horz_right).
    fn reset_horz_direction(&self, horz: usize, max_vertex: usize) -> R<(bool, i64, i64)> {
        let h = *self.a(horz)?;
        if h.bot.x == h.top.x {
            let mut e = h.next_in_ael;
            let mut g = self.guard_ael();
            while e != NIL && self.a(e)?.vertex_top != max_vertex {
                g.tick()?;
                e = self.a(e)?.next_in_ael;
            }
            Ok((e != NIL, h.curr_x, h.curr_x))
        } else if h.curr_x < h.top.x {
            Ok((true, h.curr_x, h.top.x))
        } else {
            Ok((false, h.top.x, h.curr_x))
        }
    }

    fn do_horizontal(&mut self, horz: usize) -> R<()> {
        let horz_is_open = self.is_open(horz)?;
        if horz_is_open {
            return Err(Fault::Index);
        }
        let y = self.a(horz)?.bot.y;
        let vertex_max = self.get_curr_y_maxima_vertex(horz)?;
        let (mut is_left_to_right, mut horz_left, mut horz_right) = self.reset_horz_direction(horz, vertex_max)?;
        if self.is_hot(horz)? {
            let cx = self.a(horz)?.curr_x;
            let op = self.add_out_pt(horz, P64 { x: cx, y })?;
            self.add_trial_horz_join(op)?;
        }
        let mut g = Guard::new(self.verts.len().saturating_add(self.actv.len()).saturating_mul(4));
        loop {
            g.tick()?;
            let mut e = if is_left_to_right { self.a(horz)?.next_in_ael } else { self.a(horz)?.prev_in_ael };
            while e != NIL {
                g.tick()?;
                if self.a(e)?.vertex_top == vertex_max {
                    if self.is_hot(horz)? && self.is_joined(e)? {
                        let t = self.a(e)?.top;
                        self.split(e, t)?;
                    }
                    if self.is_hot(horz)? {
                        let mut g2 = self.guard_v();
                        while self.a(horz)?.vertex_top != vertex_max {
                            g2.tick()?;
                            let t = self.a(horz)?.top;
                            self.add_out_pt(horz, t)?;
                            self.update_edge_into_ael(horz)?;
                        }
                        let t = self.a(horz)?.top;
                        if is_left_to_right {
                            self.add_local_max_poly(horz, e, t)?;
                        } else {
                            self.add_local_max_poly(e, horz, t)?;
                        }
                    }
                    self.delete_from_ael(e)?;
                    self.delete_from_ael(horz)?;
                    return Ok(());
                }
                if vertex_max != self.a(horz)?.vertex_top || self.is_open_end(horz)? {
                    let ecx = self.a(e)?.curr_x;
                    if (is_left_to_right && ecx > horz_right) || (!is_left_to_right && ecx < horz_left) {
                        break;
                    }
                    if ecx == self.a(horz)?.top.x && !self.is_horizontal(e)? {
                        let pt = self.v(self.next_vertex(horz)?)?.pt;
                        let open_other = self.is_open(e)? && !self.is_same_polytype(e, horz)? && !self.is_hot(e)?;
                        let tx = self.top_x(e, pt.y)?;
                        if is_left_to_right {
                            if open_other {
                                if tx > pt.x {
                                    break;
                                }
                            } else if tx >= pt.x {
                                break;
                            }
                        } else if open_other {
                            if tx < pt.x {
                                break;
                            }
                        } else if tx <= pt.x {
                            break;
                        }
                    }
                }
                let pt = P64 { x: self.a(e)?.curr_x, y: self.a(horz)?.bot.y };
                if is_left_to_right {
                    self.intersect_edges(horz, e, pt)?;
                    self.swap_positions_in_ael(horz, e)?;
                    self.check_join_left(e, pt, false)?;
                    let ecx = self.a(e)?.curr_x;
                    self.a_mut(horz)?.curr_x = ecx;
                    e = self.a(horz)?.next_in_ael;
                } else {
                    self.intersect_edges(e, horz, pt)?;
                    self.swap_positions_in_ael(e, horz)?;
                    self.check_join_right(e, pt, false)?;
                    let ecx = self.a(e)?.curr_x;
                    self.a_mut(horz)?.curr_x = ecx;
                    e = self.a(horz)?.prev_in_ael;
                }
                if self.a(horz)?.outrec != NIL {
                    let lo = self.get_last_op(horz)?;
                    self.add_trial_horz_join(lo)?;
                }
            }
            // horzIsOpen is false here
            if self.v(self.next_vertex(horz)?)?.pt.y != self.a(horz)?.top.y {
                break;
            }
            if self.is_hot(horz)? {
                let t = self.a(horz)?.top;
                self.add_out_pt(horz, t)?;
            }
            self.update_edge_into_ael(horz)?;
            (is_left_to_right, horz_left, horz_right) = self.reset_horz_direction(horz, vertex_max)?;
        }
        if self.is_hot(horz)? {
            let t = self.a(horz)?.top;
            let op = self.add_out_pt(horz, t)?;
            self.add_trial_horz_join(op)?;
        }
        self.update_edge_into_ael(horz)
    }

    fn do_top_of_scanbeam(&mut self, y: i64) -> R<()> {
        self.sel = NIL;
        let mut e = self.actives;
        let mut g = Guard::new(self.actv.len().saturating_mul(2));
        while e != NIL {
            g.tick()?;
            if self.a(e)?.top.y == y {
                let tx = self.a(e)?.top.x;
                self.a_mut(e)?.curr_x = tx;
                if self.is_maxima(e)? {
                    e = self.do_maxima(e)?;
                    continue;
                }
                if self.is_hot(e)? {
                    let t = self.a(e)?.top;
                    self.add_out_pt(e, t)?;
                }
                self.update_edge_into_ael(e)?;
                if self.is_horizontal(e)? {
                    self.push_horz(e)?;
                }
            } else {
                let tx = self.top_x(e, y)?;
                self.a_mut(e)?.curr_x = tx;
            }
            e = self.a(e)?.next_in_ael;
        }
        Ok(())
    }

    fn do_maxima(&mut self, e: usize) -> R<usize> {
        let prev_e = self.a(e)?.prev_in_ael;
        let mut next_e = self.a(e)?.next_in_ael;
        if self.is_open_end(e)? {
            // open paths only
            return Err(Fault::Index);
        }
        let max_pair = self.get_maxima_pair(e)?;
        if max_pair == NIL {
            return Ok(next_e);
        }
        if self.is_joined(e)? {
            let t = self.a(e)?.top;
            self.split(e, t)?;
        }
        if self.is_joined(max_pair)? {
            let t = self.a(max_pair)?.top;
            self.split(max_pair, t)?;
        }
        let mut g = self.guard_ael();
        while next_e != max_pair {
            g.tick()?;
            let t = self.a(e)?.top;
            self.intersect_edges(e, next_e, t)?;
            self.swap_positions_in_ael(e, next_e)?;
            next_e = self.a(e)?.next_in_ael;
        }
        if self.is_hot(e)? {
            let t = self.a(e)?.top;
            self.add_local_max_poly(e, max_pair, t)?;
        }
        self.delete_from_ael(e)?;
        self.delete_from_ael(max_pair)?;
        Ok(if prev_e != NIL { self.a(prev_e)?.next_in_ael } else { self.actives })
    }

    fn split(&mut self, e: usize, pt: P64) -> R<()> {
        if self.a(e)?.join_with == JoinWith::Right {
            self.a_mut(e)?.join_with = JoinWith::NoJoin;
            let nx = self.a(e)?.next_in_ael;
            self.a_mut(nx)?.join_with = JoinWith::NoJoin;
            self.add_local_min_poly(e, nx, pt, true)?;
        } else {
            self.a_mut(e)?.join_with = JoinWith::NoJoin;
            let pv = self.a(e)?.prev_in_ael;
            self.a_mut(pv)?.join_with = JoinWith::NoJoin;
            self.add_local_min_poly(pv, e, pt, true)?;
        }
        Ok(())
    }

    fn check_join_left(&mut self, e: usize, pt: P64, check_curr_x: bool) -> R<()> {
        let prev = self.a(e)?.prev_in_ael;
        if prev == NIL
            || !self.is_hot(e)?
            || !self.is_hot(prev)?
            || self.is_horizontal(e)?
            || self.is_horizontal(prev)?
            || self.is_open(e)?
            || self.is_open(prev)?
        {
            return Ok(());
        }
        let a = *self.a(e)?;
        let p = *self.a(prev)?;
        if (pt.y < a.top.y + 2 || pt.y < p.top.y + 2) && (a.bot.y > pt.y || p.bot.y > pt.y) {
            return Ok(());
        }
        if check_curr_x {
            if perpendic_dist_from_line_sqrd(pt, p.bot, p.top) > 0.25 {
                return Ok(());
            }
        } else if a.curr_x != p.curr_x {
            return Ok(());
        }
        if !is_collinear(a.top, pt, p.top) {
            return Ok(());
        }
        let ei = self.or(a.outrec)?.idx;
        let pi = self.or(p.outrec)?.idx;
        if ei == pi {
            self.add_local_max_poly(prev, e, pt)?;
        } else if ei < pi {
            self.join_outrec_paths(e, prev)?;
        } else {
            self.join_outrec_paths(prev, e)?;
        }
        self.a_mut(prev)?.join_with = JoinWith::Right;
        self.a_mut(e)?.join_with = JoinWith::Left;
        Ok(())
    }

    fn check_join_right(&mut self, e: usize, pt: P64, check_curr_x: bool) -> R<()> {
        let next = self.a(e)?.next_in_ael;
        if next == NIL
            || !self.is_hot(e)?
            || !self.is_hot(next)?
            || self.is_horizontal(e)?
            || self.is_horizontal(next)?
            || self.is_open(e)?
            || self.is_open(next)?
        {
            return Ok(());
        }
        let a = *self.a(e)?;
        let n = *self.a(next)?;
        if (pt.y < a.top.y + 2 || pt.y < n.top.y + 2) && (a.bot.y > pt.y || n.bot.y > pt.y) {
            return Ok(());
        }
        if check_curr_x {
            if perpendic_dist_from_line_sqrd(pt, n.bot, n.top) > 0.35 {
                return Ok(());
            }
        } else if a.curr_x != n.curr_x {
            return Ok(());
        }
        if !is_collinear(a.top, pt, n.top) {
            return Ok(());
        }
        let ei = self.or(a.outrec)?.idx;
        let ni = self.or(n.outrec)?.idx;
        if ei == ni {
            self.add_local_max_poly(e, next, pt)?;
        } else if ei < ni {
            self.join_outrec_paths(e, next)?;
        } else {
            self.join_outrec_paths(next, e)?;
        }
        self.a_mut(e)?.join_with = JoinWith::Right;
        self.a_mut(next)?.join_with = JoinWith::Left;
        Ok(())
    }

    /// `BuildPathD`.
    fn build_path_d(&self, op0: usize, reverse: bool, inv_scale: f64) -> R<Option<PathD>> {
        if op0 == NIL {
            return Ok(None);
        }
        let o = *self.op(op0)?;
        if o.next == op0 || o.next == o.prev {
            return Ok(None);
        }
        let mut path = Vec::new();
        let (op, mut last_pt, mut op2) = if reverse {
            (op0, o.pt, o.prev)
        } else {
            let op = o.next;
            let oo = *self.op(op)?;
            (op, oo.pt, oo.next)
        };
        path.push(PD { x: last_pt.x as f64 * inv_scale, y: last_pt.y as f64 * inv_scale });
        let mut g = self.guard_ops();
        while op2 != op {
            g.tick()?;
            let p = *self.op(op2)?;
            if p.pt != last_pt {
                last_pt = p.pt;
                path.push(PD { x: last_pt.x as f64 * inv_scale, y: last_pt.y as f64 * inv_scale });
            }
            op2 = if reverse { p.prev } else { p.next };
        }
        if path.len() == 3 && self.is_very_small_triangle(op2)? {
            return Ok(None);
        }
        Ok(Some(path))
    }

    /// `ClipperD::Execute(clip_type, fill_rule, closed_paths)`.
    pub(crate) fn execute_d(&mut self, ct: ClipType, fr: FillRule, inv_scale: f64) -> R<Vec<PathD>> {
        let mut out = Vec::new();
        if self.execute_internal(ct, fr)? {
            // the list grows while it is walked (CleanCollinear splits): the C++ rereads its size every step
            let mut i = 0;
            while i < self.outrecs.len() {
                if self.or(i)?.pts != NIL {
                    self.clean_collinear(i)?;
                    let pts = self.or(i)?.pts;
                    if let Some(p) = self.build_path_d(pts, self.reverse_solution, inv_scale)? {
                        out.push(p);
                    }
                }
                i += 1;
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{ClipType, FillRule, PD, boolean_op_d};

    fn sq(x: f64, y: f64, s: f64) -> Vec<PD> {
        vec![PD { x, y }, PD { x: x + s, y }, PD { x: x + s, y: y + s }, PD { x, y: y + s }]
    }

    #[test]
    fn union_of_overlapping_squares() {
        let r = boolean_op_d(ClipType::Union, FillRule::Positive, &[sq(0.0, 0.0, 2.0), sq(1.0, 1.0, 2.0)], &[], 8).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].len(), 8);
        let a: f64 = r.iter().map(|p| super::super::area_d(p)).sum();
        assert_eq!(a, 7.0);
    }

    #[test]
    fn difference_makes_a_hole() {
        let r = boolean_op_d(ClipType::Difference, FillRule::Positive, &[sq(0.0, 0.0, 4.0)], &[sq(1.0, 1.0, 1.0)], 8).unwrap();
        assert_eq!(r.len(), 2);
        let a: f64 = r.iter().map(|p| super::super::area_d(p)).sum();
        assert_eq!(a, 15.0);
    }

    #[test]
    fn degenerate_inputs_never_panic() {
        let cases: Vec<Vec<PD>> = vec![
            vec![],
            vec![PD { x: 0.0, y: 0.0 }],
            vec![PD { x: 0.0, y: 0.0 }, PD { x: 1.0, y: 0.0 }],
            vec![PD { x: 0.0, y: 0.0 }, PD { x: 1.0, y: 0.0 }, PD { x: 2.0, y: 0.0 }],
            vec![PD { x: 0.0, y: 0.0 }; 5],
            vec![PD { x: f64::NAN, y: 0.0 }, PD { x: 1.0, y: f64::NAN }, PD { x: 1.0, y: 1.0 }],
        ];
        for c in &cases {
            for fr in [FillRule::EvenOdd, FillRule::NonZero, FillRule::Positive, FillRule::Negative] {
                let _ = boolean_op_d(ClipType::Union, fr, std::slice::from_ref(c), &[], 8);
                let _ = boolean_op_d(ClipType::Intersection, fr, &[sq(0.0, 0.0, 1.0)], std::slice::from_ref(c), 8);
            }
        }
    }
}
