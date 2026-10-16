// Rust port (2026, CadCraft) of Manifold 3.5.4's src/disjoint_sets.h, which is
// from https://github.com/wjakob/dset, changed to add connected component computation.
// This is an altered version (ported to Rust, single-threaded).
//
// Copyright (c) 2015 Wenzel Jakob <wenzel@inf.ethz.ch>
//
// This software is provided 'as-is', without any express or implied warranty. In no event will the authors be held
// liable for any damages arising from the use of this software.
//
// Permission is granted to anyone to use this software for any purpose, including commercial applications, and to
// alter it and redistribute it freely, subject to the following restrictions:
//
// 1. The origin of this software must not be misrepresented; you must not claim that you wrote the original software.
// If you use this software in a product, an acknowledgment in the product documentation would be appreciated but is
// not required.
// 2. Altered source versions must be plainly marked as such, and must not be misrepresented as being the original
// software.
// 3. This notice may not be removed or altered from any source distribution.
//
//! disjoint_sets.h: union-find with rank in the high 32 bits and path halving, run single-threaded (every
//! compare-exchange succeeds), so the parent links come out exactly as in the sequential C++ build.
#![allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

use crate::util::{Fault, Get, GetMut, R};

pub(crate) struct DisjointSets {
    data: Vec<u64>,
}

impl DisjointSets {
    pub(crate) fn new(size: usize) -> R<DisjointSets> {
        if size > u32::MAX as usize {
            return Err(Fault::TooLarge);
        }
        Ok(DisjointSets { data: (0..size as u64).collect() })
    }

    #[inline]
    fn rank(&self, id: u32) -> R<u32> {
        Ok(((self.data.atu(id as usize)? >> 32) as u32) & 0x7FFF_FFFF)
    }

    #[inline]
    fn parent(&self, id: u32) -> R<u32> {
        Ok(self.data.atu(id as usize)? as u32)
    }

    fn find_impl(&mut self, mut id: u32) -> R<u32> {
        let mut guard = crate::util::Guard::new(self.data.len());
        while id != self.parent(id)? {
            guard.tick()?;
            let value = self.data.atu(id as usize)?;
            let new_parent = self.parent(value as u32)?;
            let new_value = (value & 0xFFFF_FFFF_0000_0000) | u64::from(new_parent);
            if value != new_value {
                self.data.setu(id as usize, new_value)?;
            }
            id = new_parent;
        }
        Ok(id)
    }

    pub(crate) fn find(&mut self, id: usize) -> R<usize> {
        let id = u32::try_from(id).map_err(|_| Fault::Index)?;
        Ok(self.find_impl(id)? as usize)
    }

    pub(crate) fn unite(&mut self, a: usize, b: usize) -> R<usize> {
        let mut id1 = u32::try_from(a).map_err(|_| Fault::Index)?;
        let mut id2 = u32::try_from(b).map_err(|_| Fault::Index)?;
        id1 = self.find_impl(id1)?;
        id2 = self.find_impl(id2)?;
        if id1 == id2 {
            return Ok(id1 as usize);
        }
        let mut r1 = self.rank(id1)?;
        let mut r2 = self.rank(id2)?;
        if r1 > r2 || (r1 == r2 && id1 < id2) {
            core::mem::swap(&mut r1, &mut r2);
            core::mem::swap(&mut id1, &mut id2);
        }
        self.data.setu(id1 as usize, (u64::from(r1) << 32) | u64::from(id2))?;
        if r1 == r2 {
            self.data.setu(id2 as usize, (u64::from(r2 + 1) << 32) | u64::from(id2))?;
        }
        Ok(id2 as usize)
    }

    /// `connectedComponents`: labels in first-seen order; singletons (rank-0 roots) skip the map.
    pub(crate) fn connected_components(&mut self, components: &mut Vec<i32>) -> R<i32> {
        let n = self.data.len();
        components.clear();
        components.resize(n, 0);
        let mut lonely: i32 = 0;
        let mut to_label: crate::util::FxMap<u32, i32> = crate::util::FxMap::default();
        for i in 0..n {
            let ip = self.find_impl(i as u32)?;
            let label_count = i32::try_from(to_label.len()).map_err(|_| Fault::TooLarge)?;
            if self.rank(ip)? == 0 {
                components.setu(i, label_count + lonely)?;
                lonely += 1;
                continue;
            }
            match to_label.get(&ip) {
                Some(&l) => components.setu(i, l)?,
                None => {
                    let s = label_count + lonely;
                    to_label.insert(ip, s);
                    components.setu(i, s)?;
                }
            }
        }
        Ok(i32::try_from(to_label.len()).map_err(|_| Fault::TooLarge)? + lonely)
    }
}
