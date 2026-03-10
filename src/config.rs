use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::secret_store;

#[derive(Serialize, Deserialize, Default)]
pub struct Config {
    pub token: Option<String>,
    pub chat_id: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum TokenPersistence {
    SecretService,
    PlaintextFallback,
}

// The path to the config file, e.g. ~/.config/tg/config.toml
pub fn config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".config")
        .join("tg")
        .join("config.toml")
}

impl Config {
    // Reads a config from the config file, or returns an empty config if the file doesn't exist.
    pub fn load() -> Self {
        Self::load_from_path(&config_path())
    }

    /// Saves the config to the config file, creating parent directories if necessary.
    pub(crate) fn save(&self) {
        let path = config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("failed to create config directory");
        }
        let contents = toml::to_string(self).expect("failed to serialize config");
        fs::write(&path, contents).expect("failed to write config");
    }

    /// Resolve the token from Secret Service first, then fallback to plaintext config.
    pub fn resolved_token(&self) -> Option<String> {
        let path = config_path();
        self.resolved_token_with(
            secret_store::load_token,
            |message| eprintln!("{message}"),
            &path,
        )
    }

    /// Save token to Secret Service first, then fallback to plaintext config on failure.
    pub(crate) fn persist_token(&mut self, token: &str) -> TokenPersistence {
        let path = config_path();
        self.persist_token_with(
            token,
            secret_store::save_token,
            |message| eprintln!("{message}"),
            &path,
        )
    }

    fn load_from_path(path: &std::path::Path) -> Self {
        if path.exists() {
            let contents = fs::read_to_string(path).unwrap_or_default();
            toml::from_str(&contents).unwrap_or_default()
        } else {
            Config::default()
        }
    }

    fn resolved_token_with<F, W>(
        &self,
        load_secret: F,
        mut warn: W,
        config_path: &std::path::Path,
    ) -> Option<String>
    where
        F: FnOnce() -> Result<Option<String>, secret_store::SecretStoreError>,
        W: FnMut(String),
    {
        match load_secret() {
            Ok(Some(token)) => Some(token),
            Ok(None) => self.token.clone(),
            Err(err) if secret_store::is_unavailable(&err) => {
                warn(format!(
                    "Warning: Secret Service API unavailable; falling back to plaintext token in {}",
                    config_path.display()
                ));
                self.token.clone()
            }
            Err(err) => {
                warn(format!(
                    "Warning: failed to read token from Secret Service ({err}); falling back to plaintext token in {}",
                    config_path.display()
                ));
                self.token.clone()
            }
        }
    }

    fn persist_token_with<F, W>(
        &mut self,
        token: &str,
        save_secret: F,
        mut warn: W,
        config_path: &std::path::Path,
    ) -> TokenPersistence
    where
        F: FnOnce(&str) -> Result<(), secret_store::SecretStoreError>,
        W: FnMut(String),
    {
        match save_secret(token) {
            Ok(()) => {
                self.token = None;
                TokenPersistence::SecretService
            }
            Err(err) if secret_store::is_unavailable(&err) => {
                warn(format!(
                    "Warning: Secret Service API unavailable; falling back to plaintext token in {}",
                    config_path.display()
                ));
                self.token = Some(token.to_string());
                TokenPersistence::PlaintextFallback
            }
            Err(err) => {
                warn(format!(
                    "Warning: failed to store token in Secret Service ({err}); falling back to plaintext token in {}",
                    config_path.display()
                ));
                self.token = Some(token.to_string());
                TokenPersistence::PlaintextFallback
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::secret_store::SecretStoreError;

    use super::{Config, TokenPersistence};

    #[test]
    fn load_from_path_returns_default_when_missing() {
        let missing = unique_tmp_path("missing-config");
        let config = Config::load_from_path(&missing);
        assert!(config.token.is_none());
        assert!(config.chat_id.is_none());
    }

    #[test]
    fn load_from_path_parses_toml_when_present() {
        let path = unique_tmp_path("present-config");
        std::fs::write(
            &path,
            "token = \"plaintext-token\"\nchat_id = 123456\n",
        )
        .expect("failed to write temporary config");

        let config = Config::load_from_path(&path);
        assert_eq!(config.token.as_deref(), Some("plaintext-token"));
        assert_eq!(config.chat_id, Some(123456));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn resolved_token_prefers_secret_service_value() {
        let config = Config {
            token: Some("plaintext-token".to_string()),
            chat_id: None,
        };

        let mut warnings = Vec::<String>::new();
        let token = config.resolved_token_with(
            || Ok(Some("secret-service-token".to_string())),
            |warning| warnings.push(warning),
            Path::new("/home/test/.config/tg/config.toml"),
        );

        assert_eq!(token.as_deref(), Some("secret-service-token"));
        assert!(warnings.is_empty());
    }

    #[test]
    fn resolved_token_falls_back_to_plaintext_when_secret_missing() {
        let config = Config {
            token: Some("plaintext-token".to_string()),
            chat_id: None,
        };

        let mut warnings = Vec::<String>::new();
        let token = config.resolved_token_with(
            || Ok(None),
            |warning| warnings.push(warning),
            Path::new("/home/test/.config/tg/config.toml"),
        );

        assert_eq!(token.as_deref(), Some("plaintext-token"));
        assert!(warnings.is_empty());
    }

    #[test]
    fn resolved_token_warns_and_falls_back_when_secret_service_unavailable() {
        let config = Config {
            token: Some("plaintext-token".to_string()),
            chat_id: None,
        };

        let mut warnings = Vec::<String>::new();
        let token = config.resolved_token_with(
            || Err(SecretStoreError::Unavailable("dbus unavailable".to_string())),
            |warning| warnings.push(warning),
            Path::new("/home/test/.config/tg/config.toml"),
        );

        assert_eq!(token.as_deref(), Some("plaintext-token"));
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].contains("Secret Service API unavailable")
                && warnings[0].contains("/home/test/.config/tg/config.toml")
        );
    }

    #[test]
    fn resolved_token_warns_and_falls_back_on_other_secret_errors() {
        let config = Config {
            token: Some("plaintext-token".to_string()),
            chat_id: None,
        };

        let mut warnings = Vec::<String>::new();
        let token = config.resolved_token_with(
            || Err(SecretStoreError::Backend(keyring::Error::NoEntry)),
            |warning| warnings.push(warning),
            Path::new("/home/test/.config/tg/config.toml"),
        );

        assert_eq!(token.as_deref(), Some("plaintext-token"));
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].contains("failed to read token from Secret Service")
                && warnings[0].contains("/home/test/.config/tg/config.toml")
        );
    }

    #[test]
    fn persist_token_uses_secret_service_when_available() {
        let mut config = Config::default();
        let mut warnings = Vec::<String>::new();

        let persistence = config.persist_token_with(
            "secret-service-token",
            |_| Ok(()),
            |warning| warnings.push(warning),
            Path::new("/home/test/.config/tg/config.toml"),
        );

        assert_eq!(persistence, TokenPersistence::SecretService);
        assert!(config.token.is_none());
        assert!(warnings.is_empty());
    }

    #[test]
    fn persist_token_warns_and_falls_back_when_secret_service_unavailable() {
        let mut config = Config::default();
        let mut warnings = Vec::<String>::new();

        let persistence = config.persist_token_with(
            "plaintext-token",
            |_| Err(SecretStoreError::Unavailable("dbus unavailable".to_string())),
            |warning| warnings.push(warning),
            Path::new("/home/test/.config/tg/config.toml"),
        );

        assert_eq!(persistence, TokenPersistence::PlaintextFallback);
        assert_eq!(config.token.as_deref(), Some("plaintext-token"));
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].contains("Secret Service API unavailable")
                && warnings[0].contains("/home/test/.config/tg/config.toml")
        );
    }

    #[test]
    fn persist_token_warns_and_falls_back_on_other_secret_errors() {
        let mut config = Config::default();
        let mut warnings = Vec::<String>::new();

        let persistence = config.persist_token_with(
            "plaintext-token",
            |_| Err(SecretStoreError::Backend(keyring::Error::NoEntry)),
            |warning| warnings.push(warning),
            Path::new("/home/test/.config/tg/config.toml"),
        );

        assert_eq!(persistence, TokenPersistence::PlaintextFallback);
        assert_eq!(config.token.as_deref(), Some("plaintext-token"));
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].contains("failed to store token in Secret Service")
                && warnings[0].contains("/home/test/.config/tg/config.toml")
        );
    }

    fn unique_tmp_path(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before UNIX_EPOCH")
            .as_nanos();
        std::env::temp_dir().join(format!("tg-cli-{prefix}-{nanos}.toml"))
    }
}
