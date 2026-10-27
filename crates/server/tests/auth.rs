use std::collections::HashMap;

use axum::http::{HeaderMap, HeaderValue, header};
use gilfoyle_server::auth::{TokenEntry, TokenStore, bearer_token, check_and_bind, parse_bearer};
use gilfoyle_server::hash::sha256;

fn entry(id: &str, secret: &str) -> TokenEntry {
    TokenEntry {
        id: id.to_string(),
        hash: sha256(secret.as_bytes()),
    }
}

fn headers(authorization: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_str(authorization).unwrap(),
    );
    headers
}

#[test]
fn extracts_a_well_formed_bearer_token() {
    assert_eq!(bearer_token(&headers("Bearer abc123")), Some("abc123"));
}

#[test]
fn scheme_is_case_insensitive() {
    assert_eq!(bearer_token(&headers("bearer abc123")), Some("abc123"));
    assert_eq!(bearer_token(&headers("BEARER abc123")), Some("abc123"));
}

#[test]
fn trims_surrounding_whitespace() {
    assert_eq!(bearer_token(&headers("Bearer   abc123  ")), Some("abc123"));
}

#[test]
fn rejects_missing_header() {
    assert_eq!(bearer_token(&HeaderMap::new()), None);
}

#[test]
fn rejects_wrong_scheme() {
    assert_eq!(bearer_token(&headers("Token abc123")), None);
}

#[test]
fn rejects_missing_separator() {
    assert_eq!(bearer_token(&headers("Bearer")), None);
}

#[test]
fn rejects_empty_token() {
    assert_eq!(bearer_token(&headers("Bearer ")), None);
    assert_eq!(bearer_token(&headers("Bearer    ")), None);
}

#[test]
fn parse_bearer_is_case_insensitive_and_trims() {
    assert_eq!(parse_bearer("Bearer  abc  "), Some("abc"));
    assert_eq!(parse_bearer("bEaReR abc"), Some("abc"));
    assert_eq!(parse_bearer("Bearer"), None);
    assert_eq!(parse_bearer("Basic abc"), None);
    assert_eq!(parse_bearer("Bearer   "), None);
}

#[test]
fn verify_accepts_a_matching_token() {
    let store = TokenStore::new(vec![entry("admin", "s3cr3t")]);
    assert_eq!(store.verify("s3cr3t"), Some("admin"));
}

#[test]
fn verify_rejects_a_wrong_token() {
    let store = TokenStore::new(vec![entry("admin", "s3cr3t")]);
    assert_eq!(store.verify("wrong"), None);
}

#[test]
fn verify_rejects_against_an_empty_store() {
    let store = TokenStore::new(vec![]);
    assert_eq!(store.verify("anything"), None);
}

#[test]
fn verify_finds_the_right_entry_among_many() {
    let store = TokenStore::new(vec![entry("a", "alpha"), entry("b", "beta")]);
    assert_eq!(store.verify("beta"), Some("b"));
    assert_eq!(store.verify("alpha"), Some("a"));
}

#[test]
fn binds_a_new_session() {
    let mut sessions = HashMap::new();
    assert!(check_and_bind(&mut sessions, "sess-1", "admin"));
    assert_eq!(sessions.get("sess-1"), Some(&"admin".to_string()));
}

#[test]
fn accepts_the_same_token_on_a_bound_session() {
    let mut sessions = HashMap::new();
    assert!(check_and_bind(&mut sessions, "sess-1", "admin"));
    assert!(check_and_bind(&mut sessions, "sess-1", "admin"));
}

#[test]
fn rejects_a_different_token_on_a_bound_session() {
    let mut sessions = HashMap::new();
    assert!(check_and_bind(&mut sessions, "sess-1", "admin"));
    assert!(!check_and_bind(&mut sessions, "sess-1", "other"));
    assert_eq!(sessions.get("sess-1"), Some(&"admin".to_string()));
}
