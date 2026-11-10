use gilfoyle_core::PackageOperation;
use gilfoyle_server::config::{
    ConfigError, DEFAULT_DOCKER_ENDPOINT, DEFAULT_HOST_KEY_PATH, DEFAULT_LOG_LEVEL,
    DEFAULT_MAX_CHANNELS_PER_CONNECTION, DEFAULT_MAX_CONNECTIONS, RawConfig, known_tools,
    parse_bind, resolve_config, resolve_docker_endpoint, resolve_enabled,
};
use gilfoyle_ssh::russh::keys::{Algorithm, PrivateKey};

fn public_key() -> String {
    PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)
        .unwrap()
        .public_key()
        .to_openssh()
        .unwrap()
}

fn raw(toml: &str) -> RawConfig {
    toml::from_str(toml).unwrap()
}

fn resolve(toml: &str) -> Result<gilfoyle_server::config::Config, ConfigError> {
    resolve_config(raw(toml), &|_| None)
}

const KEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIexample";

#[test]
fn requires_a_bind_address() {
    let result = resolve(&format!(
        "[[keys]]\nid = \"a\"\npublic_key = \"{}\"\n",
        public_key()
    ));
    assert_eq!(result.unwrap_err(), ConfigError::MissingBind);
}

#[test]
fn rejects_a_malformed_bind() {
    let result = resolve(&format!(
        "bind = \"not-an-address\"\n[[keys]]\nid = \"a\"\npublic_key = \"{}\"\n",
        public_key()
    ));
    assert_eq!(
        result.unwrap_err(),
        ConfigError::InvalidBind("not-an-address".to_string())
    );
}

#[test]
fn requires_at_least_one_key() {
    let result = resolve("bind = \"127.0.0.1:2222\"\n");
    assert_eq!(result.unwrap_err(), ConfigError::NoKeys);
}

#[test]
fn rejects_duplicate_key_ids() {
    let key = public_key();
    let result = resolve(&format!(
        "bind = \"127.0.0.1:2222\"\n[[keys]]\nid = \"dup\"\npublic_key = \"{key}\"\n[[keys]]\nid = \"dup\"\npublic_key = \"{key}\"\n"
    ));
    assert_eq!(
        result.unwrap_err(),
        ConfigError::DuplicateKeyId("dup".to_string())
    );
}

#[test]
fn rejects_an_invalid_public_key() {
    let result =
        resolve("bind = \"127.0.0.1:2222\"\n[[keys]]\nid = \"a\"\npublic_key = \"not a key\"\n");
    assert!(matches!(result, Err(ConfigError::InvalidPublicKey(_, _))));
}

#[test]
fn rejects_an_unknown_granted_tool() {
    let key = public_key();
    let result = resolve(&format!(
        "bind = \"127.0.0.1:2222\"\n[[keys]]\nid = \"a\"\npublic_key = \"{key}\"\ntools = [\"nope\"]\n"
    ));
    assert_eq!(
        result.unwrap_err(),
        ConfigError::UnknownTool("a".to_string(), "nope".to_string())
    );
}

#[test]
fn rejects_a_legacy_token_config() {
    let key = public_key();
    let result = resolve(&format!(
        "bind = \"127.0.0.1:2222\"\n[[keys]]\nid = \"a\"\npublic_key = \"{key}\"\n[[tokens]]\nid = \"admin\"\n"
    ));
    assert_eq!(
        result.unwrap_err(),
        ConfigError::LegacyField("[[tokens]]".to_string())
    );
}

#[test]
fn rejects_a_legacy_allow_public() {
    let key = public_key();
    let result = resolve(&format!(
        "bind = \"127.0.0.1:2222\"\nallow_public = true\n[[keys]]\nid = \"a\"\npublic_key = \"{key}\"\n"
    ));
    assert_eq!(
        result.unwrap_err(),
        ConfigError::LegacyField("allow_public".to_string())
    );
}

#[test]
fn rejects_an_unknown_field() {
    let parsed = toml::from_str::<RawConfig>("bind = \"127.0.0.1:2222\"\nmystery = 1\n");
    assert!(parsed.is_err());
}

