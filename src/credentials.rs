use keyring::Entry;
use thiserror::Error;

const SERVICE_NAME: &str = "dyl-osu-twitchbot";
const TOKEN_KEY: &str = "twitch-access-token";

#[derive(Debug, Error)]
pub enum CredentialError {
    #[error("Failed to access credential store: {0}")]
    Keyring(#[from] keyring::Error),
    #[error("No token found")]
    NotFound,
}

pub struct CredentialStore;

impl CredentialStore {
    fn entry() -> Result<Entry, CredentialError> {
        Ok(Entry::new(SERVICE_NAME, TOKEN_KEY)?)
    }

    pub fn save_token(token: &str) -> Result<(), CredentialError> {
        let entry = Self::entry()?;
        entry.set_password(token)?;
        Ok(())
    }

    pub fn load_token() -> Result<String, CredentialError> {
        let entry = Self::entry()?;
        match entry.get_password() {
            Ok(token) => Ok(token),
            Err(keyring::Error::NoEntry) => Err(CredentialError::NotFound),
            Err(e) => Err(CredentialError::Keyring(e)),
        }
    }

    pub fn delete_token() -> Result<(), CredentialError> {
        let entry = Self::entry()?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(CredentialError::Keyring(e)),
        }
    }
}
