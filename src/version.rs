//! The version, stamped into the built binary by `tools/stamp.sh` at release.

const MARKER: &[u8; 16] = b"\0SM-VERSION-SLOT";
const LEN: usize = 32;

#[used]
static SLOT: [u8; MARKER.len() + LEN] = slot(env!("CARGO_PKG_VERSION").as_bytes());

const fn slot(version: &[u8]) -> [u8; MARKER.len() + LEN] {
    assert!(version.len() < LEN);
    let mut s = [0u8; MARKER.len() + LEN];
    let mut i = 0;
    while i < MARKER.len() {
        s[i] = MARKER[i];
        i += 1;
    }
    let mut j = 0;
    while j < version.len() {
        s[MARKER.len() + j] = version[j];
        j += 1;
    }
    s
}

pub fn version() -> &'static str {
    // black_box: the stamp rewrites the bytes after the build, so they must be read, not folded.
    let slot: &'static [u8; MARKER.len() + LEN] = std::hint::black_box(&SLOT);
    let value = &slot[MARKER.len()..];
    let end = value.iter().position(|&b| b == 0).unwrap_or(LEN);
    std::str::from_utf8(&value[..end]).unwrap_or("unknown")
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_the_cargo_version() {
        assert_eq!(super::version(), env!("CARGO_PKG_VERSION"));
    }
}
