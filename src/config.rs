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
    #[serde(default = "default_admin_user")]
    pub admin_user: String,
    #[serde(default = "default_admin_password")]
    pub admin_password: String,
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
            admin_user: default_admin_user(),
            admin_password: default_admin_password(),
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
fn default_admin_user() -> String {
    "admin".to_string()
}
fn default_admin_password() -> String {
    "admin".to_string()
}

pub fn load() -> Config {
    let mut config = match std::fs::read_to_string("paste.toml") {
        Ok(s) => match toml::from_str(&s) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("warning: failed to parse paste.toml: {e}; using defaults");
                Config::default()
            }
        },
        Err(_) => Config::default(),
    };

    normalize_prefix(&mut config);

    if config.admin_user == "admin" && config.admin_password == "admin" {
        eprintln!("warning: using default admin credentials (admin:admin); set admin_user and admin_password in paste.toml");
    }

    config
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
        assert_eq!(config.admin_user, "admin");
        assert_eq!(config.admin_password, "admin");
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
admin_user = "root"
admin_password = "pass123"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.bind, "0.0.0.0:8080");
        assert_eq!(config.prefix, "/paste");
        assert_eq!(config.max_ttl_secs, 120);
        assert_eq!(config.default_ttl_mins, 60);
        assert_eq!(config.max_size, 2048);
        assert_eq!(config.max_pastes, 10);
        assert_eq!(config.admin_user, "root");
        assert_eq!(config.admin_password, "pass123");
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
        assert_eq!(config.admin_user, "admin");
        assert_eq!(config.admin_password, "admin");
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
