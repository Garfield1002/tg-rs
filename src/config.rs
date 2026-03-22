use std::{collections::HashMap, fs, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::secret_store;

#[derive(Serialize, Deserialize, Default, Clone)]
pub(crate) struct ProfileConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) chat_id: Option<i64>,
}

#[derive(Serialize, Deserialize, Default)]
pub(crate) struct ConfigFile {
    // Default profile fields at the top level for backward compatibility
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) chat_id: Option<i64>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub(crate) profiles: HashMap<String, ProfileConfig>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum TokenPersistence {
    SecretService,
    PlaintextFallback,
}

// The path to the config file, e.g. ~/.config/tg/config.toml
pub(crate) fn config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".config")
        .join("tg")
        .join("config.toml")
}

impl ConfigFile {
    pub(crate) fn load() -> Self {
        Self::load_from_path(&config_path())
    }

    pub(crate) fn save(&self) {
        let path = config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("failed to create config directory");
        }
        let contents = toml::to_string(self).expect("failed to serialize config");
        fs::write(&path, contents).expect("failed to write config");
    }

    pub(crate) fn get_profile(&self, profile: Option<&str>) -> ProfileConfig {
        match profile {
            None => ProfileConfig {
                token: self.token.clone(),
                chat_id: self.chat_id,
            },
            Some(name) => self.profiles.get(name).cloned().unwrap_or_default(),
        }
    }

    pub(crate) fn set_profile(&mut self, profile: Option<&str>, data: ProfileConfig) {
        match profile {
            None => {
                self.token = data.token;
                self.chat_id = data.chat_id;
            }
            Some(name) => {
                self.profiles.insert(name.to_string(), data);
            }
        }
    }

    pub(crate) fn delete_profile(&mut self, profile: Option<&str>) {
        match profile {
            None => {
                self.token = None;
                self.chat_id = None;
            }
            Some(name) => {
                self.profiles.remove(name);
            }
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.token.is_none() && self.chat_id.is_none() && self.profiles.is_empty()
    }

    fn load_from_path(path: &std::path::Path) -> Self {
        if path.exists() {
            let contents = fs::read_to_string(path).unwrap_or_default();
            toml::from_str(&contents).unwrap_or_default()
        } else {
            ConfigFile::default()
        }
    }
}

impl ProfileConfig {
    pub(crate) async fn resolved_token_for(&self, profile: Option<&str>) -> Option<String> {
        let path = config_path();
        let profile_owned = profile.map(|s| s.to_string());
        self.resolved_token_with(
            || secret_store::load_token_for(profile_owned),
            |message| eprintln!("{message}"),
            &path,
        )
        .await
    }

    pub(crate) async fn persist_token_for(
        &mut self,
        token: &str,
        profile: Option<&str>,
    ) -> TokenPersistence {
        let path = config_path();
        let profile_owned = profile.map(|s| s.to_string());
        self.persist_token_with(
            token,
            |t| secret_store::save_token_for(profile_owned, t),
            |message| eprintln!("{message}"),
            &path,
        )
        .await
    }

