//! SQLCipher-backed persistence inside the trusted local Core boundary.

mod backup;
mod crypto;
#[cfg(windows)]
mod dpapi;
mod error;
mod key_store;
mod object_store;
mod repository;
mod schema;

pub use backup::{BackupReceipt, RestoreReceipt, VaultBackup};
pub use crypto::VaultKey;
pub use error::VaultError;
pub use key_store::{InitializedVault, PreparedVault, RecoveryKey, VaultKeyStore};
pub use repository::VaultRepository;

#[cfg(test)]
mod test_support {
    use std::sync::{Mutex, MutexGuard};

    static SQLCIPHER_TEST_LOCK: Mutex<()> = Mutex::new(());

    pub(crate) fn sqlcipher_test_lock() -> MutexGuard<'static, ()> {
        SQLCIPHER_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}
