use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use eam_backup::{
    DeletionHead, DeletionRecord, EnvelopeKind, Snapshot, SnapshotFile, decode_deletion_head,
    decode_snapshot, encode_deletion_head, encode_snapshot, inspect_envelope, open_envelope,
    seal_envelope,
};
use eam_core::{EvidenceId, ForgetTarget, Timestamp};
use tempfile::{Builder, NamedTempFile};

use crate::{RecoveryKey, VaultError, VaultKey, VaultKeyStore, VaultRepository};

const DELETION_HEAD_FILE: &str = "deletion-head.eam";
const RETAINED_SNAPSHOTS: usize = 3;
const MAX_ENVELOPE_BYTES: u64 = 1024 * 1024 * 1024;

pub struct BackupReceipt {
    generation: u64,
    snapshot_path: PathBuf,
    deletion_watermark: u64,
}

impl BackupReceipt {
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    pub fn snapshot_path(&self) -> &Path {
        &self.snapshot_path
    }

    #[must_use]
    pub const fn deletion_watermark(&self) -> u64 {
        self.deletion_watermark
    }
}

pub struct RestoreReceipt {
    generation: u64,
    replayed_deletions: usize,
    vault_key_rotated: bool,
}

impl RestoreReceipt {
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    pub const fn replayed_deletions(&self) -> usize {
        self.replayed_deletions
    }

    #[must_use]
    pub const fn vault_key_rotated(&self) -> bool {
        self.vault_key_rotated
    }
}

/// Creates and restores encrypted recovery sets inside the trusted Vault boundary.
pub struct VaultBackup;

impl VaultBackup {
    /// Creates one consistent encrypted snapshot and refreshes its deletion head.
    ///
    /// # Errors
    ///
    /// Rejects backup locations inside the vault, missing referenced objects,
    /// malformed existing heads, and any failed authenticated publication.
    pub fn create(
        repository: &mut VaultRepository,
        backup_set: impl AsRef<Path>,
        created_at_millis: i64,
    ) -> Result<BackupReceipt, VaultError> {
        let backup_set = prepare_backup_set(repository, backup_set.as_ref())?;
        let key = repository.backup_key()?;
        let metadata = VaultKeyStore::portable_metadata(repository.vault_root()?)?;
        let head_path = backup_set.join(DELETION_HEAD_FILE);
        let (set_id, previous_generation) = if head_path.exists() {
            let head = open_deletion_head_with_key(&head_path, &key)?;
            (head.set_id(), head.latest_generation())
        } else {
            (random_set_id()?, 0)
        };
        let generation = previous_generation
            .checked_add(1)
            .ok_or(VaultError::InvalidBackup)?;
        let records = deletion_records(repository)?;
        let deletion_watermark = records.last().map_or(0, DeletionRecord::intent_id);

        let temporary = Builder::new()
            .prefix(".snapshot-build-")
            .tempdir_in(&backup_set)?;
        let database_snapshot = temporary.path().join("self.db");
        let raw_files = repository.create_backup_snapshot(&database_snapshot)?;
        let files = raw_files
            .into_iter()
            .map(|(path, bytes)| SnapshotFile::new(path, bytes).map_err(invalid_backup))
            .collect::<Result<Vec<_>, _>>()?;
        let schema_version =
            u32::try_from(repository.schema_version()?).map_err(|_| VaultError::InvalidBackup)?;
        let snapshot = Snapshot::new(
            set_id,
            generation,
            created_at_millis,
            deletion_watermark,
            schema_version,
            files,
        );
        let snapshot_payload = encode_snapshot(&snapshot).map_err(invalid_backup)?;
        let snapshot_envelope =
            seal_envelope(EnvelopeKind::Snapshot, &metadata, &snapshot_payload, &key)
                .map_err(invalid_backup)?;

        let head = DeletionHead::new(set_id, generation, created_at_millis, records);
        let head_payload = encode_deletion_head(&head).map_err(invalid_backup)?;
        let head_envelope =
            seal_envelope(EnvelopeKind::DeletionHead, &metadata, &head_payload, &key)
                .map_err(invalid_backup)?;
        atomic_write(&head_path, &head_envelope)?;

        let snapshot_path = backup_set.join(format!("backup-{generation:020}.eambak"));
        publish_new(&snapshot_path, &snapshot_envelope)?;
        prune_snapshots(&backup_set)?;
        Ok(BackupReceipt {
            generation,
            snapshot_path,
            deletion_watermark,
        })
    }

