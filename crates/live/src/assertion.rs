//! Signed-in names (LIVE-CRYPTO.md § Signed-in names, LV.19 / SI.8): live-crypto.js `verifyAssertion`,
//! `assertionAudience` and `pictureMatches`, checked offline against getcadcraft.com's pinned Ed25519 keys.
//!
//! A token is `v1.` + base64url(JSON claims) + `.` + base64url(Ed25519 signature over `cadcraft-live-assertion/1.` + the
//! claims part). The host checks it against the audience of the joiner's connection (`aud`, the `join` card's field:
//! base64url of `exporter("assertion")`), so a tick can't be replayed into another session. Getting one needs the site
//! (`POST /api/v1/live/assertion` with a device token): the desktop has no HTTP client, so this window never sends one of
//! its own; it checks the ones joiners bring.
//!
//! Spec gap: 0.5 calls noble-curves' `ed25519.verify(sig, msg, pub, { zip215: false })`. Its rules, followed here one by
//! one: the signature is 64 bytes and the key 32; `S` (the signature's last 32 bytes, little-endian) must be < L, else no;
//! `A` and `R` decode under RFC 8032's strict rules — `y` < p (no unreduced encodings) and no `x = 0` with the sign bit
//! set — which is exactly "re-encoding the decoded point gives the same bytes"; `A` of small order is refused; the
//! challenge `k` = SHA-512(R ‖ A ‖ message) mod L over the bytes as given; and the equation is the cofactored one,
//! `[8]([S]B − R − [k]A) = 0`. `R` of small order is not refused (noble doesn't).

use curve25519_dalek::edwards::{CompressedEdwardsY, EdwardsPoint};
use curve25519_dalek::scalar::Scalar;
use curve25519_dalek::traits::IsIdentity;
use sha2::{Digest, Sha256, Sha512};

/// getcadcraft.com's public keys, as live-crypto.js `ASSERTION_KEYS` pins them (kid, base64url key).
pub const ASSERTION_KEYS: &[(&str, &str)] = &[("a1f5b9cc", "e2Bo1-R1Hnd9kDkp3I7LvT1kZWGMm1Ha2K_XRQVW6DY")];
const DOMAIN: &[u8] = b"cadcraft-live-assertion/1.";
/// The longest token looked at (`hello.assertion` is capped at 2 KB on the wire too).
pub const MAX_TOKEN: usize = 4096;

/// What a checked assertion says (live-crypto.js `{ok: true, name, sub, verified, pic}`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signed {
    pub name: String,
    pub sub: String,
    pub verified: bool,
    /// sha256 hex of the person's 64 × 64 picture, when they have one
    pub pic: Option<String>,
}

/// Why an assertion was refused (live-crypto.js reasons).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    Format,
    Key,
    Signature,
    Expired,
    Audience,
}

impl Refused {
    pub fn as_str(self) -> &'static str {
        match self {
            Refused::Format => "format",
            Refused::Key => "key",
            Refused::Signature => "signature",
            Refused::Expired => "expired",
            Refused::Audience => "audience",
        }
    }
}

/// base64url without padding → bytes, as live-crypto.js `fromB64url`: leftover bits must be zero, a length of 4n+1 is no.
pub fn from_b64url(s: &str) -> Option<Vec<u8>> {
    let mut bits: u32 = 0;
    let mut n: u32 = 0;
    let mut out = Vec::with_capacity(s.len().saturating_mul(3) / 4);
    for ch in s.bytes() {
        let v = u32::from(match ch {
            b'A'..=b'Z' => ch.wrapping_sub(b'A'),
            b'a'..=b'z' => ch.wrapping_sub(b'a').wrapping_add(26),
            b'0'..=b'9' => ch.wrapping_sub(b'0').wrapping_add(52),
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        });
        bits = (bits << 6) | v;
        n = n.wrapping_add(6);
        if n >= 8 {
            n = n.wrapping_sub(8);
            out.push(u8::try_from((bits >> n) & 0xff).ok()?);
            bits &= (1u32 << n).wrapping_sub(1);
        }
    }
    if bits != 0 || s.chars().count() % 4 == 1 {
        return None;
    }
    Some(out)
}

