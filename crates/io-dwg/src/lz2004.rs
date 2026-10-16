//! R2004-family LZ77 decompression (port of lib/dwg/lz-r2004.js; spec 4.7).
//!
//! Spec gap (lz-r2004.js): the copy source is (current position − compOffset − 1).
//! Reading past the input stops the decompression (the JavaScript reader's `undefined` arithmetic ends
//! its loop the same way); the output keeps what was decoded.

use crate::bits::{Res, err};
use crate::messages::msg;

struct Lz<'a> {
    src: &'a [u8],
    ip: usize,
    out: Vec<u8>,
    op: usize,
}

impl Lz<'_> {
    fn next(&mut self) -> Option<u32> {
        let b = self.src.get(self.ip).copied();
        self.ip = self.ip.saturating_add(1);
        b.map(u32::from)
    }
    fn literal_length(&mut self) -> Option<usize> {
        let b = u32::from(*self.src.get(self.ip)?);
        if b == 0 {
            self.ip = self.ip.saturating_add(1);
            let mut total: usize = 0x0F;
            loop {
                let c = self.next()?;
                if c != 0 {
                    return Some(total.saturating_add(c as usize).saturating_add(3));
                }
                total = total.saturating_add(0xFF);
            }
        }
        if b & 0xF0 != 0 {
            return Some(0);
        }
        self.ip = self.ip.saturating_add(1);
        Some((b as usize).saturating_add(3))
    }
    fn long_count(&mut self) -> Option<usize> {
        let c = self.next()?;
        if c != 0 {
            return Some(c as usize);
        }
        let mut total: usize = 0xFF;
        loop {
            let c = self.next()?;
            if c != 0 {
                return Some(total.saturating_add(c as usize));
            }
            total = total.saturating_add(0xFF);
        }
    }
    fn copy_lit(&mut self, n: usize) {
        let k = n.min(self.out.len().saturating_sub(self.op));
        if k > 0 {
            for i in 0..k {
                let b = self.src.get(self.ip.saturating_add(i)).copied().unwrap_or(0);
                if let Some(o) = self.out.get_mut(self.op.saturating_add(i)) {
                    *o = b;
                }
            }
        }
        self.ip = self.ip.saturating_add(n);
        self.op = self.op.saturating_add(k);
    }
    fn copy_back(&mut self, offset: usize, mut n: usize) -> Res<()> {
        let Some(mut s) = self.op.checked_sub(offset).and_then(|x| x.checked_sub(1)) else {
            return err(msg("ccdwg.err.lz_before_start", &[]));
        };
        let size = self.out.len();
        if self.op.saturating_add(n) > size {
            n = size.saturating_sub(self.op);
        }
        for _ in 0..n {
            let b = self.out.get(s).copied().unwrap_or(0);
            if let Some(o) = self.out.get_mut(self.op) {
                *o = b;
            }
            self.op = self.op.saturating_add(1);
            s = s.saturating_add(1);
        }
        Ok(())
    }
}

/// Decompress `src[from..to]` into `out_size` bytes.
pub fn decompress_r2004(src: &[u8], from: usize, to: usize, out_size: usize) -> Res<Vec<u8>> {
    let mut z = Lz { src, ip: from, out: vec![0u8; out_size], op: 0 };
    let Some(n) = z.literal_length() else { return Ok(z.out) };
    z.copy_lit(n);
    while z.ip < to {
        let Some(op1) = z.next() else { break };
        if op1 == 0x11 {
            break;
        }
        let (count, offset, mut lit);
        if op1 >= 0x40 {
            count = (((op1 & 0xF0) >> 4) as usize).saturating_sub(1);
            let Some(op2) = z.next() else { break };
            offset = ((op2 << 2) | ((op1 & 0x0C) >> 2)) as usize;
            lit = (op1 & 3) as usize;
        } else {
            let c = if op1 == 0x10 {
                let Some(l) = z.long_count() else { break };
                l.saturating_add(9)
            } else if (0x12..=0x1F).contains(&op1) {
                ((op1 & 0x0F) as usize).saturating_add(2)
            } else if op1 == 0x20 {
                let Some(l) = z.long_count() else { break };
                l.saturating_add(0x21)
            } else if op1 >= 0x21 {
                (op1 as usize).saturating_sub(0x1E) // op1 >= 0x21
            } else {
                return err(msg("ccdwg.err.lz_opcode", &[("op", &format!("{op1:x}"))]));
            };
            count = c;
            let (Some(b1), Some(b2)) = (z.next(), z.next()) else { break };
            let mut o = ((b1 >> 2) | (b2 << 6)) as usize;
            lit = (b1 & 3) as usize;
            if op1 < 0x20 {
                o = o.saturating_add(0x3FFF);
            }
            offset = o;
        }
        z.copy_back(offset, count)?;
        if lit == 0 {
            let Some(l) = z.literal_length() else { break };
            lit = l;
        }
        z.copy_lit(lit);
        if z.op >= out_size {
            break;
        }
    }
    Ok(z.out)
}