    /// Refreshes the latest encrypted deletion state after a confirmed forget.
    ///
    /// # Errors
    ///
    /// Requires an existing head authenticated by the current Vault Key.
    pub fn synchronize_deletions(
        repository: &VaultRepository,
        backup_set: impl AsRef<Path>,
        updated_at_millis: i64,
    ) -> Result<(), VaultError> {
        let backup_set = prepare_backup_set(repository, backup_set.as_ref())?;
        let head_path = backup_set.join(DELETION_HEAD_FILE);
        if !head_path.is_file() {
            return Err(VaultError::InvalidBackup);
        }
        let key = repository.backup_key()?;
        let previous = open_deletion_head_with_key(&head_path, &key)?;
        let metadata = VaultKeyStore::portable_metadata(repository.vault_root()?)?;
        let head = DeletionHead::new(
            previous.set_id(),
            previous.latest_generation(),
            updated_at_millis,
            deletion_records(repository)?,
        );
        let payload = encode_deletion_head(&head).map_err(invalid_backup)?;
        let encoded = seal_envelope(EnvelopeKind::DeletionHead, &metadata, &payload, &key)
            .map_err(invalid_backup)?;
        atomic_write(&head_path, &encoded)
    }

    /// Restores into a new destination, replays the latest deletion head,
    /// rebuilds retrieval indexes, and rotates the Vault Key before publishing.
    ///
    /// # Errors
    ///
    /// Any invalid key, truncation, tampering, missing object, head mismatch, or
    /// staging failure leaves the destination unpublished.
    pub fn restore(
        snapshot_path: impl AsRef<Path>,
        deletion_head_path: impl AsRef<Path>,
        destination: impl AsRef<Path>,
        recovery_key: &RecoveryKey,
    ) -> Result<RestoreReceipt, VaultError> {
        let snapshot_path = snapshot_path.as_ref();
        let deletion_head_path = deletion_head_path.as_ref();
        let destination = destination.as_ref();
        if destination.exists() {
            return Err(VaultError::BackupDestinationExists);
        }
        let parent = destination.parent().ok_or(VaultError::InvalidBackup)?;
        fs::create_dir_all(parent)?;
        let snapshot_encoded = read_bounded(snapshot_path)?;
        let head_encoded = read_bounded(deletion_head_path)?;
        let snapshot = open_snapshot(&snapshot_encoded, recovery_key)?;
        let head = open_deletion_head(&head_encoded, recovery_key)?;
        validate_recovery_pair(&snapshot, &head)?;

        let staging = Builder::new().prefix(".eam-restore-").tempdir_in(parent)?;
        write_snapshot(staging.path(), &snapshot)?;
        let old_header = inspect_envelope(&snapshot_encoded).map_err(invalid_backup)?;
        let old_key =
            VaultKeyStore::unlock_recovery_metadata(old_header.portable_metadata(), recovery_key)
                .map_err(|_| VaultError::InvalidBackup)?;
        let mut repository = VaultRepository::open(staging.path(), old_key)
            .map_err(|_| VaultError::InvalidBackup)?;
        if repository.schema_version()? < i64::from(snapshot.schema_version()) {
            return Err(VaultError::InvalidBackup);
        }
        let replay_records = head
            .records()
            .iter()
            .filter(|record| record.intent_id() > snapshot.deletion_watermark())
            .copied()
            .collect::<Vec<_>>();
        for record in &replay_records {
            repository.replay_backup_deletion(
                record.intent_id(),
                decode_target(record)?,
                Timestamp::from_millis(record.requested_at_millis()),
            )?;
        }
        repository.rebuild_retrieval_after_restore()?;
        let new_key = VaultKey::generate()?;
        repository.rotate_after_restore(&new_key)?;
        repository.close()?;
        VaultKeyStore::install_rotated_metadata(staging.path(), &new_key, recovery_key)?;

        let verify_key = VaultKeyStore::unlock_recovery(staging.path(), recovery_key)
            .map_err(|_| VaultError::InvalidBackup)?;
        VaultRepository::open(staging.path(), verify_key)
            .map_err(|_| VaultError::InvalidBackup)?
            .close()?;

        let staging_path = staging.keep();
        if let Err(error) = fs::rename(&staging_path, destination) {
            let _ = fs::remove_dir_all(&staging_path);
            return Err(VaultError::Io(error));
        }
        Ok(RestoreReceipt {
            generation: snapshot.generation(),
            replayed_deletions: replay_records.len(),
            vault_key_rotated: true,
        })
    }
}

