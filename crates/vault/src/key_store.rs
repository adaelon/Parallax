use std::{
    fmt,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use bech32::{Bech32m, Hrp, primitives::decode::CheckedHrpstring};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::{VaultError, VaultKey};

const METADATA_FILE: &str = "bundle.meta";
const DATABASE_FILE: &str = "self.db";
const METADATA_MAGIC: &[u8; 8] = b"EAMKEYS\0";
const METADATA_VERSION: u16 = 1;
const RECOVERY_HRP: &str = "eamrecovery";
const RECOVERY_WRAP_INFO: &[u8] = b"evrything-about-me/v1/recovery-wrap";
const RECOVERY_WRAP_AAD: &[u8] = b"evrything-about-me/bundle-meta/v1/recovery-wrap";
const RECOVERY_SALT_LENGTH: usize = 32;
const RECOVERY_NONCE_LENGTH: usize = 24;
const RECOVERY_CIPHERTEXT_LENGTH: usize = 48;
const FIXED_METADATA_LENGTH: usize = 8 + 2 + 32 + 24 + 48 + 4;
const MAX_LOCAL_WRAP_LENGTH: usize = 16 * 1024;
const MAX_METADATA_LENGTH: usize = FIXED_METADATA_LENGTH + MAX_LOCAL_WRAP_LENGTH;

/// A versioned, checksummed carrier for the user-held 256-bit recovery secret.
///
/// The secret is intentionally neither clonable nor printable through
/// `Display`; callers must explicitly opt in through [`Self::expose_secret`]
/// when presenting it once to the person.
pub struct RecoveryKey(Zeroizing<String>);

impl RecoveryKey {
    /// Validates and canonicalizes a Bech32m recovery carrier.
    ///
    /// # Errors
    ///
    /// Every malformed carrier uses the same failure surface as an incorrect
    /// but well-formed recovery key.
    pub fn parse(secret: &str) -> Result<Self, VaultError> {
        let raw = decode_recovery_key(secret)?;
        let canonical = encode_recovery_key(&raw).map_err(|_| VaultError::UnlockFailed)?;
        Ok(Self(Zeroizing::new(canonical)))
    }

    #[must_use]
    pub fn expose_secret(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Debug for RecoveryKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RecoveryKey([REDACTED])")
    }
}

/// Secrets produced while initializing a vault for the first time.
pub struct InitializedVault {
    vault_key: VaultKey,
    recovery_key: RecoveryKey,
}

impl InitializedVault {
    #[must_use]
    pub fn into_parts(self) -> (VaultKey, RecoveryKey) {
        (self.vault_key, self.recovery_key)
    }
}

/// First-run key material prepared in memory but not yet committed to a vault.
///
/// Dropping this value clears both secrets and leaves the filesystem unchanged.
/// This lets a trusted caller present the Recovery Key and wait for explicit
/// confirmation before making the new vault durable.
pub struct PreparedVault {
    vault_key: VaultKey,
    recovery_key: RecoveryKey,
    encoded_metadata: Zeroizing<Vec<u8>>,
}

impl PreparedVault {
    #[must_use]
    pub fn recovery_key(&self) -> &RecoveryKey {
        &self.recovery_key
    }

    /// Atomically commits the prepared key metadata to an empty vault root.
    ///
    /// # Errors
    ///
    /// Fails if key metadata or an encrypted database already exists, or when
    /// the destination cannot be created and synchronized.
    pub fn commit(&self, vault_root: impl AsRef<Path>) -> Result<(), VaultError> {
        let vault_root = vault_root.as_ref();
        fs::create_dir_all(vault_root)?;
        if vault_root.join(METADATA_FILE).try_exists()? {
            return Err(VaultError::AlreadyInitialized);
        }
        if vault_root.join(DATABASE_FILE).try_exists()? {
            return Err(VaultError::ExistingVaultWithoutKeyMetadata);
        }
        persist_metadata(vault_root, &self.encoded_metadata)
    }

    #[must_use]
    pub fn into_parts(self) -> (VaultKey, RecoveryKey) {
        (self.vault_key, self.recovery_key)
    }
}

/// Creates and opens the two independent S03 Vault Key unlock paths.
pub struct VaultKeyStore;

impl VaultKeyStore {
    /// Reports whether committed key metadata exists at `vault_root`.
    ///
    /// # Errors
    ///
    /// Fails closed when an encrypted database exists without key metadata or
    /// when the filesystem cannot be inspected.
    pub fn is_initialized(vault_root: impl AsRef<Path>) -> Result<bool, VaultError> {
        let vault_root = vault_root.as_ref();
        if vault_root.join(METADATA_FILE).try_exists()? {
            return Ok(true);
        }
        if vault_root.join(DATABASE_FILE).try_exists()? {
            return Err(VaultError::ExistingVaultWithoutKeyMetadata);
        }
        Ok(false)
    }

    /// Prepares first-run key material without writing anything to disk.
    ///
    /// # Errors
    ///
    /// Fails when secure randomness or the Windows `CurrentUser` DPAPI wrapper
    /// is unavailable.
    pub fn prepare() -> Result<PreparedVault, VaultError> {
        prepare()
    }

    /// Creates a random Vault Key, a user-held Recovery Key, and one atomic
    /// metadata bundle containing independent recovery and DPAPI wrappers.
    ///
    /// # Errors
    ///
    /// Fails if key metadata already exists, secure randomness or DPAPI is
    /// unavailable, or the metadata cannot be committed.
    pub fn initialize(vault_root: impl AsRef<Path>) -> Result<InitializedVault, VaultError> {
        let prepared = Self::prepare()?;
        prepared.commit(vault_root)?;
        let (vault_key, recovery_key) = prepared.into_parts();
        Ok(InitializedVault {
            vault_key,
            recovery_key,
        })
    }

    /// Unlocks the Vault Key through the Windows `CurrentUser` DPAPI copy.
    ///
    /// # Errors
    ///
    /// Uses one generic error for an absent, invalid, foreign-user, or tampered
    /// DPAPI copy.
    pub fn unlock_local(vault_root: impl AsRef<Path>) -> Result<VaultKey, VaultError> {
        let metadata = read_metadata(vault_root.as_ref())?;
        let local_wrapped = metadata
            .local_wrapped_key
            .as_deref()
            .ok_or(VaultError::UnlockFailed)?;
        unlock_local(local_wrapped)
    }

    /// Unlocks the Vault Key only from portable metadata and the Recovery Key.
    /// The DPAPI field is deliberately not inspected.
    ///
    /// # Errors
    ///
    /// Malformed carriers, wrong keys, and authenticated metadata tampering all
    /// return [`VaultError::UnlockFailed`].
    pub fn unlock_recovery(
        vault_root: impl AsRef<Path>,
        recovery_key: &RecoveryKey,
    ) -> Result<VaultKey, VaultError> {
        let metadata = read_metadata(vault_root.as_ref())?;
        let raw_recovery_key = decode_recovery_key(recovery_key.expose_secret())?;
        unwrap_recovery_key(&metadata, &raw_recovery_key)
    }

    pub(crate) fn portable_metadata(vault_root: &Path) -> Result<Vec<u8>, VaultError> {
        let mut metadata = read_metadata(vault_root)?;
        metadata.local_wrapped_key = None;
        metadata.encode()
    }

    pub(crate) fn unlock_recovery_metadata(
        encoded: &[u8],
        recovery_key: &RecoveryKey,
    ) -> Result<VaultKey, VaultError> {
        let metadata = BundleMetadata::decode(encoded)?;
        let raw_recovery_key = decode_recovery_key(recovery_key.expose_secret())?;
        unwrap_recovery_key(&metadata, &raw_recovery_key)
    }

    pub(crate) fn install_rotated_metadata(
        vault_root: &Path,
        vault_key: &VaultKey,
        recovery_key: &RecoveryKey,
    ) -> Result<(), VaultError> {
        let raw_recovery_key = decode_recovery_key(recovery_key.expose_secret())?;
        let metadata = metadata_for_vault_key(vault_key, &raw_recovery_key)?;
        persist_metadata(vault_root, &metadata.encode()?)
    }
}

fn metadata_for_vault_key(
    vault_key: &VaultKey,
    raw_recovery_key: &[u8; 32],
) -> Result<BundleMetadata, VaultError> {
    let mut recovery_salt = [0_u8; RECOVERY_SALT_LENGTH];
    let mut recovery_nonce = [0_u8; RECOVERY_NONCE_LENGTH];
    fill_random(&mut recovery_salt)?;
    fill_random(&mut recovery_nonce)?;
    let recovery_wrapped_key = wrap_recovery_key(
        vault_key.expose_secret(),
        raw_recovery_key,
        &recovery_salt,
        &recovery_nonce,
    )?;
    Ok(BundleMetadata {
        recovery_salt,
        recovery_nonce,
        recovery_wrapped_key,
        local_wrapped_key: local_wrap_for_restore(vault_key)?,
    })
}

#[cfg(windows)]
fn local_wrap_for_restore(vault_key: &VaultKey) -> Result<Option<Vec<u8>>, VaultError> {
    crate::dpapi::protect_current_user(vault_key.expose_secret()).map(Some)
}

#[cfg(not(windows))]
fn local_wrap_for_restore(_vault_key: &VaultKey) -> Result<Option<Vec<u8>>, VaultError> {
    Ok(None)
}

#[cfg(windows)]
fn prepare() -> Result<PreparedVault, VaultError> {
    let vault_key = VaultKey::generate()?;
    let mut raw_recovery_key = Zeroizing::new([0_u8; 32]);
    fill_random(&mut *raw_recovery_key)?;
    let recovery_key = RecoveryKey(Zeroizing::new(
        encode_recovery_key(&raw_recovery_key).map_err(|_| VaultError::KeyProtectionFailed)?,
    ));

    let mut recovery_salt = [0_u8; RECOVERY_SALT_LENGTH];
    let mut recovery_nonce = [0_u8; RECOVERY_NONCE_LENGTH];
    fill_random(&mut recovery_salt)?;
    fill_random(&mut recovery_nonce)?;
    let recovery_wrapped_key = wrap_recovery_key(
        vault_key.expose_secret(),
        &raw_recovery_key,
        &recovery_salt,
        &recovery_nonce,
    )?;
    let local_wrapped_key = crate::dpapi::protect_current_user(vault_key.expose_secret())?;
    let metadata = BundleMetadata {
        recovery_salt,
        recovery_nonce,
        recovery_wrapped_key,
        local_wrapped_key: Some(local_wrapped_key),
    };
    Ok(PreparedVault {
        vault_key,
        recovery_key,
        encoded_metadata: Zeroizing::new(metadata.encode()?),
    })
}

#[cfg(not(windows))]
fn prepare() -> Result<PreparedVault, VaultError> {
    Err(VaultError::UnsupportedPlatform)
}

#[cfg(windows)]
fn unlock_local(local_wrapped_key: &[u8]) -> Result<VaultKey, VaultError> {
    let plaintext = crate::dpapi::unprotect_current_user(local_wrapped_key)?;
    if plaintext.len() != 32 {
        return Err(VaultError::UnlockFailed);
    }
    let mut bytes = Zeroizing::new([0_u8; 32]);
    bytes.copy_from_slice(&plaintext);
    Ok(VaultKey::from_zeroizing(bytes))
}

#[cfg(not(windows))]
fn unlock_local(_local_wrapped_key: &[u8]) -> Result<VaultKey, VaultError> {
    Err(VaultError::UnsupportedPlatform)
}

fn fill_random(output: &mut [u8]) -> Result<(), VaultError> {
    getrandom::fill(output).map_err(|_| VaultError::EntropyUnavailable)
}

fn encode_recovery_key(raw: &[u8; 32]) -> Result<String, bech32::EncodeError> {
    let hrp = Hrp::parse(RECOVERY_HRP).expect("the recovery HRP is a valid constant");
    bech32::encode::<Bech32m>(hrp, raw)
}

fn decode_recovery_key(secret: &str) -> Result<Zeroizing<[u8; 32]>, VaultError> {
    let checked = CheckedHrpstring::new::<Bech32m>(secret).map_err(|_| VaultError::UnlockFailed)?;
    let expected_hrp = Hrp::parse(RECOVERY_HRP).expect("the recovery HRP is a valid constant");
    if checked.hrp() != expected_hrp {
        return Err(VaultError::UnlockFailed);
    }
    let decoded = Zeroizing::new(checked.byte_iter().collect::<Vec<_>>());
    if decoded.len() != 32 {
        return Err(VaultError::UnlockFailed);
    }
    let mut raw = Zeroizing::new([0_u8; 32]);
    raw.copy_from_slice(&decoded);
    let canonical = encode_recovery_key(&raw).map_err(|_| VaultError::UnlockFailed)?;
    if !canonical.eq_ignore_ascii_case(secret) {
        return Err(VaultError::UnlockFailed);
    }
    Ok(raw)
}

fn recovery_wrap_key(raw_recovery_key: &[u8; 32], salt: &[u8; 32]) -> Zeroizing<[u8; 32]> {
    let hkdf = Hkdf::<Sha256>::new(Some(salt), raw_recovery_key);
    let mut output = Zeroizing::new([0_u8; 32]);
    hkdf.expand(RECOVERY_WRAP_INFO, &mut *output)
        .expect("32 bytes is a valid HKDF-SHA256 output length");
    output
}

fn wrap_recovery_key(
    vault_key: &[u8; 32],
    raw_recovery_key: &[u8; 32],
    salt: &[u8; 32],
    nonce: &[u8; 24],
) -> Result<[u8; RECOVERY_CIPHERTEXT_LENGTH], VaultError> {
    let wrapping_key = recovery_wrap_key(raw_recovery_key, salt);
    let cipher = XChaCha20Poly1305::new((&*wrapping_key).into());
    let ciphertext = cipher
        .encrypt(
            &XNonce::from(*nonce),
            Payload {
                msg: vault_key,
                aad: RECOVERY_WRAP_AAD,
            },
        )
        .map_err(|_| VaultError::KeyProtectionFailed)?;
    ciphertext
        .try_into()
        .map_err(|_| VaultError::KeyProtectionFailed)
}

fn unwrap_recovery_key(
    metadata: &BundleMetadata,
    raw_recovery_key: &[u8; 32],
) -> Result<VaultKey, VaultError> {
    let wrapping_key = recovery_wrap_key(raw_recovery_key, &metadata.recovery_salt);
    let cipher = XChaCha20Poly1305::new((&*wrapping_key).into());
    let plaintext = Zeroizing::new(
        cipher
            .decrypt(
                &XNonce::from(metadata.recovery_nonce),
                Payload {
                    msg: &metadata.recovery_wrapped_key,
                    aad: RECOVERY_WRAP_AAD,
                },
            )
            .map_err(|_| VaultError::UnlockFailed)?,
    );
    if plaintext.len() != 32 {
        return Err(VaultError::UnlockFailed);
    }
    let mut bytes = Zeroizing::new([0_u8; 32]);
    bytes.copy_from_slice(&plaintext);
    Ok(VaultKey::from_zeroizing(bytes))
}

struct BundleMetadata {
    recovery_salt: [u8; RECOVERY_SALT_LENGTH],
    recovery_nonce: [u8; RECOVERY_NONCE_LENGTH],
    recovery_wrapped_key: [u8; RECOVERY_CIPHERTEXT_LENGTH],
    local_wrapped_key: Option<Vec<u8>>,
}

impl BundleMetadata {
    fn encode(&self) -> Result<Vec<u8>, VaultError> {
        let local_length = self.local_wrapped_key.as_ref().map_or(0, Vec::len);
        if local_length > MAX_LOCAL_WRAP_LENGTH {
            return Err(VaultError::KeyProtectionFailed);
        }
        let local_length =
            u32::try_from(local_length).map_err(|_| VaultError::KeyProtectionFailed)?;
        let mut encoded = Vec::with_capacity(FIXED_METADATA_LENGTH + local_length as usize);
        encoded.extend_from_slice(METADATA_MAGIC);
        encoded.extend_from_slice(&METADATA_VERSION.to_le_bytes());
        encoded.extend_from_slice(&self.recovery_salt);
        encoded.extend_from_slice(&self.recovery_nonce);
        encoded.extend_from_slice(&self.recovery_wrapped_key);
        encoded.extend_from_slice(&local_length.to_le_bytes());
        if let Some(local_wrapped_key) = &self.local_wrapped_key {
            encoded.extend_from_slice(local_wrapped_key);
        }
        Ok(encoded)
    }

    fn decode(encoded: &[u8]) -> Result<Self, VaultError> {
        if encoded.len() < FIXED_METADATA_LENGTH || encoded.len() > MAX_METADATA_LENGTH {
            return Err(VaultError::UnlockFailed);
        }
        if encoded.get(..METADATA_MAGIC.len()) != Some(METADATA_MAGIC) {
            return Err(VaultError::UnlockFailed);
        }
        let version = u16::from_le_bytes(
            encoded[8..10]
                .try_into()
                .map_err(|_| VaultError::UnlockFailed)?,
        );
        if version != METADATA_VERSION {
            return Err(VaultError::UnsupportedKeyMetadata(version));
        }

        let recovery_salt = encoded[10..42]
            .try_into()
            .map_err(|_| VaultError::UnlockFailed)?;
        let recovery_nonce = encoded[42..66]
            .try_into()
            .map_err(|_| VaultError::UnlockFailed)?;
        let recovery_wrapped_key = encoded[66..114]
            .try_into()
            .map_err(|_| VaultError::UnlockFailed)?;
        let local_length = u32::from_le_bytes(
            encoded[114..118]
                .try_into()
                .map_err(|_| VaultError::UnlockFailed)?,
        ) as usize;
        if local_length > MAX_LOCAL_WRAP_LENGTH
            || encoded.len() != FIXED_METADATA_LENGTH + local_length
        {
            return Err(VaultError::UnlockFailed);
        }
        let local_wrapped_key = (local_length > 0).then(|| encoded[118..].to_vec());
        Ok(Self {
            recovery_salt,
            recovery_nonce,
            recovery_wrapped_key,
            local_wrapped_key,
        })
    }
}

fn read_metadata(vault_root: &Path) -> Result<BundleMetadata, VaultError> {
    let file = File::open(vault_root.join(METADATA_FILE))?;
    let mut encoded = Vec::with_capacity(FIXED_METADATA_LENGTH);
    file.take(
        u64::try_from(MAX_METADATA_LENGTH + 1).expect("the metadata size limit always fits in u64"),
    )
    .read_to_end(&mut encoded)?;
    BundleMetadata::decode(&encoded)
}

fn persist_metadata(vault_root: &Path, encoded: &[u8]) -> Result<(), VaultError> {
    let metadata_path = vault_root.join(METADATA_FILE);
    if metadata_path.exists() {
        return Err(VaultError::AlreadyInitialized);
    }
    let pending_path = create_pending_path(vault_root)?;
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&pending_path)?;
        file.write_all(encoded)?;
        file.sync_all()?;
        fs::rename(&pending_path, &metadata_path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&pending_path);
    }
    result
}

fn create_pending_path(vault_root: &Path) -> Result<PathBuf, VaultError> {
    for _ in 0..4 {
        let mut random = [0_u8; 8];
        fill_random(&mut random)?;
        let candidate = vault_root.join(format!(
            ".bundle.meta.{:016x}.pending",
            u64::from_le_bytes(random)
        ));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(VaultError::KeyProtectionFailed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_metadata_rejects_truncation_and_trailing_bytes() {
        let metadata = BundleMetadata {
            recovery_salt: [1; 32],
            recovery_nonce: [2; 24],
            recovery_wrapped_key: [3; 48],
            local_wrapped_key: Some(vec![4; 16]),
        };
        let encoded = metadata.encode().unwrap();

        assert!(matches!(
            BundleMetadata::decode(&encoded[..encoded.len() - 1]),
            Err(VaultError::UnlockFailed)
        ));
        let mut trailing = encoded;
        trailing.push(0);
        assert!(matches!(
            BundleMetadata::decode(&trailing),
            Err(VaultError::UnlockFailed)
        ));
    }
}
