use std::{env, error::Error, path::PathBuf};

#[cfg(windows)]
use eam_vault::VaultKeyStore;

#[cfg(windows)]
fn main() -> Result<(), Box<dyn Error>> {
    let vault_root = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: seed_installer_smoke_vault <empty-vault-root>")?;
    if vault_root.try_exists()? {
        return Err("installer smoke vault root must not already exist".into());
    }

    // The smoke root is ephemeral and deleted by the acceptance runner. Drop
    // both returned secrets without printing them; the installed application
    // must use the committed DPAPI CurrentUser wrapper to create self.db.
    let initialized = VaultKeyStore::initialize(&vault_root)?;
    let (vault_key, recovery_key) = initialized.into_parts();
    drop(vault_key);
    drop(recovery_key);
    Ok(())
}

#[cfg(not(windows))]
fn main() -> Result<(), Box<dyn Error>> {
    let _ = (env::args_os(), PathBuf::new());
    Err("installer smoke vault seeding requires Windows".into())
}
