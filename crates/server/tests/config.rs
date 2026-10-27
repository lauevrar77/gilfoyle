use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use gilfoyle_server::auth::TokenEntry;
use gilfoyle_server::config::{
    ConfigError, DEFAULT_BIND, DEFAULT_LOG_LEVEL, RawConfig, RawToken, RawTools, parse_bind,
    resolve_config,
};
use gilfoyle_server::hash::{sha256, to_hex};

fn env_from(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn no_files() -> HashMap<PathBuf, String> {
    HashMap::new()
}

fn resolve(
    raw: RawConfig,
    env: &HashMap<String, String>,
    files: &HashMap<PathBuf, String>,
) -> Result<gilfoyle_server::config::Config, ConfigError> {
    let env_lookup = |key: &str| env.get(key).cloned();
    let file_lookup = |path: &Path| files.get(path).cloned();
    resolve_config(raw, &env_lookup, &file_lookup)
}

fn token(id: &str) -> RawToken {
    RawToken {
        id: id.to_string(),
        sha256: Some(to_hex(&sha256(b"secret"))),
    }
}

#[test]
fn parse_bind_accepts_ipv4_and_ipv6() {
    assert_eq!(parse_bind("127.0.0.1:8765").unwrap().port(), 8765);
    assert!(parse_bind("[::1]:8765").unwrap().ip().is_loopback());
    assert_eq!(
        parse_bind("nonsense").unwrap_err(),
        ConfigError::InvalidBind("nonsense".to_string())
    );
}

#[test]
fn applies_defaults_when_empty() {
    let raw = RawConfig {
        tokens: vec![token("admin")],
        ..RawConfig::default()
    };
    let config = resolve(raw, &env_from(&[]), &no_files()).unwrap();
    assert_eq!(config.bind.to_string(), DEFAULT_BIND);
    assert_eq!(config.log_level, DEFAULT_LOG_LEVEL);
    assert!(!config.allow_public);
    assert_eq!(config.enabled_tools, None);
    assert_eq!(config.tokens.len(), 1);
}

#[test]
fn environment_overrides_file_values() {
    let raw = RawConfig {
        bind: Some("127.0.0.1:1000".to_string()),
        log_level: Some("debug".to_string()),
        tokens: vec![token("admin")],
        ..RawConfig::default()
    };
    let env = env_from(&[
        ("GILFOYLE_BIND", "127.0.0.1:2000"),
        ("GILFOYLE_LOG", "trace"),
    ]);
    let config = resolve(raw, &env, &no_files()).unwrap();
    assert_eq!(config.bind.to_string(), "127.0.0.1:2000");
    assert_eq!(config.log_level, "trace");
}

#[test]
fn refuses_public_bind_without_opt_in() {
    let raw = RawConfig {
        bind: Some("0.0.0.0:8765".to_string()),
        tokens: vec![token("admin")],
        ..RawConfig::default()
    };
    assert!(matches!(
        resolve(raw, &env_from(&[]), &no_files()).unwrap_err(),
        ConfigError::PublicBindWithoutOptIn(_)
    ));
}

#[test]
fn allows_public_bind_with_opt_in() {
    let raw = RawConfig {
        bind: Some("0.0.0.0:8765".to_string()),
        allow_public: true,
        tokens: vec![token("admin")],
        ..RawConfig::default()
    };
    let config = resolve(raw, &env_from(&[]), &no_files()).unwrap();
    assert!(config.allow_public);
    assert!(!config.bind.ip().is_loopback());
}

#[test]
fn fails_closed_without_tokens() {
    let raw = RawConfig::default();
    assert_eq!(
        resolve(raw, &env_from(&[]), &no_files()).unwrap_err(),
        ConfigError::NoTokens
    );
}

#[test]
fn rejects_empty_token_id() {
    let raw = RawConfig {
        tokens: vec![RawToken {
            id: "   ".to_string(),
            sha256: Some(to_hex(&sha256(b"x"))),
        }],
        ..RawConfig::default()
    };
    assert_eq!(
        resolve(raw, &env_from(&[]), &no_files()).unwrap_err(),
        ConfigError::EmptyTokenId
    );
}

#[test]
fn rejects_duplicate_token_id() {
    let raw = RawConfig {
        tokens: vec![token("admin"), token("admin")],
        ..RawConfig::default()
    };
    assert_eq!(
        resolve(raw, &env_from(&[]), &no_files()).unwrap_err(),
        ConfigError::DuplicateTokenId("admin".to_string())
    );
}

#[test]
fn rejects_invalid_inline_hash() {
    let raw = RawConfig {
        tokens: vec![RawToken {
            id: "admin".to_string(),
            sha256: Some("not-hex".to_string()),
        }],
        ..RawConfig::default()
    };
    assert_eq!(
        resolve(raw, &env_from(&[]), &no_files()).unwrap_err(),
        ConfigError::InvalidHash("admin".to_string())
    );
}

#[test]
fn resolves_secret_from_environment() {
    let raw = RawConfig {
        tokens: vec![RawToken {
            id: "admin".to_string(),
            sha256: None,
        }],
        ..RawConfig::default()
    };
    let env = env_from(&[("GILFOYLE_TOKEN_ADMIN", "hunter2")]);
    let config = resolve(raw, &env, &no_files()).unwrap();
    assert_eq!(
        config.tokens,
        vec![TokenEntry {
            id: "admin".to_string(),
            hash: sha256(b"hunter2"),
        }]
    );
}

#[test]
fn resolves_secret_from_hash_file() {
    let raw = RawConfig {
        tokens: vec![RawToken {
            id: "admin".to_string(),
            sha256: None,
        }],
        ..RawConfig::default()
    };
    let expected = sha256(b"from-file");
    let mut files = HashMap::new();
    files.insert(
        PathBuf::from("/etc/gilfoyle/tokens/admin"),
        format!("{}\n", to_hex(&expected)),
    );
    let config = resolve(raw, &env_from(&[]), &files).unwrap();
    assert_eq!(config.tokens[0].hash, expected);
}

#[test]
fn rejects_invalid_hash_file() {
    let raw = RawConfig {
        tokens: vec![RawToken {
            id: "admin".to_string(),
            sha256: None,
        }],
        ..RawConfig::default()
    };
    let mut files = HashMap::new();
    files.insert(
        PathBuf::from("/etc/gilfoyle/tokens/admin"),
        "bad".to_string(),
    );
    assert_eq!(
        resolve(raw, &env_from(&[]), &files).unwrap_err(),
        ConfigError::InvalidHash("admin".to_string())
    );
}

#[test]
fn missing_secret_fails_closed() {
    let raw = RawConfig {
        tokens: vec![RawToken {
            id: "admin".to_string(),
            sha256: None,
        }],
        ..RawConfig::default()
    };
    assert_eq!(
        resolve(raw, &env_from(&[]), &no_files()).unwrap_err(),
        ConfigError::MissingSecret("admin".to_string())
    );
}

#[test]
fn custom_token_dir_is_used() {
    let raw = RawConfig {
        token_dir: Some("/custom".to_string()),
        tokens: vec![RawToken {
            id: "admin".to_string(),
            sha256: None,
        }],
        ..RawConfig::default()
    };
    let expected = sha256(b"custom");
    let mut files = HashMap::new();
    files.insert(PathBuf::from("/custom/admin"), to_hex(&expected));
    let config = resolve(raw, &env_from(&[]), &files).unwrap();
    assert_eq!(config.tokens[0].hash, expected);
}

#[test]
fn tools_allowlist_is_resolved() {
    let raw = RawConfig {
        tools: RawTools {
            enabled: Some(vec!["get_uptime".to_string()]),
        },
        tokens: vec![token("admin")],
        ..RawConfig::default()
    };
    let config = resolve(raw, &env_from(&[]), &no_files()).unwrap();
    let mut expected = BTreeSet::new();
    expected.insert("get_uptime".to_string());
    assert_eq!(config.enabled_tools, Some(expected));
}

#[test]
fn parses_a_toml_document() {
    let document = r#"
        bind = "127.0.0.1:9000"
        log_level = "warn"
        allowed_hosts = ["localhost"]
        allowed_origins = []

        [tools]
        enabled = ["get_uptime"]

        [[tokens]]
        id = "admin"
        sha256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    "#;
    let raw: RawConfig = toml::from_str(document).unwrap();
    let config = resolve(raw, &env_from(&[]), &no_files()).unwrap();
    assert_eq!(config.bind.to_string(), "127.0.0.1:9000");
    assert_eq!(config.log_level, "warn");
    assert_eq!(config.allowed_hosts, vec!["localhost".to_string()]);
    assert_eq!(config.tokens[0].hash, sha256(b""));
}

#[test]
fn rejects_unknown_toml_fields() {
    let document = "unknown = true";
    assert!(toml::from_str::<RawConfig>(document).is_err());
}