fn prepare_backup_set(
    repository: &VaultRepository,
    backup_set: &Path,
) -> Result<PathBuf, VaultError> {
    fs::create_dir_all(backup_set)?;
    let backup_set = fs::canonicalize(backup_set)?;
    let vault_root = fs::canonicalize(repository.vault_root()?)?;
    if backup_set.starts_with(&vault_root) {
        return Err(VaultError::InvalidBackup);
    }
    Ok(backup_set)
}

fn deletion_records(repository: &VaultRepository) -> Result<Vec<DeletionRecord>, VaultError> {
    repository
        .backup_deletion_records()?
        .into_iter()
        .map(|record| {
            Ok(DeletionRecord::new(
                record.intent_id,
                record.target_kind,
                record.target_id,
                record.requested_at_millis,
            ))
        })
        .collect()
}

fn open_snapshot(encoded: &[u8], recovery_key: &RecoveryKey) -> Result<Snapshot, VaultError> {
    let header = inspect_envelope(encoded).map_err(invalid_backup)?;
    if header.kind() != EnvelopeKind::Snapshot {
        return Err(VaultError::InvalidBackup);
    }
    let vault_key =
        VaultKeyStore::unlock_recovery_metadata(header.portable_metadata(), recovery_key)
            .map_err(|_| VaultError::InvalidBackup)?;
    let payload = open_envelope(encoded, EnvelopeKind::Snapshot, &vault_key.backup_key()?)
        .map_err(invalid_backup)?;
    decode_snapshot(&payload).map_err(invalid_backup)
}

fn open_deletion_head(
    encoded: &[u8],
    recovery_key: &RecoveryKey,
) -> Result<DeletionHead, VaultError> {
    let header = inspect_envelope(encoded).map_err(invalid_backup)?;
    if header.kind() != EnvelopeKind::DeletionHead {
        return Err(VaultError::InvalidBackup);
    }
    let vault_key =
        VaultKeyStore::unlock_recovery_metadata(header.portable_metadata(), recovery_key)
            .map_err(|_| VaultError::InvalidBackup)?;
    let payload = open_envelope(
        encoded,
        EnvelopeKind::DeletionHead,
        &vault_key.backup_key()?,
    )
    .map_err(invalid_backup)?;
    decode_deletion_head(&payload).map_err(invalid_backup)
}

fn open_deletion_head_with_key(
    path: &Path,
    key: &eam_backup::BackupKey,
) -> Result<DeletionHead, VaultError> {
    let encoded = read_bounded(path)?;
    let payload =
        open_envelope(&encoded, EnvelopeKind::DeletionHead, key).map_err(invalid_backup)?;
    decode_deletion_head(&payload).map_err(invalid_backup)
}