#[test]
fn applies_sensible_defaults() {
    let config = resolve(&format!(
        "bind = \"10.0.0.1:2222\"\n[[keys]]\nid = \"a\"\npublic_key = \"{}\"\n",
        public_key()
    ))
    .unwrap();
    assert_eq!(config.log_level, DEFAULT_LOG_LEVEL);
    assert_eq!(config.max_connections, DEFAULT_MAX_CONNECTIONS);
    assert_eq!(
        config.max_channels_per_connection,
        DEFAULT_MAX_CHANNELS_PER_CONNECTION
    );
    assert_eq!(
        config.host_key,
        std::path::PathBuf::from(DEFAULT_HOST_KEY_PATH)
    );
    assert_eq!(config.enabled_tools, known_tools());
    assert!(config.allowlist.is_empty());
}

#[test]
fn a_grant_is_recorded_on_the_key() {
    let key = public_key();
    let config = resolve(&format!(
        "bind = \"127.0.0.1:2222\"\n[[keys]]\nid = \"reader\"\npublic_key = \"{key}\"\ntools = [\"get_uptime\"]\n"
    ))
    .unwrap();
    let entry = config
        .keys
        .iter()
        .find(|entry| entry.id == "reader")
        .unwrap();
    assert_eq!(
        entry
            .tools
            .as_ref()
            .unwrap()
            .iter()
            .cloned()
            .collect::<Vec<_>>(),
        vec!["get_uptime".to_string()]
    );
}

#[test]
fn env_overrides_bind_and_host_key() {
    let raw = raw(&format!(
        "bind = \"127.0.0.1:2222\"\n[[keys]]\nid = \"a\"\npublic_key = \"{}\"\n",
        public_key()
    ));
    let config = resolve_config(raw, &|key| match key {
        "GILFOYLE_BIND" => Some("0.0.0.0:9999".to_string()),
        "GILFOYLE_HOST_KEY" => Some("/tmp/other_key".to_string()),
        _ => None,
    })
    .unwrap();
    assert_eq!(config.bind, parse_bind("0.0.0.0:9999").unwrap());
    assert_eq!(config.host_key, std::path::PathBuf::from("/tmp/other_key"));
}

#[test]
fn rejects_an_invalid_managed_unit() {
    let key = public_key();
    let result = resolve(&format!(
        "bind = \"127.0.0.1:2222\"\n[systemd]\nmanage_units = [\"bad name!\"]\n[[keys]]\nid = \"a\"\npublic_key = \"{key}\"\n"
    ));
    assert_eq!(
        result.unwrap_err(),
        ConfigError::InvalidManagedUnit("bad name!".to_string())
    );
}

#[test]
fn narrows_the_global_tool_set() {
    let enabled = resolve_enabled(Some(vec!["get_uptime".to_string()])).unwrap();
    assert_eq!(enabled.len(), 1);
    assert!(enabled.contains("get_uptime"));
}

#[test]
fn rejects_an_unknown_enabled_tool() {
    assert_eq!(
        resolve_enabled(Some(vec!["nope".to_string()])).unwrap_err(),
        ConfigError::UnknownEnabledTool("nope".to_string())
    );
}

#[test]
fn parses_bind_trimmed() {
    assert_eq!(parse_bind("  127.0.0.1:22  ").unwrap().port(), 22);
}

// Keep `KEY` referenced so the constant documents the OpenSSH line shape; unused otherwise.
#[test]
fn key_constant_is_parseable_shape() {
    assert!(KEY.starts_with("ssh-ed25519 "));
}

#[test]
fn resolves_the_package_allowlist_and_options() {
    let config = resolve(&format!(
        "bind = \"127.0.0.1:2222\"\n\
         [packagekit]\n\
         allow = [\"refresh\", \"upgrade\"]\n\
         allow_transitive_changes = true\n\
         poll_timeout_seconds = 60\n\
         default_limit = 50\n\
         [[keys]]\n\
         id = \"a\"\n\
         public_key = \"{}\"\n",
        public_key()
    ))
    .unwrap();
    assert!(config.package_allowlist.contains(PackageOperation::Refresh));
    assert!(config.package_allowlist.contains(PackageOperation::Upgrade));
    assert!(config.allow_transitive_changes);
    assert_eq!(config.package_poll_timeout_seconds, Some(60));
    assert_eq!(config.package_default_limit, Some(50));
}

#[test]
fn rejects_an_unknown_package_operation() {
    let result = resolve(&format!(
        "bind = \"127.0.0.1:2222\"\n\
         [packagekit]\n\
         allow = [\"install\"]\n\
         [[keys]]\n\
         id = \"a\"\n\
         public_key = \"{}\"\n",
        public_key()
    ));
    assert_eq!(
        result.unwrap_err(),
        ConfigError::InvalidPackageOperation("install".to_string())
    );
}

