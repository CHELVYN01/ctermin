//! Password server di Windows Credential Manager, lewat crate keyring.
//!
//! Pesan error sengaja tidak menyertakan detail dari keyring, supaya tidak ada kemungkinan
//! informasi kredensial ikut tampil di UI.

use anyhow::{anyhow, Result};
use keyring::{Entry, Error};

use super::model::ServerId;

const SERVICE: &str = "ctermin";

fn entry(id: ServerId) -> Result<Entry> {
    Entry::new(SERVICE, &format!("server-{}", id.0))
        .map_err(|_| anyhow!("Credential Manager Windows tidak bisa dibuka"))
}

pub fn save_password(id: ServerId, password: &str) -> Result<()> {
    entry(id)?
        .set_password(password)
        .map_err(|_| anyhow!("Password gagal disimpan ke Credential Manager"))
}

/// `None` jika belum ada password tersimpan.
pub fn load_password(id: ServerId) -> Result<Option<String>> {
    match entry(id)?.get_password() {
        Ok(password) => Ok(Some(password)),
        Err(Error::NoEntry) => Ok(None),
        Err(_) => Err(anyhow!("Password gagal dibaca dari Credential Manager")),
    }
}

pub fn has_password(id: ServerId) -> bool {
    matches!(load_password(id), Ok(Some(_)))
}

pub fn delete_password(id: ServerId) -> Result<()> {
    match entry(id)?.delete_credential() {
        Ok(()) | Err(Error::NoEntry) => Ok(()),
        Err(_) => Err(anyhow!("Password gagal dihapus dari Credential Manager")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Memakai id jauh di luar jangkauan pemakaian normal, dan selalu dibersihkan.
    #[test]
    fn simpan_baca_hapus_di_credential_manager_asli() {
        let id = ServerId(u64::MAX - u64::from(std::process::id()));
        assert_eq!(load_password(id).expect("bisa dibaca"), None);
        assert!(!has_password(id));

        save_password(id, "nilai-uji-sementara").expect("bisa disimpan");
        let loaded = load_password(id);
        let cleaned = delete_password(id);

        assert_eq!(
            loaded.expect("bisa dibaca").as_deref(),
            Some("nilai-uji-sementara")
        );
        cleaned.expect("bisa dihapus");
        assert_eq!(load_password(id).expect("bisa dibaca"), None);
    }

    #[test]
    fn menghapus_yang_tidak_ada_bukan_error() {
        let id = ServerId(u64::MAX - 1 - u64::from(std::process::id()));
        delete_password(id).expect("tidak error");
    }
}