fn validate_recovery_pair(snapshot: &Snapshot, head: &DeletionHead) -> Result<(), VaultError> {
    let head_watermark = head.records().last().map_or(0, DeletionRecord::intent_id);
    if snapshot.set_id() != head.set_id()
        || head.latest_generation() < snapshot.generation()
        || head_watermark < snapshot.deletion_watermark()
    {
        return Err(VaultError::InvalidBackup);
    }
    Ok(())
}

fn write_snapshot(root: &Path, snapshot: &Snapshot) -> Result<(), VaultError> {
    let mut saw_database = false;
    fs::create_dir_all(root.join("objects"))?;
    for file in snapshot.files() {
        let destination = if file.path() == "self.db" {
            if saw_database {
                return Err(VaultError::InvalidBackup);
            }
            saw_database = true;
            root.join("self.db")
        } else if let Some(object_id) = file.path().strip_prefix("objects/") {
            if !valid_object_id(object_id) {
                return Err(VaultError::InvalidBackup);
            }
            root.join("objects").join(object_id)
        } else {
            return Err(VaultError::InvalidBackup);
        };
        let mut output = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(destination)?;
        output.write_all(file.bytes())?;
        output.sync_all()?;
    }
    if !saw_database {
        return Err(VaultError::InvalidBackup);
    }
    Ok(())
}

fn decode_target(record: &DeletionRecord) -> Result<ForgetTarget, VaultError> {
    match record.target_kind() {
        0 => Ok(ForgetTarget::ConversationEvidence(EvidenceId::from_raw(
            record.target_id(),
        ))),
        1 => Ok(ForgetTarget::ArchivedEvidence(record.target_id())),
        _ => Err(VaultError::InvalidBackup),
    }
}

fn random_set_id() -> Result<[u8; 16], VaultError> {
    let mut set_id = [0_u8; 16];
    getrandom::fill(&mut set_id).map_err(|_| VaultError::EntropyUnavailable)?;
    Ok(set_id)
}

fn valid_object_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, VaultError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| VaultError::InvalidBackup)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_ENVELOPE_BYTES
    {
        return Err(VaultError::InvalidBackup);
    }
    fs::read(path).map_err(|_| VaultError::InvalidBackup)
}

fn publish_new(path: &Path, bytes: &[u8]) -> Result<(), VaultError> {
    if path.exists() {
        return Err(VaultError::InvalidBackup);
    }
    let parent = path.parent().ok_or(VaultError::InvalidBackup)?;
    let mut pending = Builder::new()
        .prefix(".backup-pending-")
        .tempfile_in(parent)?;
    pending.write_all(bytes)?;
    pending.as_file().sync_all()?;
    pending
        .persist_noclobber(path)
        .map_err(|error| VaultError::Io(error.error))?;
    Ok(())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), VaultError> {
    let parent = path.parent().ok_or(VaultError::InvalidBackup)?;
    fs::create_dir_all(parent)?;
    let mut pending: NamedTempFile = Builder::new()
        .prefix(".head-pending-")
        .tempfile_in(parent)?;
    pending.write_all(bytes)?;
    pending.as_file().sync_all()?;
    pending
        .persist(path)
        .map_err(|error| VaultError::Io(error.error))?;
    Ok(())
}

fn prune_snapshots(backup_set: &Path) -> Result<(), VaultError> {
    let mut snapshots = fs::read_dir(backup_set)?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            (name.starts_with("backup-") && name.ends_with(".eambak"))
                .then_some((name, entry.path()))
        })
        .collect::<Vec<_>>();
    snapshots.sort_by(|left, right| left.0.cmp(&right.0));
    let remove_count = snapshots.len().saturating_sub(RETAINED_SNAPSHOTS);
    for (_, path) in snapshots.into_iter().take(remove_count) {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn invalid_backup<T>(_error: T) -> VaultError {
    VaultError::InvalidBackup
}
