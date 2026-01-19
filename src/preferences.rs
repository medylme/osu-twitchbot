use serde::{Deserialize, Serialize};
use thiserror::Error;

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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: VERSION.to_string(),
            auto_connect: false,
        }
    }
}

pub struct PreferencesStore {
    config: Config,
}

impl PreferencesStore {
    pub fn load() -> Result<Self, PreferencesError> {
        let config: Config = confy::load(APP_NAME, None)?;
        Ok(Self { config })
    }

    pub fn save(&self) -> Result<(), PreferencesError> {
        confy::store(APP_NAME, None, &self.config)?;
        Ok(())
    }

    pub fn auto_connect(&self) -> bool {
        self.config.auto_connect
    }

    pub fn set_auto_connect(value: bool) -> Result<(), PreferencesError> {
        let mut store = Self::load()?;
        store.config.auto_connect = value;
        store.save()
    }
}
