use serde::Deserialize;

#[derive(Deserialize, Clone)]
pub struct Config {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_prefix")]
    pub prefix: String,
    #[serde(default = "default_max_ttl_secs")]
    pub max_ttl_secs: u64,
    #[serde(default = "default_default_ttl_mins")]
    pub default_ttl_mins: u64,
    #[serde(default = "default_max_size")]
    pub max_size: usize,
    #[serde(default = "default_max_pastes")]
    pub max_pastes: usize,
    #[serde(default = "default_lockdown")]
    pub lockdown: bool,
    #[serde(default = "default_user")]
    pub user: String,
    #[serde(default = "default_password")]
    pub password: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            prefix: default_prefix(),
            max_ttl_secs: default_max_ttl_secs(),
            default_ttl_mins: default_default_ttl_mins(),
            max_size: default_max_size(),
            max_pastes: default_max_pastes(),
            lockdown: default_lockdown(),
            user: default_user(),
            password: default_password(),
        }
    }
}

fn default_bind() -> String {
    "0.0.0.0:3000".to_string()
}
fn default_prefix() -> String {
    String::new()
}
fn default_max_ttl_secs() -> u64 {
    86400
}
fn default_default_ttl_mins() -> u64 {
    15
}
fn default_max_size() -> usize {
    8_388_608
}
fn default_max_pastes() -> usize {
    512
}
fn default_lockdown() -> bool {
    false
}
fn default_user() -> String {
    "user".to_string()
}
fn default_password() -> String {
    "pass".to_string()
}

pub fn load(path: &str) -> Result<Config, String> {
    let s = std::fs::read_to_string(path)
        .map_err(|e| format!("config file not found: {path}: {e}"))?;
    let mut config: Config = toml::from_str(&s)
        .map_err(|e| format!("failed to parse config {path}: {e}"))?;

    normalize_prefix(&mut config);

    if config.user == "user" && config.password == "pass" {
        eprintln!("warning: using default credentials (user:pass); set user and password in paste.toml");
    }

    Ok(config)
}

fn normalize_prefix(config: &mut Config) {
    if config.prefix.is_empty() {
        return;
    }
    if !config.prefix.starts_with('/') {
        config.prefix.insert(0, '/');
    }
    while config.prefix.len() > 1 && config.prefix.ends_with('/') {
        config.prefix.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults() {
        let config = Config::default();
        assert_eq!(config.bind, "0.0.0.0:3000");
        assert_eq!(config.prefix, "");
        assert_eq!(config.max_ttl_secs, 86400);
        assert_eq!(config.default_ttl_mins, 15);
        assert_eq!(config.max_size, 8_388_608);
        assert_eq!(config.max_pastes, 512);
        assert!(!config.lockdown);
        assert_eq!(config.user, "user");
        assert_eq!(config.password, "pass");
    }

    #[test]
    fn config_from_toml() {
        let toml = r#"
bind = "0.0.0.0:8080"
prefix = "/paste"
max_ttl_secs = 120
default_ttl_mins = 60
max_size = 2048
max_pastes = 10
user = "root"
password = "pass123"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.bind, "0.0.0.0:8080");
        assert_eq!(config.prefix, "/paste");
        assert_eq!(config.max_ttl_secs, 120);
        assert_eq!(config.default_ttl_mins, 60);
        assert_eq!(config.max_size, 2048);
        assert_eq!(config.max_pastes, 10);
        assert!(!config.lockdown);
        assert_eq!(config.user, "root");
        assert_eq!(config.password, "pass123");
    }

    #[test]
    fn config_partial_toml_uses_defaults() {
        let toml = r#"bind = "0.0.0.0:9999""#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.bind, "0.0.0.0:9999");
        assert_eq!(config.prefix, "");
        assert_eq!(config.max_ttl_secs, 86400);
        assert_eq!(config.default_ttl_mins, 15);
        assert_eq!(config.max_size, 8_388_608);
        assert_eq!(config.max_pastes, 512);
        assert_eq!(config.user, "user");
        assert_eq!(config.password, "pass");
    }

    #[test]
    fn prefix_normalization() {
        let mut config = Config {
            prefix: "paste".to_string(),
            ..Config::default()
        };
        normalize_prefix(&mut config);
        assert_eq!(config.prefix, "/paste");

        let mut config = Config {
            prefix: "/paste/".to_string(),
            ..Config::default()
        };
        normalize_prefix(&mut config);
        assert_eq!(config.prefix, "/paste");

        let mut config = Config {
            prefix: "//paste//".to_string(),
            ..Config::default()
        };
        normalize_prefix(&mut config);
        assert_eq!(config.prefix, "//paste");

        let mut config = Config {
            prefix: String::new(),
            ..Config::default()
        };
        normalize_prefix(&mut config);
        assert_eq!(config.prefix, "");
    }
}