/// Ed25519 verification with noble's strict (`zip215: false`) rules (see the module's "Spec gap").
// group and scalar operations, not integers: nothing can overflow
#[allow(clippy::arithmetic_side_effects)]
pub fn ed25519_verify(sig: &[u8], msg: &[u8], public: &[u8]) -> bool {
    let (Ok(sig), Ok(public)) = (<[u8; 64]>::try_from(sig), <[u8; 32]>::try_from(public)) else { return false };
    let mut r = [0u8; 32];
    let mut s = [0u8; 32];
    r.copy_from_slice(sig.get(..32).unwrap_or(&[0; 32]));
    s.copy_from_slice(sig.get(32..).unwrap_or(&[0; 32]));
    let Some(s) = Option::<Scalar>::from(Scalar::from_canonical_bytes(s)) else { return false };
    let strict = |b: &[u8; 32]| -> Option<EdwardsPoint> {
        let p = CompressedEdwardsY(*b).decompress()?;
        (p.compress().as_bytes() == b).then_some(p)
    };
    let (Some(a), Some(rp)) = (strict(&public), strict(&r)) else { return false };
    if a.is_small_order() {
        return false;
    }
    let mut h = Sha512::new();
    h.update(r);
    h.update(public);
    h.update(msg);
    let k = Scalar::from_bytes_mod_order_wide(&h.finalize().into());
    // [S]B − [k]A − R, times the cofactor
    let sb_minus_ka = EdwardsPoint::vartime_double_scalar_mul_basepoint(&(-k), &a, &s);
    (sb_minus_ka - rp).mul_by_cofactor().is_identity()
}

