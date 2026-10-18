//! The control channel's secret (AGENTS.md § 9, § 15; desktop-shell control.rs): 32 random bytes from the operating
//! system's random source, base64url without padding (43 characters), compared in constant time, written `0600` to
//! `control.token` in the config folder (made `0700`). Never in a URL, a log line or a screenshot.

use std::path::{Path, PathBuf};

/// Random bytes from the OS (getrandom). An error means the system has no random source: no token, no channel.
pub fn random_bytes<const N: usize>() -> Result<[u8; N], String> {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).map_err(|e| format!("the system's random source failed: {e}"))?;
    Ok(b)
}

/// base64url without padding.
pub fn base64url(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().saturating_mul(4).div_ceil(3));
    let ch = |i: u32| char::from(A.get((i & 63) as usize).copied().unwrap_or(b'A'));
    for c in bytes.chunks(3) {
        let b0 = u32::from(c.first().copied().unwrap_or(0));
        let b1 = u32::from(c.get(1).copied().unwrap_or(0));
        let b2 = u32::from(c.get(2).copied().unwrap_or(0));
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ch(n >> 18));
        out.push(ch(n >> 12));
        if c.len() > 1 {
            out.push(ch(n >> 6));
        }
        if c.len() > 2 {
            out.push(ch(n));
        }
    }
    out
}

/// A new token: 32 random bytes, base64url.
pub fn random_token() -> Result<String, String> {
    Ok(base64url(&random_bytes::<32>()?))
}

/// A session id: 16 random bytes as 32 lower-case hex digits.
pub fn session_id() -> Result<String, String> {
    Ok(random_bytes::<16>()?.iter().map(|b| format!("{b:02x}")).collect())
}

/// Constant time for equal lengths; the same amount of work when the lengths differ (control.rs `token_matches`).
pub fn token_matches(given: &str, token: &str) -> bool {
    let (a, b) = (given.as_bytes(), token.as_bytes());
    if b.is_empty() || a.len() != b.len() {
        let _ = std::hint::black_box(b.iter().fold(0u8, |acc, x| acc | (x ^ std::hint::black_box(*x))));
        return false;
    }
    std::hint::black_box(a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y))) == 0
}

/// The app's config folder: `APP_CONFIG_DIR` (tests), else the platform's (XDG / Application Support / %APPDATA%).
pub fn config_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("APP_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d));
    }
    const NAME: &str = "cadcraft-rust";
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join(NAME))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support").join(NAME))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .filter(|x| !x.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|c| c.join(NAME))
    }
}

/// Write the token to `<dir>/control.token`: the folder made `0700`, the file `0600` (Unix); replaced atomically.
/// Windows: the file lands in the per-user %APPDATA% folder (no ACL of our own yet: docs/gaps.md).
pub fn write_token_file(dir: &Path, token: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("could not make the config folder: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    let path = dir.join("control.token");
    let tmp = dir.join(format!(".control.token.{}.tmp", std::process::id()));
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    {
        use std::io::Write;
        let mut f = opts.open(&tmp).map_err(|e| format!("could not write the token file: {e}"))?;
        f.write_all(token.as_bytes()).map_err(|e| format!("could not write the token file: {e}"))?;
        f.sync_all().map_err(|e| format!("could not write the token file: {e}"))?;
    }
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("could not write the token file: {e}")
    })?;
    Ok(path)
}

/// Read a token file (for `cadcraft-cli mcp --connect`): the first line, trimmed; at most 1 KB is read.
pub fn read_token_file(path: &Path) -> Result<String, String> {
    use std::io::Read;
    let f = std::fs::File::open(path).map_err(|e| format!("could not read the token file: {e}"))?;
    let mut s = String::new();
    f.take(1024).read_to_string(&mut s).map_err(|e| format!("could not read the token file: {e}"))?;
    let t = s.lines().next().unwrap_or_default().trim().to_string();
    if t.is_empty() {
        return Err("the token file is empty".into());
    }
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens() {
        let t = random_token().unwrap();
        assert_eq!(t.len(), 43, "32 bytes base64url without padding");
        assert!(t.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'));
        assert_ne!(t, random_token().unwrap());
        assert!(token_matches(&t, &t));
        assert!(!token_matches(&t[..42], &t) && !token_matches("", &t) && !token_matches(&t, ""));
        assert_eq!(base64url(b"\xfb\xff"), "-_8");
        assert_eq!(session_id().unwrap().len(), 32);
    }

    #[test]
    fn token_file_is_private() {
        let dir = std::env::temp_dir().join(format!("cc-mcp-token-{}", std::process::id()));
        let p = write_token_file(&dir, "abc").unwrap();
        assert_eq!(read_token_file(&p).unwrap(), "abc");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
            assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
