#![cfg(windows)]

use std::fs;

use eam_vault::{RecoveryKey, VaultError, VaultKeyStore, VaultRepository};
use tempfile::tempdir;

const META_FILE: &str = "bundle.meta";
const META_MAGIC: &[u8; 8] = b"EAMKEYS\0";
const LOCAL_LENGTH_OFFSET: usize = 8 + 2 + 32 + 24 + 48;
const LOCAL_BYTES_OFFSET: usize = LOCAL_LENGTH_OFFSET + 4;
const RECOVERY_CIPHERTEXT_OFFSET: usize = 8 + 2 + 32 + 24;

#[test]
fn same_user_and_recovery_paths_unlock_the_same_vault_without_a_dpapi_copy() {
    let directory = tempdir().unwrap();
    let initialized = VaultKeyStore::initialize(directory.path()).unwrap();
    let (initial_key, recovery_key) = initialized.into_parts();

    let repository = VaultRepository::open(directory.path(), initial_key).unwrap();
    assert_eq!(repository.schema_version().unwrap(), 21);
    repository.close().unwrap();

    let local_key = VaultKeyStore::unlock_local(directory.path()).unwrap();
    VaultRepository::open(directory.path(), local_key)
        .unwrap()
        .close()
        .unwrap();

    remove_local_copy(directory.path());
    assert!(matches!(
        VaultKeyStore::unlock_local(directory.path()),
        Err(VaultError::UnlockFailed)
    ));

    let recovered_key = VaultKeyStore::unlock_recovery(directory.path(), &recovery_key).unwrap();
    VaultRepository::open(directory.path(), recovered_key)
        .unwrap()
        .close()
        .unwrap();
}

#[test]
fn wrong_key_and_tampered_recovery_ciphertext_share_one_failure_surface() {
    let first = tempdir().unwrap();
    let second = tempdir().unwrap();
    let first_initialized = VaultKeyStore::initialize(first.path()).unwrap();
    let second_initialized = VaultKeyStore::initialize(second.path()).unwrap();
    let (_, first_recovery_key) = first_initialized.into_parts();
    let (_, wrong_recovery_key) = second_initialized.into_parts();

    let wrong_key_error = unlock_error(VaultKeyStore::unlock_recovery(
        first.path(),
        &wrong_recovery_key,
    ));

    let metadata_path = first.path().join(META_FILE);
    let mut metadata = fs::read(&metadata_path).unwrap();
    metadata[RECOVERY_CIPHERTEXT_OFFSET] ^= 0x01;
    fs::write(&metadata_path, metadata).unwrap();
    let tamper_error = unlock_error(VaultKeyStore::unlock_recovery(
        first.path(),
        &first_recovery_key,
    ));

    assert!(matches!(wrong_key_error, VaultError::UnlockFailed));
    assert!(matches!(tamper_error, VaultError::UnlockFailed));
    assert_eq!(wrong_key_error.to_string(), tamper_error.to_string());
}

#[test]
fn recovery_carrier_and_metadata_are_versioned_without_plaintext_key_material() {
    let directory = tempdir().unwrap();
    let initialized = VaultKeyStore::initialize(directory.path()).unwrap();
    let (_, recovery_key) = initialized.into_parts();
    let recovery_text = recovery_key.expose_secret();
    let reparsed = RecoveryKey::parse(recovery_text).unwrap();
    let metadata = fs::read(directory.path().join(META_FILE)).unwrap();

    assert!(recovery_text.starts_with("eamrecovery1"));
    assert!(recovery_text.len() <= 90);
    assert_eq!(&metadata[..META_MAGIC.len()], META_MAGIC);
    assert_eq!(u16::from_le_bytes([metadata[8], metadata[9]]), 1);
    assert!(!contains_bytes(&metadata, recovery_text.as_bytes()));

    let recovered_key = VaultKeyStore::unlock_recovery(directory.path(), &reparsed).unwrap();
    VaultRepository::open(directory.path(), recovered_key)
        .unwrap()
        .close()
        .unwrap();
}

#[test]
fn malformed_recovery_carriers_fail_without_format_or_key_oracle() {
    let directory = tempdir().unwrap();
    let initialized = VaultKeyStore::initialize(directory.path()).unwrap();
    let (_, recovery_key) = initialized.into_parts();
    let mut malformed = recovery_key.expose_secret().as_bytes().to_vec();
    let last = malformed.last_mut().unwrap();
    *last = if *last == b'q' { b'p' } else { b'q' };
    let malformed = String::from_utf8(malformed).unwrap();

    let parse_error = RecoveryKey::parse(&malformed)
        .expect_err("a carrier with an invalid Bech32m checksum must fail");

    assert!(matches!(parse_error, VaultError::UnlockFailed));
}

#[test]
fn initialization_refuses_an_existing_database_without_key_metadata() {
    let directory = tempdir().unwrap();
    fs::write(
        directory.path().join("self.db"),
        b"existing encrypted vault",
    )
    .unwrap();

    let result = VaultKeyStore::initialize(directory.path());

    assert!(matches!(
        result,
        Err(VaultError::ExistingVaultWithoutKeyMetadata)
    ));
    assert!(!directory.path().join(META_FILE).exists());
}

fn remove_local_copy(vault_root: &std::path::Path) {
    let metadata_path = vault_root.join(META_FILE);
    let mut metadata = fs::read(&metadata_path).unwrap();
    assert!(metadata.len() > LOCAL_BYTES_OFFSET);
    metadata[LOCAL_LENGTH_OFFSET..LOCAL_BYTES_OFFSET].copy_from_slice(&0_u32.to_le_bytes());
    metadata.truncate(LOCAL_BYTES_OFFSET);
    fs::write(metadata_path, metadata).unwrap();
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn unlock_error(result: Result<eam_vault::VaultKey, VaultError>) -> VaultError {
    match result {
        Ok(_) => panic!("unlock unexpectedly succeeded"),
        Err(error) => error,
    }
}
