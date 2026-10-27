//! SHA-256 helpers shared by configuration and authentication.

use sha2::{Digest, Sha256};

/// Length in bytes of a SHA-256 digest.
pub const HASH_LEN: usize = 32;

/// SHA-256 of `input`.
pub fn sha256(input: &[u8]) -> [u8; HASH_LEN] {
    let digest = Sha256::digest(input);
    let mut out = [0u8; HASH_LEN];
    out.copy_from_slice(&digest);
    out
}

/// Lowercase hex encoding of a digest.
pub fn to_hex(bytes: &[u8; HASH_LEN]) -> String {
    let mut out = String::with_capacity(HASH_LEN * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Parse exactly 64 hex characters into a digest.
pub fn parse_hex_32(input: &str) -> Option<[u8; HASH_LEN]> {
    let bytes = input.as_bytes();
    if bytes.len() != HASH_LEN * 2 {
        return None;
    }
    let mut out = [0u8; HASH_LEN];
    for (index, pair) in bytes.as_chunks::<2>().0.iter().enumerate() {
        let high = hex_value(pair[0])?;
        let low = hex_value(pair[1])?;
        // `+` rather than `|`: the two nibbles never overlap, so `|` would be
        // indistinguishable from `^` and leave an equivalent mutant alive.
        out[index] = (high << 4) + low;
    }
    Some(out)
}

/// Hex-encode the SHA-256 of a plaintext secret. Backs `gilfoyle hash-token`.
pub fn hash_token_hex(plaintext: &str) -> String {
    to_hex(&sha256(plaintext.as_bytes()))
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
