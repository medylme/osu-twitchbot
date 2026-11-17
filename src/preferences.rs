use std::fs;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::log_warn;
use crate::twitch::ChatSettings;

use super::{APP_NAME, VERSION};

const CONFIG_NAME: &str = "settings";

pub fn open_config_dir() {
    match confy::get_configuration_file_path(APP_NAME, CONFIG_NAME) {
        Ok(path) => {
            let dir = path.parent().unwrap_or(&path);
            if let Err(e) = open::that(dir) {
                log_warn!("prefs", "failed to open config dir {}: {e}", dir.display());
            }
        }
        Err(e) => log_warn!("prefs", "could not resolve config dir: {e}"),
    }
}

#[derive(Debug, Error)]
pub enum PreferencesError {
    #[error("Failed to access preferences: {0}")]
    Confy(#[from] confy::ConfyError),
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    version: String,
    auto_connect: bool,
    np_command: String,
    np_format: String,
    pp_command: String,
    pp_format: String,
    telemetry_enabled: bool,
    minimize_to_tray: bool,
}

impl Default for Config {
    fn default() -> Self {
        let chat = ChatSettings::default();

        Self {
            version: VERSION.to_string(),
            auto_connect: false,
            np_command: chat.np_command,
            np_format: chat.np_format,
            pp_command: chat.pp_command,
            pp_format: chat.pp_format,
            telemetry_enabled: true,
            minimize_to_tray: true,
        }
    }
}

pub struct PreferencesStore {
    config: Config,
}

impl PreferencesStore {
    fn load() -> Result<Self, PreferencesError> {
        let config: Config = confy::load(APP_NAME, CONFIG_NAME)?;
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
        if let Ok(config_path) = confy::get_configuration_file_path(APP_NAME, CONFIG_NAME)
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
        confy::store(APP_NAME, CONFIG_NAME, &self.config)?;
        Ok(())
    }

    pub fn auto_connect(&self) -> bool {
        self.config.auto_connect
    }

    pub fn telemetry_enabled(&self) -> bool {
        self.config.telemetry_enabled
    }

    pub fn minimize_to_tray(&self) -> bool {
        self.config.minimize_to_tray
    }

    pub fn chat_settings(&self) -> ChatSettings {
        ChatSettings {
            np_command: self.config.np_command.clone(),
            np_format: self.config.np_format.clone(),
            pp_command: self.config.pp_command.clone(),
            pp_format: self.config.pp_format.clone(),
        }
    }

    pub fn set_auto_connect(&mut self, value: bool) -> Result<(), PreferencesError> {
        self.config.auto_connect = value;
        self.save()
    }

    pub fn set_minimize_to_tray(&mut self, value: bool) -> Result<(), PreferencesError> {
        self.config.minimize_to_tray = value;
        self.save()
    }

    pub fn set_chat_settings(&mut self, chat: &ChatSettings) -> Result<(), PreferencesError> {
        self.config.np_command.clone_from(&chat.np_command);
        self.config.np_format.clone_from(&chat.np_format);
        self.config.pp_command.clone_from(&chat.pp_command);
        self.config.pp_format.clone_from(&chat.pp_format);
        self.save()
    }
}
