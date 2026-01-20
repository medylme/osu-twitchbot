use std::fs;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::log_warn;
use crate::twitch::{DEFAULT_NP_COMMAND, DEFAULT_NP_FORMAT, DEFAULT_PP_COMMAND, DEFAULT_PP_FORMAT};

use super::{APP_NAME, VERSION};

#[derive(Debug, Error)]
pub enum PreferencesError {
    #[error("Failed to access preferences: {0}")]
    Confy(#[from] confy::ConfyError),
}

#[derive(Serialize, Deserialize)]
pub struct Config {
    version: String,
    auto_connect: bool,
    np_command: String,
    np_format: String,
    pp_command: String,
    pp_format: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: VERSION.to_string(),
            auto_connect: false,
            np_command: DEFAULT_NP_COMMAND.to_string(),
            np_format: DEFAULT_NP_FORMAT.to_string(),
            pp_command: DEFAULT_PP_COMMAND.to_string(),
            pp_format: DEFAULT_PP_FORMAT.to_string(),
        }
    }
}

pub struct PreferencesStore {
    config: Config,
}

impl PreferencesStore {
    fn load() -> Result<Self, PreferencesError> {
        let config: Config = confy::load(APP_NAME, None)?;
        Ok(Self { config })
    }

    pub fn load_or_default() -> Self {
        Self::load().unwrap_or_else(|e| {
            log_warn!(
                "prefs",
                "Failed to load preferences, resetting to defaults: {e}"
            );
            Self::backup_config();
            Self {
                config: Config::default(),
            }
        })
    }

    fn backup_config() {
        if let Ok(config_path) = confy::get_configuration_file_path(APP_NAME, None)
            && config_path.exists()
        {
            let backup_path = config_path.with_extension("toml.bak");
            if let Err(e) = fs::rename(&config_path, &backup_path) {
                log_warn!("prefs", "Failed to backup corrupted config: {e}");
            } else {
                log_warn!(
                    "prefs",
                    "Corrupted config backed up to: {}",
                    backup_path.display()
                );
            }
        }
    }

    pub fn save(&self) -> Result<(), PreferencesError> {
        confy::store(APP_NAME, None, &self.config)?;
        Ok(())
    }

    pub fn auto_connect(&self) -> bool {
        self.config.auto_connect
    }

    pub fn np_command(&self) -> &str {
        &self.config.np_command
    }

    pub fn np_format(&self) -> &str {
        &self.config.np_format
    }

    pub fn pp_command(&self) -> &str {
        &self.config.pp_command
    }

    pub fn pp_format(&self) -> &str {
        &self.config.pp_format
    }

    pub fn set_auto_connect(value: bool) -> Result<(), PreferencesError> {
        let mut store = Self::load_or_default();
        store.config.auto_connect = value;
        store.save()
    }

    pub fn set_np_command(value: String) -> Result<(), PreferencesError> {
        let mut store = Self::load_or_default();
        store.config.np_command = value;
        store.save()
    }

    pub fn set_np_format(value: String) -> Result<(), PreferencesError> {
        let mut store = Self::load_or_default();
        store.config.np_format = value;
        store.save()
    }

    pub fn set_pp_command(value: String) -> Result<(), PreferencesError> {
        let mut store = Self::load_or_default();
        store.config.pp_command = value;
        store.save()
    }

    pub fn set_pp_format(value: String) -> Result<(), PreferencesError> {
        let mut store = Self::load_or_default();
        store.config.pp_format = value;
        store.save()
    }
}