#[test]
fn empty_package_allowlist_is_default_deny() {
    let config = resolve(&format!(
        "bind = \"127.0.0.1:2222\"\n[[keys]]\nid = \"a\"\npublic_key = \"{}\"\n",
        public_key()
    ))
    .unwrap();
    assert!(config.package_allowlist.is_empty());
    assert!(!config.allow_transitive_changes);
    assert_eq!(config.package_poll_timeout_seconds, None);
}

#[test]
fn known_tools_include_the_package_tools() {
    let tools = known_tools();
    for tool in [
        "list_packages",
        "list_updates",
        "preview_upgrade",
        "refresh_package_cache",
        "upgrade_packages",
    ] {
        assert!(tools.contains(tool), "{tool}");
    }
}

#[test]
fn known_tools_include_the_container_tools() {
    let tools = known_tools();
    for tool in [
        "list_containers",
        "container_status",
        "start_container",
        "stop_container",
        "restart_container",
    ] {
        assert!(tools.contains(tool), "{tool}");
    }
}

#[test]
fn docker_defaults_are_fail_closed() {
    let config = resolve(&format!(
        "bind = \"127.0.0.1:2222\"\n[[keys]]\nid = \"a\"\npublic_key = \"{}\"\n",
        public_key()
    ))
    .unwrap();
    assert_eq!(
        config.docker_endpoint,
        std::path::PathBuf::from(DEFAULT_DOCKER_ENDPOINT)
    );
    assert!(config.container_allowlist.is_empty());
    assert_eq!(config.docker_call_timeout_seconds, None);
    assert_eq!(config.docker_stop_timeout_seconds, None);
    assert_eq!(config.docker_default_limit, None);
}

#[test]
fn resolves_the_container_allowlist_and_options() {
    let config = resolve(&format!(
        "bind = \"127.0.0.1:2222\"\n\
         [docker]\n\
         endpoint = \"/run/docker.sock\"\n\
         manage_containers = [\"web\", \"re:^worker-[0-9]+$\"]\n\
         call_timeout_seconds = 5\n\
         stop_timeout_seconds = 20\n\
         default_limit = 50\n\
         [[keys]]\n\
         id = \"a\"\n\
         public_key = \"{}\"\n",
        public_key()
    ))
    .unwrap();
    assert_eq!(
        config.docker_endpoint,
        std::path::PathBuf::from("/run/docker.sock")
    );
    assert!(config.container_allowlist.contains("web"));
    assert!(config.container_allowlist.contains("worker-1"));
    assert!(!config.container_allowlist.contains("worker-x"));
    assert_eq!(config.docker_call_timeout_seconds, Some(5));
    assert_eq!(config.docker_stop_timeout_seconds, Some(20));
    assert_eq!(config.docker_default_limit, Some(50));
}

#[test]
fn rejects_a_non_socket_docker_endpoint() {
    for endpoint in ["tcp://1.2.3.4:2375", "http://localhost", "relative/path"] {
        let result = resolve(&format!(
            "bind = \"127.0.0.1:2222\"\n[docker]\nendpoint = \"{endpoint}\"\n[[keys]]\nid = \"a\"\npublic_key = \"{}\"\n",
            public_key()
        ));
        assert_eq!(
            result.unwrap_err(),
            ConfigError::InvalidDockerEndpoint(endpoint.to_string())
        );
    }
}

#[test]
fn rejects_invalid_container_entries() {
    for entry in ["bad name!", "re:(", "re:"] {
        let result = resolve(&format!(
            "bind = \"127.0.0.1:2222\"\n[docker]\nmanage_containers = [\"{entry}\"]\n[[keys]]\nid = \"a\"\npublic_key = \"{}\"\n",
            public_key()
        ));
        assert_eq!(
            result.unwrap_err(),
            ConfigError::InvalidContainerEntry(entry.to_string())
        );
    }
}

#[test]
fn resolve_docker_endpoint_trims_and_defaults() {
    assert_eq!(
        resolve_docker_endpoint(None).unwrap(),
        std::path::PathBuf::from(DEFAULT_DOCKER_ENDPOINT)
    );
    assert_eq!(
        resolve_docker_endpoint(Some("  /run/docker.sock  ".to_string())).unwrap(),
        std::path::PathBuf::from("/run/docker.sock")
    );
    assert!(resolve_docker_endpoint(Some(String::new())).is_err());
}
