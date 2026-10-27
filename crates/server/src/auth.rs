//! Bearer-token authentication and session binding.
//!
//! Every MCP request must carry `Authorization: Bearer <token>`. Tokens are
//! stored as SHA-256 digests and compared in constant time, so a wrong token
//! leaks nothing through timing. Once a legacy session id is seen, it is bound
//! to the token that first presented it, preventing session hijacking.

use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Request, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use subtle::ConstantTimeEq;
use tokio::sync::Mutex;

use crate::hash::sha256;

/// A configured token: a stable id and the SHA-256 of its secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenEntry {
    /// Stable identifier, used in logs and for revocation.
    pub id: String,
    /// SHA-256 digest of the secret.
    pub hash: [u8; 32],
}

/// The set of accepted tokens.
pub struct TokenStore {
    entries: Vec<TokenEntry>,
}

impl TokenStore {
    /// Build a store from resolved entries.
    pub fn new(entries: Vec<TokenEntry>) -> Self {
        Self { entries }
    }

    /// Verify a presented secret, returning the matching token id.
    ///
    /// The comparison runs over every entry without early exit so the running
    /// time does not depend on which entry matches.
    pub fn verify(&self, presented: &str) -> Option<&str> {
        let digest = sha256(presented.as_bytes());
        let mut matched: Option<&str> = None;
        for entry in &self.entries {
            if bool::from(entry.hash.ct_eq(&digest)) {
                matched = Some(entry.id.as_str());
            }
        }
        matched
    }
}

/// The authenticated principal attached to a request.
#[derive(Debug, Clone)]
pub struct Principal {
    /// Id of the token that authenticated the request.
    pub token_id: String,
}

/// Shared authentication state.
#[derive(Clone)]
pub struct AuthState {
    store: Arc<TokenStore>,
    sessions: Arc<Mutex<HashMap<String, String>>>,
}

impl AuthState {
    /// Build auth state around a token store.
    pub fn new(store: Arc<TokenStore>) -> Self {
        Self {
            store,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

/// Extract a bearer token from a raw `Authorization` header value.
///
/// The scheme match is case-insensitive per RFC 7235. Kept free of transport
/// types so it can be unit-tested and fuzzed in isolation.
pub fn parse_bearer(value: &str) -> Option<&str> {
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("Bearer") {
        return None;
    }
    let token = token.trim();
    if token.is_empty() {
        return None;
    }
    Some(token)
}

/// Extract a bearer token from the `Authorization` header.
pub fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    parse_bearer(headers.get(header::AUTHORIZATION)?.to_str().ok()?)
}

/// Bind a session id to a token id, or check an existing binding.
///
/// Returns `true` when the session is (now) owned by `token_id`, `false` when
/// it is already bound to a different token.
pub fn check_and_bind(
    sessions: &mut HashMap<String, String>,
    session_id: &str,
    token_id: &str,
) -> bool {
    match sessions.get(session_id) {
        Some(existing) => existing == token_id,
        None => {
            sessions.insert(session_id.to_string(), token_id.to_string());
            true
        }
    }
}

/// Axum middleware enforcing bearer authentication and session binding.
pub async fn require_bearer(
    State(state): State<AuthState>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let token_id = bearer_token(request.headers())
        .and_then(|token| state.store.verify(token))
        .map(str::to_owned);

    let Some(token_id) = token_id else {
        return StatusCode::UNAUTHORIZED.into_response();
    };

    let session_id = request
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);

    if let Some(session_id) = session_id {
        let allowed = {
            let mut sessions = state.sessions.lock().await;
            check_and_bind(&mut sessions, &session_id, &token_id)
        };
        if !allowed {
            return StatusCode::FORBIDDEN.into_response();
        }
    }

    request.extensions_mut().insert(Principal { token_id });
    next.run(request).await
}
