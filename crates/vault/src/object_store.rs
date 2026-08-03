use std::{
    collections::HashSet,
    fmt::Write as _,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use hmac::{Hmac, KeyInit as HmacKeyInit, Mac};
use sha2::Sha256;
use zeroize::{Zeroize, Zeroizing};

use crate::VaultError;

const OBJECTS_DIRECTORY: &str = "objects";
const OBJECT_MAGIC: &[u8; 8] = b"EAMOBJ01";
const OBJECT_AAD_PREFIX: &[u8] = b"eam-object-v1\0";
const NONCE_BYTES: usize = 24;
const AUTH_TAG_BYTES: usize = 16;

pub(crate) struct ObjectStore {
    root: PathBuf,
    key: Zeroizing<[u8; 32]>,
}

pub(crate) struct StoredObject {
    pub id: String,
    pub reused: bool,
}

impl ObjectStore {
    pub(crate) fn open(vault_root: &Path, key: Zeroizing<[u8; 32]>) -> Result<Self, VaultError> {
        Self::open_directory(vault_root.join(OBJECTS_DIRECTORY), key)
    }

    pub(crate) fn open_directory(
        root: PathBuf,
        key: Zeroizing<[u8; 32]>,
    ) -> Result<Self, VaultError> {
        fs::create_dir_all(&root)?;
        Ok(Self { root, key })
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn store(&self, plaintext: &[u8]) -> Result<StoredObject, VaultError> {
        let id = object_identifier(&self.key, plaintext)?;
        let destination = self.root.join(&id);
        if destination.exists() {
            self.verify_existing(&id, plaintext)?;
            return Ok(StoredObject { id, reused: true });
        }

        let mut nonce = [0_u8; NONCE_BYTES];
        getrandom::fill(&mut nonce).map_err(|_| VaultError::EntropyUnavailable)?;
        let cipher = XChaCha20Poly1305::new((&*self.key).into());
        let aad = object_aad(&id);
        let ciphertext = cipher
            .encrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: plaintext,
                    aad: &aad,
                },
            )
            .map_err(|_| VaultError::CipherUnavailable)?;

        let temporary = self.temporary_path()?;
        let write_result = (|| -> Result<(), VaultError> {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)?;
            file.write_all(OBJECT_MAGIC)?;
            file.write_all(&nonce)?;
            file.write_all(&ciphertext)?;
            file.sync_all()?;
            drop(file);
            match fs::rename(&temporary, &destination) {
                Ok(()) => Ok(()),
                Err(_) if destination.exists() => {
                    let _ = fs::remove_file(&temporary);
                    self.verify_existing(&id, plaintext)
                }
                Err(error) => Err(VaultError::Io(error)),
            }
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        write_result?;
        Ok(StoredObject { id, reused: false })
    }

    pub(crate) fn read(&self, id: &str) -> Result<Vec<u8>, VaultError> {
        if !valid_object_identifier(id) {
            return Err(VaultError::InvalidKeyOrCorrupt);
        }
        let mut encoded = Vec::new();
        File::open(self.root.join(id))?.read_to_end(&mut encoded)?;
        if encoded.len() < OBJECT_MAGIC.len() + NONCE_BYTES + AUTH_TAG_BYTES
            || &encoded[..OBJECT_MAGIC.len()] != OBJECT_MAGIC
        {
            return Err(VaultError::InvalidKeyOrCorrupt);
        }
        let nonce_start = OBJECT_MAGIC.len();
        let ciphertext_start = nonce_start + NONCE_BYTES;
        let nonce = <&XNonce>::try_from(&encoded[nonce_start..ciphertext_start])
            .map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
        let cipher = XChaCha20Poly1305::new((&*self.key).into());
        let aad = object_aad(id);
        let plaintext = cipher
            .decrypt(
                nonce,
                Payload {
                    msg: &encoded[ciphertext_start..],
                    aad: &aad,
                },
            )
            .map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
        if object_identifier(&self.key, &plaintext)? != id {
            return Err(VaultError::InvalidKeyOrCorrupt);
        }
        Ok(plaintext)
    }

    pub(crate) fn cleanup_unreferenced(
        &self,
        referenced: &HashSet<String>,
    ) -> Result<(), VaultError> {
        for id in referenced {
            if !valid_object_identifier(id) {
                return Err(VaultError::InvalidKeyOrCorrupt);
            }
            let metadata = fs::symlink_metadata(self.root.join(id))
                .map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(VaultError::InvalidKeyOrCorrupt);
            }
        }
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let metadata = fs::symlink_metadata(entry.path())?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(".pending-")
                || (valid_object_identifier(&name) && !referenced.contains(&name))
            {
                fs::remove_file(entry.path())?;
            }
        }
        Ok(())
    }

    pub(crate) fn zeroize(&mut self) {
        self.key.zeroize();
    }

    fn verify_existing(&self, id: &str, expected: &[u8]) -> Result<(), VaultError> {
        if self.read(id)? == expected {
            Ok(())
        } else {
            Err(VaultError::InvalidKeyOrCorrupt)
        }
    }

    fn temporary_path(&self) -> Result<PathBuf, VaultError> {
        let mut random = [0_u8; 16];
        getrandom::fill(&mut random).map_err(|_| VaultError::EntropyUnavailable)?;
        Ok(self.root.join(format!(".pending-{}", hex(&random))))
    }

    #[cfg(test)]
    pub(crate) fn object_file_count(&self) -> Result<usize, VaultError> {
        let mut count = 0;
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if valid_object_identifier(&name) {
                count += 1;
            }
        }
        Ok(count)
    }
}

fn object_identifier(key: &[u8; 32], plaintext: &[u8]) -> Result<String, VaultError> {
    let mut mac = <Hmac<Sha256> as HmacKeyInit>::new_from_slice(key)
        .map_err(|_| VaultError::CipherUnavailable)?;
    mac.update(plaintext);
    Ok(hex(&mac.finalize().into_bytes()))
}

fn object_aad(id: &str) -> Vec<u8> {
    let mut aad = Vec::with_capacity(OBJECT_AAD_PREFIX.len() + id.len());
    aad.extend_from_slice(OBJECT_AAD_PREFIX);
    aad.extend_from_slice(id.as_bytes());
    aad
}

fn valid_object_identifier(value: &str) -> bool {
    value.len() == 64
        && value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut output, "{byte:02x}").expect("writing to a string cannot fail");
    }
    output
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn authenticated_objects_deduplicate_and_reject_tampering() {
        let directory = tempdir().unwrap();
        let store = ObjectStore::open(directory.path(), Zeroizing::new([0x42; 32])).unwrap();

        let first = store.store(b"same evidence").unwrap();
        let second = store.store(b"same evidence").unwrap();
        assert!(!first.reused);
        assert!(second.reused);
        assert_eq!(first.id, second.id);
        assert_eq!(store.read(&first.id).unwrap(), b"same evidence");

        let path = store.root.join(&first.id);
        let mut encoded = fs::read(&path).unwrap();
        *encoded.last_mut().unwrap() ^= 1;
        fs::write(path, encoded).unwrap();
        assert!(matches!(
            store.read(&first.id),
            Err(VaultError::InvalidKeyOrCorrupt)
        ));
    }
}
