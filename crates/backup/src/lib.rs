//! Authenticated S30 backup envelopes and deterministic recovery-set payloads.

use std::{collections::HashSet, error::Error, fmt};

use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

const ENVELOPE_MAGIC: &[u8; 8] = b"EAMBAK01";
const ENVELOPE_VERSION: u16 = 1;
const ENVELOPE_FIXED_BYTES: usize = 8 + 2 + 1 + 1 + 4 + 24 + 8;
const MAX_PORTABLE_METADATA_BYTES: usize = 32 * 1024;
const MAX_PAYLOAD_BYTES: usize = 1024 * 1024 * 1024;
const MAX_FILES: usize = 1_000_000;
const MAX_PATH_BYTES: usize = 512;
const MAX_DELETIONS: usize = 10_000_000;
const SNAPSHOT_MAGIC: &[u8; 8] = b"EAMSNP01";
const DELETION_MAGIC: &[u8; 8] = b"EAMDEL01";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum EnvelopeKind {
    Snapshot = 1,
    DeletionHead = 2,
}

impl EnvelopeKind {
    fn decode(value: u8) -> Result<Self, BackupFormatError> {
        match value {
            1 => Ok(Self::Snapshot),
            2 => Ok(Self::DeletionHead),
            _ => Err(BackupFormatError),
        }
    }
}

/// A purpose-separated key owned by the trusted backup boundary.
pub struct BackupKey(Zeroizing<[u8; 32]>);

impl BackupKey {
    #[must_use]
    pub fn new(bytes: Zeroizing<[u8; 32]>) -> Self {
        Self(bytes)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackupFormatError;

impl fmt::Display for BackupFormatError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("encrypted backup is invalid")
    }
}

impl Error for BackupFormatError {}

pub struct EnvelopeHeader {
    kind: EnvelopeKind,
    portable_metadata: Vec<u8>,
}

impl EnvelopeHeader {
    #[must_use]
    pub const fn kind(&self) -> EnvelopeKind {
        self.kind
    }

