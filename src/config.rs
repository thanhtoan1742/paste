use serde::Deserialize;

#[derive(Deserialize, Clone)]
pub struct Config {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_ttl_secs")]
    pub ttl_secs: u64,
    #[serde(default = "default_default_ttl_secs")]
    pub default_ttl_secs: u64,
    #[serde(default = "default_max_size")]
    pub max_size: usize,
    #[serde(default = "default_max_pastes")]
    pub max_pastes: usize,
    #[serde(default = "default_admin_user")]
    pub admin_user: String,
    #[serde(default = "default_admin_pass")]
    pub admin_pass: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            ttl_secs: default_ttl_secs(),
            default_ttl_secs: default_default_ttl_secs(),
            max_size: default_max_size(),
            max_pastes: default_max_pastes(),
            admin_user: default_admin_user(),
            admin_pass: default_admin_pass(),
        }
    }
}

fn default_bind() -> String {
    "0.0.0.0:3000".to_string()
}
fn default_ttl_secs() -> u64 {
    86400
}
fn default_default_ttl_secs() -> u64 {
    900
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
fn default_admin_pass() -> String {
    "admin".to_string()
}

pub fn load() -> Config {
    std::fs::read_to_string("paste.toml")
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults() {
        let config = Config::default();
        assert_eq!(config.bind, "0.0.0.0:3000");
        assert_eq!(config.ttl_secs, 86400);
        assert_eq!(config.default_ttl_secs, 900);
        assert_eq!(config.max_size, 8_388_608);
        assert_eq!(config.max_pastes, 512);
        assert_eq!(config.admin_user, "admin");
        assert_eq!(config.admin_pass, "admin");
    }

    #[test]
    fn config_from_toml() {
        let toml = r#"
bind = "0.0.0.0:8080"
ttl_secs = 120
default_ttl_secs = 60
max_size = 2048
max_pastes = 10
admin_user = "root"
admin_pass = "pass123"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.bind, "0.0.0.0:8080");
        assert_eq!(config.ttl_secs, 120);
        assert_eq!(config.default_ttl_secs, 60);
        assert_eq!(config.max_size, 2048);
        assert_eq!(config.max_pastes, 10);
        assert_eq!(config.admin_user, "root");
        assert_eq!(config.admin_pass, "pass123");
    }

    #[test]
    fn config_partial_toml_uses_defaults() {
        let toml = r#"bind = "0.0.0.0:9999""#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.bind, "0.0.0.0:9999");
        assert_eq!(config.ttl_secs, 86400);
        assert_eq!(config.default_ttl_secs, 900);
        assert_eq!(config.max_size, 8_388_608);
        assert_eq!(config.max_pastes, 512);
        assert_eq!(config.admin_user, "admin");
        assert_eq!(config.admin_pass, "admin");
    }
}
