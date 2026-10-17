//! zip.js — a minimal ZIP writer (stored entries, CRC-32, UTF-8 names) for the NC file set, and `crc32` (drawing and
//! model refs use it too). Deterministic for a given date.

fn table() -> &'static [u32; 256] {
    static T: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0u32; 256];
        for (n, slot) in (0u32..).zip(t.iter_mut()) {
            let mut c = n;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *slot = c;
        }
        t
    })
}
/// `x >>> 0` of a length (DataView.setUint32 keeps the low 32 bits).
fn u32_of(x: usize) -> u32 {
    u32::try_from(x & 0xffff_ffff).unwrap_or(0)
}
fn u16_of(x: usize) -> u16 {
    u16::try_from(x & 0xffff).unwrap_or(0)
}

/// `crc32(bytes)`.
pub fn crc32(bytes: &[u8]) -> u32 {
    let t = table();
    let mut c: u32 = 0xffff_ffff;
    for &b in bytes {
        let i = usize::from(u8::try_from(c & 0xff).unwrap_or(0) ^ b);
        c = t.get(i).copied().unwrap_or(0) ^ (c >> 8);
    }
    c ^ 0xffff_ffff
}
/// The local date and time a zip's entries carry (`new Date()` fields).
#[derive(Clone, Copy, Debug)]
pub struct DosDate {
    pub year: u32,
    pub month: u32,
    pub day: u32,
    pub hours: u32,
    pub minutes: u32,
    pub seconds: u32,
}

/// `zip(files, date)` → bytes.
pub fn zip(files: &[(String, Vec<u8>)], date: DosDate) -> Vec<u8> {
    let dos_time = ((date.hours << 11) | (date.minutes << 5) | (date.seconds >> 1)) & 0xffff;
    let dos_date = ((date.year.wrapping_sub(1980) << 9) | (date.month << 5) | date.day) & 0xffff;
    let (dt, dd) = (u16::try_from(dos_time & 0xffff).unwrap_or(0), u16::try_from(dos_date & 0xffff).unwrap_or(0));
    let mut locals: Vec<u8> = Vec::new();
    let mut centrals: Vec<u8> = Vec::new();
    let mut offset: u32 = 0;
    for (name, data) in files {
        let nb = name.as_bytes();
        let crc = crc32(data);
        let (dl, nl) = (u32_of(data.len()), u16_of(nb.len()));
        locals.extend(0x0403_4b50u32.to_le_bytes());
        locals.extend(20u16.to_le_bytes());
        locals.extend(0x0800u16.to_le_bytes());
        locals.extend(0u16.to_le_bytes());
        locals.extend(dt.to_le_bytes());
        locals.extend(dd.to_le_bytes());
        locals.extend(crc.to_le_bytes());
        locals.extend(dl.to_le_bytes());
        locals.extend(dl.to_le_bytes());
        locals.extend(nl.to_le_bytes());
        locals.extend(0u16.to_le_bytes());
        locals.extend(nb);
        locals.extend(data);
        centrals.extend(0x0201_4b50u32.to_le_bytes());
        centrals.extend(20u16.to_le_bytes());
        centrals.extend(20u16.to_le_bytes());
        centrals.extend(0x0800u16.to_le_bytes());
        centrals.extend(0u16.to_le_bytes());
        centrals.extend(dt.to_le_bytes());
        centrals.extend(dd.to_le_bytes());
        centrals.extend(crc.to_le_bytes());
        centrals.extend(dl.to_le_bytes());
        centrals.extend(dl.to_le_bytes());
        centrals.extend(nl.to_le_bytes());
        centrals.extend([0u8; 12]);
        centrals.extend(offset.to_le_bytes());
        centrals.extend(nb);
        offset = offset.wrapping_add(30).wrapping_add(u32_of(nb.len())).wrapping_add(dl);
    }
    let n = u16_of(files.len());
    let cd = u32_of(centrals.len());
    let mut out = locals;
    out.extend(centrals);
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend([0u8; 4]);
    out.extend(n.to_le_bytes());
    out.extend(n.to_le_bytes());
    out.extend(cd.to_le_bytes());
    out.extend(offset.to_le_bytes());
    out.extend([0u8; 2]);
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn crc() {
        assert_eq!(super::crc32(b"123456789"), 0xcbf4_3926);
    }
}