/// A JSON number as JS reads `claims.x` in arithmetic: anything else is NaN.
fn num(v: Option<&serde_json::Value>) -> f64 {
    v.and_then(serde_json::Value::as_f64).unwrap_or(f64::NAN)
}
fn truthy(v: Option<&serde_json::Value>) -> bool {
    match v {
        None | Some(serde_json::Value::Null) => false,
        Some(serde_json::Value::Bool(b)) => *b,
        Some(serde_json::Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Some(serde_json::Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// live-crypto.js `verifyAssertion(token, {aud, now, keys, skew: 60})`. `now` in seconds since 1970.
pub fn verify_assertion(token: &str, aud: &str, now: f64, keys: &[(&str, &str)]) -> Result<Signed, Refused> {
    // /^v1\.([A-Za-z0-9_-]{20,2000})\.([A-Za-z0-9_-]{86})$/
    if token.len() > MAX_TOKEN {
        return Err(Refused::Format);
    }
    let b64ok = |s: &str| s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_');
    let rest = token.strip_prefix("v1.").ok_or(Refused::Format)?;
    let (claims_part, sig_part) = rest.split_once('.').ok_or(Refused::Format)?;
    if !(20..=2000).contains(&claims_part.len()) || sig_part.len() != 86 || !b64ok(claims_part) || !b64ok(sig_part) {
        return Err(Refused::Format);
    }
    let body = from_b64url(claims_part).ok_or(Refused::Format)?;
    let sig = from_b64url(sig_part).ok_or(Refused::Format)?;
    let text = String::from_utf8_lossy(&body);
    let claims: serde_json::Value = serde_json::from_str(&text).map_err(|_| Refused::Format)?;
    let c = claims.as_object().ok_or(Refused::Format)?;
    if num(c.get("v")) != 1.0 {
        return Err(Refused::Format);
    }
    let name = c.get("name").and_then(serde_json::Value::as_str).ok_or(Refused::Format)?.to_string();
    let kid = c.get("kid").and_then(serde_json::Value::as_str);
    let public = keys.iter().find(|(k, _)| Some(*k) == kid).and_then(|(_, key)| from_b64url(key)).ok_or(Refused::Key)?;
    if public.len() != 32 {
        return Err(Refused::Key);
    }
    let mut msg = DOMAIN.to_vec();
    msg.extend_from_slice(claims_part.as_bytes());
    if !ed25519_verify(&sig, &msg, &public) {
        return Err(Refused::Signature);
    }
    const SKEW: f64 = 60.0;
    let (exp, iat) = (num(c.get("exp")), num(c.get("iat")));
    let in_time = exp + SKEW > now && iat - SKEW <= now;
    if !in_time {
        return Err(Refused::Expired);
    }
    if c.get("aud").and_then(serde_json::Value::as_str) != Some(aud) {
        return Err(Refused::Audience);
    }
    let sub = match c.get("sub") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Null) | None => String::new(),
        Some(v) => v.to_string(),
    };
    let pic = c.get("pic").filter(|p| truthy(Some(p))).map(|p| p.as_str().map_or_else(|| p.to_string(), str::to_string));
    Ok(Signed { name, sub, verified: truthy(c.get("verified")), pic })
}

/// live-crypto.js `pictureMatches(bytes, pic)`: the picture that came with a joiner is the one their assertion names.
pub fn picture_matches(bytes: &[u8], pic: &str) -> bool {
    if bytes.len() > 16384 {
        return false;
    }
    let d = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for b in d {
        use std::fmt::Write;
        let _ = write!(hex, "{b:02x}");
    }
    hex == pic
}

#[cfg(test)]
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect()
    }

    // RFC 8032 § 7.1, tests 1–3 (public key, message, signature)
    const RFC: &[(&str, &str, &str)] = &[
        (
            "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
            "",
            "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
        ),
        (
            "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
            "72",
            "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
        ),
        (
            "fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025",
            "af82",
            "6291d657deec24024827e69c3abe01a30ce548a284743a445e3680d7db5ac3ac18ff9b538d16f290ae67f760984dc6594a7c15e9716ed28dc027beceea1ec40a",
        ),
    ];

    #[test]
    fn rfc8032_vectors_pass_and_mutations_fail() {
        for (pk, m, s) in RFC {
            let (pk, m, s) = (hex(pk), hex(m), hex(s));
            assert!(ed25519_verify(&s, &m, &pk));
            for i in [0usize, 17, 31, 32, 50, 63] {
                let mut bad = s.clone();
                bad[i] ^= 1;
                assert!(!ed25519_verify(&bad, &m, &pk), "flipped bit {i}");
            }
            let mut m2 = m.clone();
            m2.push(0);
            assert!(!ed25519_verify(&s, &m2, &pk));
            // S + L (same S mod L, not canonical): refused
            let mut sl = s.clone();
            let l = hex("edd3f55c1a631258d69cf7a2def9de1400000000000000000000000000000010");
            let mut carry = 0u16;
            for i in 0..32 {
                let v = u16::from(sl[32 + i]) + u16::from(l[i]) + carry;
                sl[32 + i] = (v & 0xff) as u8;
                carry = v >> 8;
            }
            if carry == 0 {
                assert!(!ed25519_verify(&sl, &m, &pk), "S >= L");
            }
            // R with the sign bit forced on an x = 0 point is not this R; a non-canonical R (y + p) is refused
            assert!(!ed25519_verify(&s[..63], &m, &pk));
        }
    }

    #[test]
    fn non_canonical_and_small_order_encodings_are_refused() {
        // y = 1 (the identity) as a public key: small order
        let mut id = [0u8; 32];
        id[0] = 1;
        let (pk, m, s) = (hex(RFC[0].0), hex(RFC[0].1), hex(RFC[0].2));
        assert!(!ed25519_verify(&s, &m, &id));
        // y = p + 1 ≡ 1, unreduced: refused before anything else
        let unreduced = hex("eeffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f");
        assert!(!ed25519_verify(&s, &m, &unreduced));
        // x = 0 with the sign bit set (y = 1, sign 1)
        let mut neg0 = id;
        neg0[31] |= 0x80;
        let mut sig = s.clone();
        sig[..32].copy_from_slice(&neg0);
        assert!(!ed25519_verify(&sig, &m, &pk));
        // a non-canonical R: R's y + p (only when it fits under 2^255)
        let mut r = s[..32].to_vec();
        let sign = r[31] & 0x80;
        r[31] &= 0x7f;
        let p = hex("edffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f");
        let mut carry = 0u16;
        let mut out = [0u8; 32];
        for i in 0..32 {
            let v = u16::from(r[i]) + u16::from(p[i]) + carry;
            out[i] = (v & 0xff) as u8;
            carry = v >> 8;
        }
        if carry == 0 && out[31] & 0x80 == 0 {
            out[31] |= sign;
            let mut sig = s.clone();
            sig[..32].copy_from_slice(&out);
            assert!(!ed25519_verify(&sig, &m, &pk));
        }
    }

    #[test]
    fn b64url_as_live_crypto() {
        assert_eq!(from_b64url("-_8").as_deref(), Some(&[0xfb, 0xff][..]));
        assert_eq!(from_b64url("A"), None);
        assert_eq!(from_b64url("AB"), None); // leftover bits
        assert_eq!(from_b64url("a+b"), None);
        assert_eq!(from_b64url("").as_deref(), Some(&[][..]));
    }

    #[test]
    fn hostile_tokens_are_format() {
        for t in ["", "v1.abc", "v2.aaaaaaaaaaaaaaaaaaaaaaaa.x", "v1..", &"v1.".repeat(3000)] {
            assert_eq!(verify_assertion(t, "x", 0.0, ASSERTION_KEYS), Err(Refused::Format));
        }
        assert!(picture_matches(b"", "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"));
        assert!(!picture_matches(&vec![0; 20000], ""));
    }
}
