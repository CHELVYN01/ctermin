//! Server passwords in Windows Credential Manager, through the keyring crate.
//!
//! Error messages leave out keyring details on purpose, so there is no chance of
//! credential data showing up in the UI.

use anyhow::{anyhow, Result};
use keyring::{Entry, Error};

use super::model::ServerId;

const SERVICE: &str = "ctermin";
const API_KEY_ACCOUNT: &str = "anthropic-api-key";

fn entry(id: ServerId) -> Result<Entry> {
    Entry::new(SERVICE, &format!("server-{}", id.0))
        .map_err(|_| anyhow!("Could not open Windows Credential Manager"))
}

pub fn save_password(id: ServerId, password: &str) -> Result<()> {
    entry(id)?
        .set_password(password)
        .map_err(|_| anyhow!("Could not save the password to Credential Manager"))
}

/// `None` if no password is saved yet.
pub fn load_password(id: ServerId) -> Result<Option<String>> {
    match entry(id)?.get_password() {
        Ok(password) => Ok(Some(password)),
        Err(Error::NoEntry) => Ok(None),
        Err(_) => Err(anyhow!(
            "Could not read the password from Credential Manager"
        )),
    }
}

pub fn has_password(id: ServerId) -> bool {
    matches!(load_password(id), Ok(Some(_)))
}

pub fn delete_password(id: ServerId) -> Result<()> {
    match entry(id)?.delete_credential() {
        Ok(()) | Err(Error::NoEntry) => Ok(()),
        Err(_) => Err(anyhow!(
            "Could not delete the password from Credential Manager"
        )),
    }
}

fn api_key_entry() -> Result<Entry> {
    Entry::new(SERVICE, API_KEY_ACCOUNT)
        .map_err(|_| anyhow!("Could not open Windows Credential Manager"))
}

pub fn save_api_key(key: &str) -> Result<()> {
    api_key_entry()?
        .set_password(key)
        .map_err(|_| anyhow!("Could not save the API key to Credential Manager"))
}

/// `None` if no API key is saved yet.
pub fn load_api_key() -> Result<Option<String>> {
    match api_key_entry()?.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(Error::NoEntry) => Ok(None),
        Err(_) => Err(anyhow!(
            "Could not read the API key from Credential Manager"
        )),
    }
}

pub fn has_api_key() -> bool {
    matches!(load_api_key(), Ok(Some(_)))
}

pub fn delete_api_key() -> Result<()> {
    match api_key_entry()?.delete_credential() {
        Ok(()) | Err(Error::NoEntry) => Ok(()),
        Err(_) => Err(anyhow!(
            "Could not delete the API key from Credential Manager"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Uses an id far outside the normal range, and always cleans up.
    #[test]
    fn save_load_delete_in_real_credential_manager() {
        let id = ServerId(u64::MAX - u64::from(std::process::id()));
        assert_eq!(load_password(id).expect("readable"), None);
        assert!(!has_password(id));

        save_password(id, "temporary-test-value").expect("saved");
        let loaded = load_password(id);
        let cleaned = delete_password(id);

        assert_eq!(
            loaded.expect("readable").as_deref(),
            Some("temporary-test-value")
        );
        cleaned.expect("deletable");
        assert_eq!(load_password(id).expect("readable"), None);
    }

    #[test]
    fn deleting_a_missing_entry_is_not_an_error() {
        let id = ServerId(u64::MAX - 1 - u64::from(std::process::id()));
        delete_password(id).expect("no error");
    }
}
