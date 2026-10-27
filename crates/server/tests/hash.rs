use gilfoyle_server::hash::{HASH_LEN, hash_token_hex, parse_hex_32, sha256, to_hex};

const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

#[test]
fn hash_len_is_32() {
    assert_eq!(HASH_LEN, 32);
}

#[test]
fn sha256_matches_known_vectors() {
    assert_eq!(to_hex(&sha256(b"")), EMPTY_SHA256);
    assert_eq!(hash_token_hex("abc"), ABC_SHA256);
}

#[test]
fn to_hex_and_parse_hex_round_trip() {
    let digest = sha256(b"round trip");
    let hex = to_hex(&digest);
    assert_eq!(hex.len(), 64);
    assert_eq!(parse_hex_32(&hex), Some(digest));
}

#[test]
fn parse_hex_accepts_uppercase() {
    let upper = ABC_SHA256.to_uppercase();
    assert_eq!(parse_hex_32(&upper), parse_hex_32(ABC_SHA256));
}

#[test]
fn parse_hex_rejects_wrong_length() {
    assert_eq!(parse_hex_32(""), None);
    assert_eq!(parse_hex_32(&"a".repeat(63)), None);
    assert_eq!(parse_hex_32(&"a".repeat(65)), None);
}

#[test]
fn parse_hex_rejects_non_hex() {
    let mut bad: Vec<u8> = ABC_SHA256.bytes().collect();
    bad[0] = b'z';
    assert_eq!(parse_hex_32(std::str::from_utf8(&bad).unwrap()), None);
}