    #[must_use]
    pub fn portable_metadata(&self) -> &[u8] {
        &self.portable_metadata
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotFile {
    path: String,
    bytes: Vec<u8>,
}

impl SnapshotFile {
    /// Creates one exact relative file entry.
    ///
    /// # Errors
    ///
    /// Rejects empty, absolute, traversal, NUL-containing, or overlong paths.
    pub fn new(path: impl Into<String>, bytes: Vec<u8>) -> Result<Self, BackupFormatError> {
        let path = path.into();
        validate_path(&path)?;
        Ok(Self { path, bytes })
    }

    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    pub fn into_parts(self) -> (String, Vec<u8>) {
        (self.path, self.bytes)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Snapshot {
    set_id: [u8; 16],
    generation: u64,
    created_at_millis: i64,
    deletion_watermark: u64,
    schema_version: u32,
    files: Vec<SnapshotFile>,
}

impl Snapshot {
    #[must_use]
    pub fn new(
        set_id: [u8; 16],
        generation: u64,
        created_at_millis: i64,
        deletion_watermark: u64,
        schema_version: u32,
        files: Vec<SnapshotFile>,
    ) -> Self {
        Self {
            set_id,
            generation,
            created_at_millis,
            deletion_watermark,
            schema_version,
            files,
        }
    }

    #[must_use]
    pub const fn set_id(&self) -> [u8; 16] {
        self.set_id
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    pub const fn created_at_millis(&self) -> i64 {
        self.created_at_millis
    }

    #[must_use]
    pub const fn deletion_watermark(&self) -> u64 {
        self.deletion_watermark
    }

    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    #[must_use]
    pub fn files(&self) -> &[SnapshotFile] {
        &self.files
    }

    #[must_use]
    pub fn into_files(self) -> Vec<SnapshotFile> {
        self.files
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeletionRecord {
    intent_id: u64,
    target_kind: u8,
    target_id: u64,
    requested_at_millis: i64,
}

impl DeletionRecord {
    #[must_use]
    pub const fn new(
        intent_id: u64,
        target_kind: u8,
        target_id: u64,
        requested_at_millis: i64,
    ) -> Self {
        Self {
            intent_id,
            target_kind,
            target_id,
            requested_at_millis,
        }
    }

    #[must_use]
    pub const fn intent_id(&self) -> u64 {
        self.intent_id
    }

    #[must_use]
    pub const fn target_kind(&self) -> u8 {
        self.target_kind
    }

    #[must_use]
    pub const fn target_id(&self) -> u64 {
        self.target_id
    }

    #[must_use]
    pub const fn requested_at_millis(&self) -> i64 {
        self.requested_at_millis
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeletionHead {
    set_id: [u8; 16],
    latest_generation: u64,
    updated_at_millis: i64,
    records: Vec<DeletionRecord>,
}

impl DeletionHead {
    #[must_use]
    pub fn new(
        set_id: [u8; 16],
        latest_generation: u64,
        updated_at_millis: i64,
        records: Vec<DeletionRecord>,
    ) -> Self {
        Self {
            set_id,
            latest_generation,
            updated_at_millis,
            records,
        }
    }

    #[must_use]
    pub const fn set_id(&self) -> [u8; 16] {
        self.set_id
    }

    #[must_use]
    pub const fn latest_generation(&self) -> u64 {
        self.latest_generation
    }

    #[must_use]
    pub const fn updated_at_millis(&self) -> i64 {
        self.updated_at_millis
    }

    #[must_use]
    pub fn records(&self) -> &[DeletionRecord] {
        &self.records
    }
}

/// Reads only the bounded portable-unlock header needed to derive a Backup Key.
///
/// # Errors
///
/// Rejects malformed, unsupported, or overlong envelopes before allocation.
pub fn inspect_envelope(encoded: &[u8]) -> Result<EnvelopeHeader, BackupFormatError> {
    let parsed = parse_envelope(encoded)?;
    Ok(EnvelopeHeader {
        kind: parsed.kind,
        portable_metadata: parsed.metadata.to_vec(),
    })
}

/// Authenticates and decrypts one envelope.
///
/// # Errors
///
/// Wrong keys, header changes, truncation, trailing bytes, and ciphertext changes
/// use the same invalid-backup surface.
pub fn open_envelope(
    encoded: &[u8],
    expected_kind: EnvelopeKind,
    key: &BackupKey,
) -> Result<Vec<u8>, BackupFormatError> {
    let parsed = parse_envelope(encoded)?;
    if parsed.kind != expected_kind {
        return Err(BackupFormatError);
    }
    let aad = &encoded[..parsed.ciphertext_offset];
    let cipher = XChaCha20Poly1305::new((&*key.0).into());
    cipher
        .decrypt(
            &XNonce::from(parsed.nonce),
            Payload {
                msg: parsed.ciphertext,
                aad,
            },
        )
        .map_err(|_| BackupFormatError)
}

/// Encrypts a payload with fresh nonce and authenticated portable metadata.
///
/// # Errors
///
/// Rejects oversized inputs and entropy/encryption failures.
pub fn seal_envelope(
    kind: EnvelopeKind,
    portable_metadata: &[u8],
    payload: &[u8],
    key: &BackupKey,
) -> Result<Vec<u8>, BackupFormatError> {
    if portable_metadata.is_empty()
        || portable_metadata.len() > MAX_PORTABLE_METADATA_BYTES
        || payload.len() > MAX_PAYLOAD_BYTES
    {
        return Err(BackupFormatError);
    }
    let metadata_len = u32::try_from(portable_metadata.len()).map_err(|_| BackupFormatError)?;
    let ciphertext_len = payload.len().checked_add(16).ok_or(BackupFormatError)?;
    let ciphertext_len = u64::try_from(ciphertext_len).map_err(|_| BackupFormatError)?;
    let ciphertext_capacity = usize::try_from(ciphertext_len).map_err(|_| BackupFormatError)?;
    let mut nonce = [0_u8; 24];
    getrandom::fill(&mut nonce).map_err(|_| BackupFormatError)?;
    let mut encoded =
        Vec::with_capacity(ENVELOPE_FIXED_BYTES + portable_metadata.len() + ciphertext_capacity);
    encoded.extend_from_slice(ENVELOPE_MAGIC);
    encoded.extend_from_slice(&ENVELOPE_VERSION.to_le_bytes());
    encoded.push(kind as u8);
    encoded.push(0);
    encoded.extend_from_slice(&metadata_len.to_le_bytes());
    encoded.extend_from_slice(&nonce);
    encoded.extend_from_slice(&ciphertext_len.to_le_bytes());
    encoded.extend_from_slice(portable_metadata);
    let cipher = XChaCha20Poly1305::new((&*key.0).into());
    let ciphertext = cipher
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: payload,
                aad: &encoded,
            },
        )
        .map_err(|_| BackupFormatError)?;
    encoded.extend_from_slice(&ciphertext);
    Ok(encoded)
}

/// Serializes one exact, checksummed snapshot payload.
///
/// # Errors
///
/// Rejects empty/oversized file sets, invalid or duplicate paths, and payloads
/// that exceed the fixed recovery resource bound.
pub fn encode_snapshot(snapshot: &Snapshot) -> Result<Vec<u8>, BackupFormatError> {
    if snapshot.generation == 0 || snapshot.files.is_empty() || snapshot.files.len() > MAX_FILES {
        return Err(BackupFormatError);
    }
    let mut seen = HashSet::with_capacity(snapshot.files.len());
    let mut encoded = Vec::new();
    encoded.extend_from_slice(SNAPSHOT_MAGIC);
    encoded.extend_from_slice(&snapshot.set_id);
    encoded.extend_from_slice(&snapshot.generation.to_le_bytes());
    encoded.extend_from_slice(&snapshot.created_at_millis.to_le_bytes());
    encoded.extend_from_slice(&snapshot.deletion_watermark.to_le_bytes());
    encoded.extend_from_slice(&snapshot.schema_version.to_le_bytes());
    encoded.extend_from_slice(
        &u32::try_from(snapshot.files.len())
            .map_err(|_| BackupFormatError)?
            .to_le_bytes(),
    );
    for file in &snapshot.files {
        validate_path(&file.path)?;
        if !seen.insert(file.path.as_str()) {
            return Err(BackupFormatError);
        }
        let path = file.path.as_bytes();
        encoded.extend_from_slice(
            &u16::try_from(path.len())
                .map_err(|_| BackupFormatError)?
                .to_le_bytes(),
        );
        encoded.extend_from_slice(path);
        encoded.extend_from_slice(
            &u64::try_from(file.bytes.len())
                .map_err(|_| BackupFormatError)?
                .to_le_bytes(),
        );
        encoded.extend_from_slice(&Sha256::digest(&file.bytes));
        encoded.extend_from_slice(&file.bytes);
        if encoded.len() > MAX_PAYLOAD_BYTES {
            return Err(BackupFormatError);
        }
    }
    Ok(encoded)
}

/// Parses and verifies one exact snapshot payload.
///
/// # Errors
///
/// Rejects malformed lengths, paths, duplicate entries, digest changes, and
/// trailing bytes.
pub fn decode_snapshot(encoded: &[u8]) -> Result<Snapshot, BackupFormatError> {
    let mut cursor = Cursor::new(encoded);
    cursor.expect(SNAPSHOT_MAGIC)?;
    let set_id = cursor.array()?;
    let generation = cursor.u64()?;
    let created_at_millis = cursor.i64()?;
    let deletion_watermark = cursor.u64()?;
    let schema_version = cursor.u32()?;
    let file_count = usize::try_from(cursor.u32()?).map_err(|_| BackupFormatError)?;
    if generation == 0 || file_count == 0 || file_count > MAX_FILES {
        return Err(BackupFormatError);
    }
    let mut seen = HashSet::with_capacity(file_count);
    let mut files = Vec::with_capacity(file_count);
    for _ in 0..file_count {
        let path_len = usize::from(cursor.u16()?);
        if path_len == 0 || path_len > MAX_PATH_BYTES {
            return Err(BackupFormatError);
        }
        let path = std::str::from_utf8(cursor.take(path_len)?)
            .map_err(|_| BackupFormatError)?
            .to_owned();
        validate_path(&path)?;
        if !seen.insert(path.clone()) {
            return Err(BackupFormatError);
        }
        let length = usize::try_from(cursor.u64()?).map_err(|_| BackupFormatError)?;
        let expected_digest: [u8; 32] = cursor.array()?;
        let bytes = cursor.take(length)?.to_vec();
        if Sha256::digest(&bytes).as_slice() != expected_digest {
            return Err(BackupFormatError);
        }
        files.push(SnapshotFile { path, bytes });
    }
    cursor.finish()?;
    Ok(Snapshot::new(
        set_id,
        generation,
        created_at_millis,
        deletion_watermark,
        schema_version,
        files,
    ))
}

/// Serializes an ordered deletion head.
///
/// # Errors
///
/// Rejects non-monotonic IDs, invalid targets, and an overlong deletion log.
pub fn encode_deletion_head(head: &DeletionHead) -> Result<Vec<u8>, BackupFormatError> {
    if head.records.len() > MAX_DELETIONS {
        return Err(BackupFormatError);
    }
    let mut encoded = Vec::with_capacity(48 + head.records.len() * 25);
    encoded.extend_from_slice(DELETION_MAGIC);
    encoded.extend_from_slice(&head.set_id);
    encoded.extend_from_slice(&head.latest_generation.to_le_bytes());
    encoded.extend_from_slice(&head.updated_at_millis.to_le_bytes());
    encoded.extend_from_slice(
        &u32::try_from(head.records.len())
            .map_err(|_| BackupFormatError)?
            .to_le_bytes(),
    );
    let mut previous = 0;
    for record in &head.records {
        if record.intent_id == 0
            || record.intent_id <= previous
            || record.target_id == 0
            || record.target_kind > 1
        {
            return Err(BackupFormatError);
        }
        previous = record.intent_id;
        encoded.extend_from_slice(&record.intent_id.to_le_bytes());
        encoded.push(record.target_kind);
        encoded.extend_from_slice(&record.target_id.to_le_bytes());
        encoded.extend_from_slice(&record.requested_at_millis.to_le_bytes());
    }
    Ok(encoded)
}

/// Parses one complete ordered deletion head.
///
/// # Errors
///
/// Rejects malformed, non-monotonic, invalid, overlong, or trailing input.
pub fn decode_deletion_head(encoded: &[u8]) -> Result<DeletionHead, BackupFormatError> {
    let mut cursor = Cursor::new(encoded);
    cursor.expect(DELETION_MAGIC)?;
    let set_id = cursor.array()?;
    let latest_generation = cursor.u64()?;
    let updated_at_millis = cursor.i64()?;
    let record_count = usize::try_from(cursor.u32()?).map_err(|_| BackupFormatError)?;
    if record_count > MAX_DELETIONS {
        return Err(BackupFormatError);
    }
    let mut records = Vec::with_capacity(record_count);
    let mut previous = 0;
    for _ in 0..record_count {
        let intent_id = cursor.u64()?;
        let target_kind = cursor.u8()?;
        let target_id = cursor.u64()?;
        let requested_at_millis = cursor.i64()?;
        if intent_id == 0 || intent_id <= previous || target_kind > 1 || target_id == 0 {
            return Err(BackupFormatError);
        }
        previous = intent_id;
        records.push(DeletionRecord::new(
            intent_id,
            target_kind,
            target_id,
            requested_at_millis,
        ));
    }
    cursor.finish()?;
    Ok(DeletionHead::new(
        set_id,
        latest_generation,
        updated_at_millis,
        records,
    ))
}

struct ParsedEnvelope<'a> {
    kind: EnvelopeKind,
    metadata: &'a [u8],
    nonce: [u8; 24],
    ciphertext_offset: usize,
    ciphertext: &'a [u8],
}

fn parse_envelope(encoded: &[u8]) -> Result<ParsedEnvelope<'_>, BackupFormatError> {
    if encoded.len() < ENVELOPE_FIXED_BYTES + 16 || encoded.len() > MAX_PAYLOAD_BYTES {
        return Err(BackupFormatError);
    }
    let mut cursor = Cursor::new(encoded);
    cursor.expect(ENVELOPE_MAGIC)?;
    if cursor.u16()? != ENVELOPE_VERSION {
        return Err(BackupFormatError);
    }
    let kind = EnvelopeKind::decode(cursor.u8()?)?;
    if cursor.u8()? != 0 {
        return Err(BackupFormatError);
    }
    let metadata_len = usize::try_from(cursor.u32()?).map_err(|_| BackupFormatError)?;
    if metadata_len == 0 || metadata_len > MAX_PORTABLE_METADATA_BYTES {
        return Err(BackupFormatError);
    }
    let nonce = cursor.array()?;
    let ciphertext_len = usize::try_from(cursor.u64()?).map_err(|_| BackupFormatError)?;
    if ciphertext_len < 16 {
        return Err(BackupFormatError);
    }
    let metadata = cursor.take(metadata_len)?;
    let ciphertext_offset = cursor.position;
    let ciphertext = cursor.take(ciphertext_len)?;
    cursor.finish()?;
    Ok(ParsedEnvelope {
        kind,
        metadata,
        nonce,
        ciphertext_offset,
        ciphertext,
    })
}

fn validate_path(path: &str) -> Result<(), BackupFormatError> {
    if path.is_empty()
        || path.len() > MAX_PATH_BYTES
        || path.contains('\0')
        || path.starts_with('/')
        || path.starts_with('\\')
        || path.contains('\\')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || path.as_bytes().get(1) == Some(&b':')
    {
        return Err(BackupFormatError);
    }
    Ok(())
}

struct Cursor<'a> {
    input: &'a [u8],
    position: usize,
}

impl<'a> Cursor<'a> {
    const fn new(input: &'a [u8]) -> Self {
        Self { input, position: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], BackupFormatError> {
        let end = self.position.checked_add(length).ok_or(BackupFormatError)?;
        let value = self
            .input
            .get(self.position..end)
            .ok_or(BackupFormatError)?;
        self.position = end;
        Ok(value)
    }

    fn expect(&mut self, expected: &[u8]) -> Result<(), BackupFormatError> {
        if self.take(expected.len())? == expected {
            Ok(())
        } else {
            Err(BackupFormatError)
        }
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], BackupFormatError> {
        self.take(N)?.try_into().map_err(|_| BackupFormatError)
    }

    fn u8(&mut self) -> Result<u8, BackupFormatError> {
        Ok(self.array::<1>()?[0])
    }

    fn u16(&mut self) -> Result<u16, BackupFormatError> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    fn u32(&mut self) -> Result<u32, BackupFormatError> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    fn u64(&mut self) -> Result<u64, BackupFormatError> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    fn i64(&mut self) -> Result<i64, BackupFormatError> {
        Ok(i64::from_le_bytes(self.array()?))
    }

    fn finish(self) -> Result<(), BackupFormatError> {
        if self.position == self.input.len() {
            Ok(())
        } else {
            Err(BackupFormatError)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> BackupKey {
        BackupKey::new(Zeroizing::new([0x51; 32]))
    }

    #[test]
    fn envelope_rejects_truncation_tampering_and_trailing_bytes() {
        let encoded =
            seal_envelope(EnvelopeKind::Snapshot, b"portable", b"secret", &key()).unwrap();
        for candidate in [
            encoded[..encoded.len() - 1].to_vec(),
            {
                let mut value = encoded.clone();
                *value.last_mut().unwrap() ^= 1;
                value
            },
            {
                let mut value = encoded.clone();
                value.push(0);
                value
            },
        ] {
            assert!(open_envelope(&candidate, EnvelopeKind::Snapshot, &key()).is_err());
        }
    }

    #[test]
    fn snapshot_and_deletion_head_round_trip_exactly() {
        let snapshot = Snapshot::new(
            [1; 16],
            2,
            3,
            4,
            25,
            vec![SnapshotFile::new("self.db", vec![5, 6]).unwrap()],
        );
        assert_eq!(
            decode_snapshot(&encode_snapshot(&snapshot).unwrap()).unwrap(),
            snapshot
        );
        let head = DeletionHead::new([1; 16], 2, 7, vec![DeletionRecord::new(1, 0, 9, 8)]);
        assert_eq!(
            decode_deletion_head(&encode_deletion_head(&head).unwrap()).unwrap(),
            head
        );
    }
}
