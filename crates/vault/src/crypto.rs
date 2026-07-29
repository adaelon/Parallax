use std::fmt::Write as _;

use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::{Zeroize, Zeroizing};

use crate::VaultError;

const KDF_SALT: &[u8] = b"evrything-about-me/v1/vault-subkeys";
const DATABASE_INFO: &[u8] = b"database";
const OBJECTS_INFO: &[u8] = b"objects";
#[cfg(test)]
const BACKUP_INFO: &[u8] = b"backup";

/// A high-entropy 256-bit key owned by the trusted vault boundary.
///
/// The wrapper intentionally omits `Clone` and `Debug`, and clears its owned
/// bytes on explicit close or drop.
pub struct VaultKey(Zeroizing<[u8; 32]>);

impl VaultKey {
    #[must_use]
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(Zeroizing::new(bytes))
    }

    pub(crate) fn generate() -> Result<Self, VaultError> {
        let mut bytes = Zeroizing::new([0_u8; 32]);
        getrandom::fill(&mut *bytes).map_err(|_| VaultError::EntropyUnavailable)?;
        Ok(Self(bytes))
    }

    pub(crate) fn from_zeroizing(bytes: Zeroizing<[u8; 32]>) -> Self {
        Self(bytes)
    }

    pub(crate) fn expose_secret(&self) -> &[u8; 32] {
        &self.0
    }

    pub(crate) fn database_key(&self) -> Result<Zeroizing<[u8; 32]>, VaultError> {
        derive_subkey(&self.0, DATABASE_INFO)
    }

    pub(crate) fn objects_key(&self) -> Result<Zeroizing<[u8; 32]>, VaultError> {
        derive_subkey(&self.0, OBJECTS_INFO)
    }

    pub(crate) fn zeroize(&mut self) {
        self.0.zeroize();
    }

    #[cfg(test)]
    pub(crate) fn is_zeroed(&self) -> bool {
        self.0.iter().all(|byte| *byte == 0)
    }
}

fn derive_subkey(vault_key: &[u8; 32], purpose: &[u8]) -> Result<Zeroizing<[u8; 32]>, VaultError> {
    let hkdf = Hkdf::<Sha256>::new(Some(KDF_SALT), vault_key);
    let mut output = Zeroizing::new([0_u8; 32]);
    hkdf.expand(purpose, &mut *output)
        .map_err(|_| VaultError::CipherUnavailable)?;
    Ok(output)
}

pub(crate) fn sqlcipher_key_pragma(key: &[u8; 32]) -> Zeroizing<String> {
    let mut statement = Zeroizing::new(String::with_capacity(82));
    statement.push_str("PRAGMA key = \"x'");
    for byte in key {
        write!(&mut *statement, "{byte:02x}").expect("writing to a string cannot fail");
    }
    statement.push_str("'\";");
    statement
}

#[cfg(test)]
mod tests {
    use chacha20poly1305::{
        XChaCha20Poly1305, XNonce,
        aead::{Aead, KeyInit, Payload},
    };

    use super::*;

    const FIXED_VAULT_KEY: [u8; 32] = [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
        0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d,
        0x1e, 0x1f,
    ];

    fn hex(bytes: &[u8]) -> String {
        let mut output = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            write!(&mut output, "{byte:02x}").unwrap();
        }
        output
    }

    #[test]
    fn derives_purpose_separated_hkdf_sha256_vectors() {
        let database = derive_subkey(&FIXED_VAULT_KEY, DATABASE_INFO).unwrap();
        let objects = derive_subkey(&FIXED_VAULT_KEY, OBJECTS_INFO).unwrap();
        let backup = derive_subkey(&FIXED_VAULT_KEY, BACKUP_INFO).unwrap();

        assert_eq!(
            hex(&*database),
            "e1d6b5beeac3e77b98ac542d45d47fdc4d6586b2d02335985d878ae9482f85bc"
        );
        assert_eq!(
            hex(&*objects),
            "b25fec21e8d2c23bba22c3c8b601778d473b5d24056b5d0ca74c1de37c89c6c6"
        );
        assert_eq!(
            hex(&*backup),
            "028d1dcaf77715b8c50c5c04cab32cf7a3747bfd2c81f3614d666f9dbd70fc76"
        );
        assert_ne!(*database, *objects);
        assert_ne!(*database, *backup);
        assert_ne!(*objects, *backup);
    }

    #[test]
    fn xchacha20poly1305_object_profile_has_a_fixed_authenticated_vector() {
        let key = derive_subkey(&FIXED_VAULT_KEY, OBJECTS_INFO).unwrap();
        let nonce = XNonce::from([
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
            0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
        ]);
        let cipher = XChaCha20Poly1305::new((&*key).into());
        let plaintext = b"eam-object-vector-v1";
        let aad = b"eam-object-v1";
        let encrypted = cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext,
                    aad,
                },
            )
            .unwrap();

        assert_eq!(
            hex(&encrypted),
            "8a39b5fff71d11951221aab028b9bc24425b7b9e7b702b7a3d1885029813757a16a37a9c"
        );
        assert_eq!(
            cipher
                .decrypt(
                    &nonce,
                    Payload {
                        msg: &encrypted,
                        aad
                    }
                )
                .unwrap(),
            plaintext
        );

        let mut tampered = encrypted;
        tampered[0] ^= 1;
        assert!(
            cipher
                .decrypt(
                    &nonce,
                    Payload {
                        msg: &tampered,
                        aad
                    }
                )
                .is_err()
        );
    }

    #[test]
    fn vault_key_explicitly_zeroizes_owned_bytes() {
        let mut key = VaultKey::new([0xa5; 32]);
        key.zeroize();
        assert!(key.is_zeroed());
    }
}