    async fn resolved_token_with<F, Fut, W>(
        &self,
        load_secret: F,
        mut warn: W,
        config_path: &std::path::Path,
    ) -> Option<String>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<Option<String>, secret_store::SecretStoreError>>,
        W: FnMut(String),
    {
        match load_secret().await {
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

    async fn persist_token_with<F, Fut, W>(
        &mut self,
        token: &str,
        save_secret: F,
        mut warn: W,
        config_path: &std::path::Path,
    ) -> TokenPersistence
    where
        F: FnOnce(String) -> Fut,
        Fut: std::future::Future<Output = Result<(), secret_store::SecretStoreError>>,
        W: FnMut(String),
    {
        match save_secret(token.to_string()).await {
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

    use super::{ConfigFile, ProfileConfig, TokenPersistence};

    #[test]
    fn load_from_path_returns_default_when_missing() {
        let missing = unique_tmp_path("missing-config");
        let config = ConfigFile::load_from_path(&missing);
        assert!(config.token.is_none());
        assert!(config.chat_id.is_none());
        assert!(config.profiles.is_empty());
    }

    #[test]
    fn load_from_path_parses_toml_when_present() {
        let path = unique_tmp_path("present-config");
        std::fs::write(&path, "token = \"plaintext-token\"\nchat_id = 123456\n")
            .expect("failed to write temporary config");

        let config = ConfigFile::load_from_path(&path);
        assert_eq!(config.token.as_deref(), Some("plaintext-token"));
        assert_eq!(config.chat_id, Some(123456));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn load_from_path_parses_named_profiles() {
        let path = unique_tmp_path("profiles-config");
        std::fs::write(
            &path,
            "chat_id = 111\n\n[profiles.work]\nchat_id = 222\n",
        )
        .expect("failed to write temporary config");

        let config = ConfigFile::load_from_path(&path);
        assert_eq!(config.chat_id, Some(111));
        assert_eq!(config.profiles["work"].chat_id, Some(222));

        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn resolved_token_prefers_secret_service_value() {
        let profile = ProfileConfig {
            token: Some("plaintext-token".to_string()),
            chat_id: None,
        };

        let mut warnings = Vec::<String>::new();
        let token = profile
            .resolved_token_with(
                || async { Ok(Some("secret-service-token".to_string())) },
                |warning| warnings.push(warning),
                Path::new("/home/test/.config/tg/config.toml"),
            )
            .await;

        assert_eq!(token.as_deref(), Some("secret-service-token"));
        assert!(warnings.is_empty());
    }

    #[tokio::test]
    async fn resolved_token_falls_back_to_plaintext_when_secret_missing() {
        let profile = ProfileConfig {
            token: Some("plaintext-token".to_string()),
            chat_id: None,
        };

        let mut warnings = Vec::<String>::new();
        let token = profile
            .resolved_token_with(
                || async { Ok(None) },
                |warning| warnings.push(warning),
                Path::new("/home/test/.config/tg/config.toml"),
            )
            .await;

        assert_eq!(token.as_deref(), Some("plaintext-token"));
        assert!(warnings.is_empty());
    }

    #[tokio::test]
    async fn resolved_token_warns_and_falls_back_when_secret_service_unavailable() {
        let profile = ProfileConfig {
            token: Some("plaintext-token".to_string()),
            chat_id: None,
        };

        let mut warnings = Vec::<String>::new();
        let token = profile
            .resolved_token_with(
                || async {
                    Err(SecretStoreError::Unavailable(
                        "dbus unavailable".to_string(),
                    ))
                },
                |warning| warnings.push(warning),
                Path::new("/home/test/.config/tg/config.toml"),
            )
            .await;

        assert_eq!(token.as_deref(), Some("plaintext-token"));
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].contains("Secret Service API unavailable")
                && warnings[0].contains("/home/test/.config/tg/config.toml")
        );
    }

    #[tokio::test]
    async fn resolved_token_warns_and_falls_back_on_other_secret_errors() {
        let profile = ProfileConfig {
            token: Some("plaintext-token".to_string()),
            chat_id: None,
        };

        let mut warnings = Vec::<String>::new();
        let token = profile
            .resolved_token_with(
                || async { Err(SecretStoreError::Backend(keyring::Error::NoEntry)) },
                |warning| warnings.push(warning),
                Path::new("/home/test/.config/tg/config.toml"),
            )
            .await;

        assert_eq!(token.as_deref(), Some("plaintext-token"));
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].contains("failed to read token from Secret Service")
                && warnings[0].contains("/home/test/.config/tg/config.toml")
        );
    }

    #[tokio::test]
    async fn persist_token_uses_secret_service_when_available() {
        let mut profile = ProfileConfig::default();
        let mut warnings = Vec::<String>::new();

        let persistence = profile
            .persist_token_with(
                "secret-service-token",
                |_| async { Ok(()) },
                |warning| warnings.push(warning),
                Path::new("/home/test/.config/tg/config.toml"),
            )
            .await;

        assert_eq!(persistence, TokenPersistence::SecretService);
        assert!(profile.token.is_none());
        assert!(warnings.is_empty());
    }

    #[tokio::test]
    async fn persist_token_warns_and_falls_back_when_secret_service_unavailable() {
        let mut profile = ProfileConfig::default();
        let mut warnings = Vec::<String>::new();

        let persistence = profile
            .persist_token_with(
                "plaintext-token",
                |_| async {
                    Err(SecretStoreError::Unavailable(
                        "dbus unavailable".to_string(),
                    ))
                },
                |warning| warnings.push(warning),
                Path::new("/home/test/.config/tg/config.toml"),
            )
            .await;

        assert_eq!(persistence, TokenPersistence::PlaintextFallback);
        assert_eq!(profile.token.as_deref(), Some("plaintext-token"));
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].contains("Secret Service API unavailable")
                && warnings[0].contains("/home/test/.config/tg/config.toml")
        );
    }

    #[tokio::test]
    async fn persist_token_warns_and_falls_back_on_other_secret_errors() {
        let mut profile = ProfileConfig::default();
        let mut warnings = Vec::<String>::new();

        let persistence = profile
            .persist_token_with(
                "plaintext-token",
                |_| async { Err(SecretStoreError::Backend(keyring::Error::NoEntry)) },
                |warning| warnings.push(warning),
                Path::new("/home/test/.config/tg/config.toml"),
            )
            .await;

        assert_eq!(persistence, TokenPersistence::PlaintextFallback);
        assert_eq!(profile.token.as_deref(), Some("plaintext-token"));
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
