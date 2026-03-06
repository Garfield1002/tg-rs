use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default)]
pub(crate) struct Config {
    pub(crate) token: Option<String>,
    pub(crate) chat_id: Option<i64>,
}

// The path to the config file, e.g. ~/.config/tg/config.toml
pub(crate) fn config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".config")
        .join("tg")
        .join("config.toml")
}

impl Config {
    // Reads a config from the config file, or returns an empty config if the file doesn't exist.
    pub(crate) fn load() -> Self {
        let path = config_path();
        if path.exists() {
            let contents = fs::read_to_string(&path).unwrap_or_default();
            toml::from_str(&contents).unwrap_or_default()
        } else {
            Config::default()
        }
    }

    /// Saves the config to the config file, creating parent directories if necessary.
    #[allow(dead_code)]
    pub(crate) fn save(&self) {
        let path = config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("failed to create config directory");
        }
        let contents = toml::to_string(self).expect("failed to serialize config");
        fs::write(&path, contents).expect("failed to write config");
    }
}
