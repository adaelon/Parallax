use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

use eam_core::{
    ApplicableTime, Claim, ClaimId, ClaimOwner, ConversationEvidence, EvidenceCitation, EvidenceId,
    MemoryRepository, RepositoryError, SessionId, Speaker, Timestamp, Uncertainty,
};
use eam_desktop_host::{
    ExitReason, HostGapId, HostGapReason, HostLifecycleRepository, HostRuntimeGap, HostSession,
    HostSessionId, HostSessionStart, LaunchMode,
};
use eam_identity::{
    IdentityProfile, IdentityRepository, IdentityStateVersion, InitialSelfIntroduction,
    IntroductionAnswer, IntroductionItem, SelfBundleRepository, SelfBundleState, SelfBundleVersion,
    SelfIntroductionCategory, WakeCommit, WakeExit, WakeTrigger,
};
use eam_ingestion::{
    AcceptedMarkdownSource, ArchiveInput, ArchiveReceipt, ArchiveRepository, ArchiveStatus,
    ArchivedEvidence, CanonicalEvidenceBlockSource, EvidenceBlock, EvidenceBlockDraft,
    EvidenceBlockId, EvidenceBlockMetadata, EvidenceBlockQueryRepository, EvidenceBlockRef,
    EvidenceExtractionRepository, ExtractionRevision, ExtractionRevisionId,
    MARKDOWN_LOCATOR_VERSION, MarkdownArchiveRepository, MarkdownLocator, MarkdownLocatorValue,
    MarkdownParseAttempt, MarkdownParseStart, MarkdownParseState, MaterializedExtraction,
    SourceAnchor, UnparsedReason, ValidatedExtraction,
};
use eam_markdown::{MarkdownBlockKind, ParseResource, ParsedMarkdownV1};
use fs4::{FileExt, TryLockError};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use sha2::{Digest, Sha256};

use crate::{
    VaultError, VaultKey, crypto::sqlcipher_key_pragma, object_store::ObjectStore, schema::migrate,
};

const DATABASE_FILE: &str = "self.db";
const WRITER_LOCK_FILE: &str = "self.db.writer.lock";

pub struct VaultRepository {
    connection: Option<Connection>,
    writer_lock: Option<File>,
    object_store: ObjectStore,
    vault_key: VaultKey,
    database_path: PathBuf,
    next_evidence_id: u64,
    next_claim_id: u64,
    next_archive_id: u64,
}

impl VaultRepository {
    /// Opens or creates the encrypted `self.db` under `vault_root`.
    ///
    /// The key is consumed by the trusted adapter. A non-blocking file lock
    /// rejects any second writer for the same vault root.
    ///
    /// # Errors
    ///
    /// Fails closed when the writer lock is held, `SQLCipher` is unavailable,
    /// the key is incorrect, page authentication fails, or migration fails.
    pub fn open(vault_root: impl AsRef<Path>, vault_key: VaultKey) -> Result<Self, VaultError> {
        let vault_root = vault_root.as_ref();
        fs::create_dir_all(vault_root)?;
        let writer_lock = acquire_writer_lock(&vault_root.join(WRITER_LOCK_FILE))?;
        let database_path = vault_root.join(DATABASE_FILE);
        let mut connection = Connection::open_with_flags(
            &database_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
        )?;

        key_connection(&connection, &vault_key)?;
        verify_sqlcipher(&connection)?;
        verify_key_and_pages(&connection)?;
        configure_connection(&connection)?;
        migrate(&mut connection)?;
        recover_interrupted_markdown_attempts(&mut connection)?;

        let object_store = ObjectStore::open(vault_root, vault_key.objects_key()?)?;
        object_store.cleanup_unreferenced(&referenced_object_ids(&connection)?)?;

        let next_evidence_id = next_identifier(&connection, "conversation_evidence")?;
        let next_claim_id = next_identifier(&connection, "claims")?;
        let next_archive_id = next_identifier(&connection, "archived_evidence")?;

        Ok(Self {
            connection: Some(connection),
            writer_lock: Some(writer_lock),
            object_store,
            vault_key,
            database_path,
            next_evidence_id,
            next_claim_id,
            next_archive_id,
        })
    }

    #[must_use]
    pub fn database_path(&self) -> &Path {
        &self.database_path
    }

    /// Reports the linked `SQLCipher` version after the connection is keyed.
    ///
    /// # Errors
    ///
    /// Returns an error if `SQLCipher` cannot answer the version pragma.
    pub fn sqlcipher_version(&self) -> Result<String, VaultError> {
        cipher_version(self.connection())
    }

    /// Reports the currently applied application schema version.
    ///
    /// # Errors
    ///
    /// Returns an error if the encrypted database cannot be queried.
    pub fn schema_version(&self) -> Result<i64, VaultError> {
        Ok(self
            .connection()
            .pragma_query_value(None, "user_version", |row| row.get(0))?)
    }

    /// Lists archived Context Inbox evidence without exposing object keys.
    ///
    /// # Errors
    ///
    /// Returns an error when encrypted metadata is unreadable or invalid.
    pub fn archived_evidence(&self) -> Result<Vec<ArchivedEvidence>, VaultError> {
        let mut statement = self.connection().prepare(
            "SELECT id, source_locator, content_length, status, unparsed_reason, archived_at
             FROM archived_evidence ORDER BY id",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(
                |(id, source_locator, content_length, status, reason, archived_at_millis)| {
                    Ok(ArchivedEvidence {
                        archive_id: u64::try_from(id)
                            .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                        source_locator,
                        content_length: u64::try_from(content_length)
                            .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                        status: decode_archive_status(status, reason)?,
                        archived_at_millis,
                    })
                },
            )
            .collect()
    }

    /// Decrypts one archived original inside the trusted Core boundary.
    ///
    /// # Errors
    ///
    /// Returns an error when the archive ID is missing, the object is absent,
    /// or authenticated decryption fails.
    pub fn read_archived_content(&self, archive_id: u64) -> Result<Vec<u8>, VaultError> {
        let object_id = self
            .connection()
            .query_row(
                "SELECT object_id FROM archived_evidence WHERE id = ?1",
                [to_vault_sql_id(archive_id)?],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or(VaultError::InvalidKeyOrCorrupt)?;
        self.object_store.read(&object_id)
    }

    /// Lists every versioned Markdown parse attempt in deterministic order.
    ///
    /// # Errors
    ///
    /// Returns an error when encrypted state cannot be decoded exactly.
    pub fn markdown_parse_attempts(&self) -> Result<Vec<MarkdownParseAttempt>, VaultError> {
        let mut statement = self.connection().prepare(
            "SELECT archive_id, parser_version, state, failure_reason,
                    started_at, finished_at
             FROM markdown_parse_attempts ORDER BY archive_id, parser_version",
        )?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(
                |(archive_id, parser_version, state, reason, started_at, finished_at)| {
                    Ok(MarkdownParseAttempt {
                        archive_id: u64::try_from(archive_id)
                            .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                        parser_version,
                        state: decode_markdown_parse_state(state)?,
                        failure_reason: reason.map(decode_unparsed_reason).transpose()?,
                        started_at_millis: started_at,
                        finished_at_millis: finished_at,
                    })
                },
            )
            .collect()
    }

    /// Restores one accepted encrypted Markdown parse artifact.
    ///
    /// # Errors
    ///
    /// Returns an error when the artifact is missing or its contract payload is
    /// corrupt.
    pub fn read_markdown_artifact(
        &self,
        archive_id: u64,
        parser_version: &str,
    ) -> Result<ParsedMarkdownV1, VaultError> {
        let encoded = self
            .connection()
            .query_row(
                "SELECT parsed_json FROM markdown_parse_artifacts
                 WHERE archive_id = ?1 AND parser_version = ?2",
                params![to_vault_sql_id(archive_id)?, parser_version],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or(VaultError::InvalidKeyOrCorrupt)?;
        serde_json::from_str(&encoded).map_err(|_| VaultError::InvalidKeyOrCorrupt)
    }

    /// Restores one complete immutable S10 extraction and its ordered blocks.
    ///
    /// # Errors
    ///
    /// Returns an error when encrypted rows, identifiers, source anchors, or
    /// the canonical archived Markdown are missing or corrupt.
    pub fn materialized_extraction(
        &self,
        evidence_id: u64,
        contract_version: &str,
    ) -> Result<Option<MaterializedExtraction>, VaultError> {
        let stored = self
            .connection()
            .query_row(
                "SELECT id, canonical_digest, accepted_at
                 FROM extraction_revisions
                 WHERE evidence_id = ?1 AND contract_version = ?2",
                params![to_vault_sql_id(evidence_id)?, contract_version],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()?;
        let Some((revision_id, canonical_digest, accepted_at_millis)) = stored else {
            return Ok(None);
        };
        let canonical_digest: [u8; 32] = canonical_digest
            .try_into()
            .map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
        let revision = ExtractionRevision::new(
            ExtractionRevisionId::new(
                u64::try_from(revision_id).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
            )
            .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
            evidence_id,
            contract_version.to_owned(),
            canonical_digest,
            accepted_at_millis,
        )
        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
        let canonical_bytes = self.read_archived_content(evidence_id)?;
        let actual_digest: [u8; 32] = Sha256::digest(&canonical_bytes).into();
        if actual_digest != *revision.canonical_digest() {
            return Err(VaultError::InvalidKeyOrCorrupt);
        }
        let canonical_text =
            std::str::from_utf8(&canonical_bytes).map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
        let blocks = load_evidence_blocks(self.connection(), &revision, canonical_text)?;
        MaterializedExtraction::new(revision, blocks, true)
            .map(Some)
            .map_err(|_| VaultError::InvalidKeyOrCorrupt)
    }

    /// Checkpoints encrypted WAL state, closes `SQLCipher`, clears the owned
    /// Vault Key, and releases the writer lock.
    ///
    /// # Errors
    ///
    /// Returns the first checkpoint, close, or unlock error after still
    /// attempting every cleanup action.
    pub fn close(mut self) -> Result<(), VaultError> {
        self.close_inner()
    }

    fn connection(&self) -> &Connection {
        self.connection
            .as_ref()
            .expect("an open vault always owns a database connection")
    }

    fn archive_with_hook<F>(
        &mut self,
        input: &ArchiveInput<'_>,
        before_commit: F,
    ) -> Result<ArchiveReceipt, VaultError>
    where
        F: FnOnce(&rusqlite::Transaction<'_>) -> Result<(), VaultError>,
    {
        if input.source_locator.is_empty() {
            return Err(VaultError::InvalidKeyOrCorrupt);
        }
        let content_length =
            i64::try_from(input.content.len()).map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
        let stored = self.object_store.store(input.content)?;
        let existing = self
            .connection()
            .query_row(
                "SELECT id, status, unparsed_reason FROM archived_evidence
                 WHERE source_kind = 0 AND source_locator = ?1 AND object_id = ?2",
                params![input.source_locator, stored.id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                    ))
                },
            )
            .optional()?;
        if let Some((archive_id, status, reason)) = existing {
            return Ok(ArchiveReceipt {
                archive_id: u64::try_from(archive_id)
                    .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                status: decode_archive_status(status, reason)?,
                object_reused: true,
                source_version_reused: true,
            });
        }

        let archive_id = self.next_archive_id;
        let (status, unparsed_reason) = encode_archive_status(input.status);
        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()?;
        transaction.execute(
            "INSERT INTO archived_evidence
             (id, source_kind, source_locator, object_id, content_length,
              status, unparsed_reason, archived_at)
             VALUES (?1, 0, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                to_vault_sql_id(archive_id)?,
                input.source_locator,
                stored.id,
                content_length,
                status,
                unparsed_reason,
                input.archived_at_millis,
            ],
        )?;
        before_commit(&transaction)?;
        transaction.commit()?;
        self.next_archive_id = archive_id
            .checked_add(1)
            .ok_or(VaultError::InvalidKeyOrCorrupt)?;
        Ok(ArchiveReceipt {
            archive_id,
            status: input.status,
            object_reused: stored.reused,
            source_version_reused: false,
        })
    }

    fn commit_extraction_with_hook<F>(
        &mut self,
        extraction: &ValidatedExtraction,
        before_commit: F,
    ) -> Result<MaterializedExtraction, VaultError>
    where
        F: FnOnce(&rusqlite::Transaction<'_>) -> Result<(), VaultError>,
    {
        if let Some(existing) =
            self.materialized_extraction(extraction.evidence_id(), extraction.contract_version())?
        {
            if materialized_matches(&existing, extraction) {
                return Ok(existing);
            }
            return Err(VaultError::InvalidKeyOrCorrupt);
        }

        let evidence_id_sql = to_vault_sql_id(extraction.evidence_id())?;
        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()?;
        let artifact_accepted_at = transaction
            .query_row(
                "SELECT a.accepted_at
                 FROM markdown_parse_artifacts a
                 JOIN markdown_parse_attempts p
                   ON p.archive_id = a.archive_id
                  AND p.parser_version = a.parser_version
                 WHERE a.archive_id = ?1 AND a.parser_version = ?2 AND p.state = 1",
                params![evidence_id_sql, extraction.contract_version()],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .ok_or(VaultError::InvalidKeyOrCorrupt)?;
        if artifact_accepted_at != extraction.accepted_at_millis() {
            return Err(VaultError::InvalidKeyOrCorrupt);
        }

        let revision_id =
            ExtractionRevisionId::new(next_identifier(&transaction, "extraction_revisions")?)
                .map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
        let revision = ExtractionRevision::new(
            revision_id,
            extraction.evidence_id(),
            extraction.contract_version().to_owned(),
            *extraction.canonical_digest(),
            extraction.accepted_at_millis(),
        )
        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
        transaction.execute(
            "INSERT INTO extraction_revisions
             (id, evidence_id, contract_version, canonical_digest, accepted_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                to_vault_sql_id(revision.id().get())?,
                evidence_id_sql,
                revision.contract_version(),
                revision.canonical_digest().as_slice(),
                revision.accepted_at_millis(),
            ],
        )?;

        let first_block_id = next_identifier(&transaction, "evidence_blocks")?;
        let mut assigned = HashMap::<u64, EvidenceBlockId>::new();
        let mut blocks = Vec::with_capacity(extraction.blocks().len());
        for (offset, draft) in extraction.blocks().iter().enumerate() {
            let block_id_raw = first_block_id
                .checked_add(u64::try_from(offset).map_err(|_| VaultError::InvalidKeyOrCorrupt)?)
                .ok_or(VaultError::InvalidKeyOrCorrupt)?;
            let block_id =
                EvidenceBlockId::new(block_id_raw).map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
            let parent_id = draft
                .parent_local_id()
                .map(|local_id| {
                    assigned
                        .get(&local_id)
                        .copied()
                        .ok_or(VaultError::InvalidKeyOrCorrupt)
                })
                .transpose()?;
            let block = insert_evidence_block(&transaction, &revision, block_id, parent_id, draft)?;
            assigned.insert(draft.local_id(), block_id);
            blocks.push(block);
        }
        before_commit(&transaction)?;
        transaction.commit()?;
        MaterializedExtraction::new(revision, blocks, false)
            .map_err(|_| VaultError::InvalidKeyOrCorrupt)
    }

    fn close_inner(&mut self) -> Result<(), VaultError> {
        let mut first_error = None;

        if let Some(connection) = self.connection.take() {
            if let Err(error) = connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);") {
                first_error = Some(VaultError::Sqlite(error));
            }
            if let Err((_connection, error)) = connection.close()
                && first_error.is_none()
            {
                first_error = Some(VaultError::Sqlite(error));
            }
        }

        self.vault_key.zeroize();
        self.object_store.zeroize();

        if let Some(writer_lock) = self.writer_lock.take()
            && let Err(error) = FileExt::unlock(&writer_lock)
            && first_error.is_none()
        {
            first_error = Some(VaultError::Io(error));
        }

        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

impl Drop for VaultRepository {
    fn drop(&mut self) {
        let _ = self.close_inner();
    }
}

impl ArchiveRepository for VaultRepository {
    type Error = VaultError;

    fn archive(&mut self, input: ArchiveInput<'_>) -> Result<ArchiveReceipt, Self::Error> {
        self.archive_with_hook(&input, |_| Ok(()))
    }
}

impl MarkdownArchiveRepository for VaultRepository {
    type Error = VaultError;

    fn begin_markdown_parse(
        &mut self,
        archive_id: u64,
        parser_version: &str,
        started_at_millis: i64,
    ) -> Result<MarkdownParseStart, Self::Error> {
        if parser_version.trim().is_empty() {
            return Err(VaultError::InvalidKeyOrCorrupt);
        }
        let archive_id_sql = to_vault_sql_id(archive_id)?;
        if let Some(state) = self
            .connection()
            .query_row(
                "SELECT state FROM markdown_parse_attempts
                 WHERE archive_id = ?1 AND parser_version = ?2",
                params![archive_id_sql, parser_version],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
        {
            return Ok(MarkdownParseStart::AlreadyAttempted(
                decode_markdown_parse_state(state)?,
            ));
        }

        let source_locator = self
            .connection()
            .query_row(
                "SELECT source_locator FROM archived_evidence WHERE id = ?1",
                [archive_id_sql],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or(VaultError::InvalidKeyOrCorrupt)?;
        if !std::path::Path::new(&source_locator)
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
        {
            return Err(VaultError::InvalidKeyOrCorrupt);
        }

        self.connection().execute(
            "INSERT INTO markdown_parse_attempts
             (archive_id, parser_version, state, failure_reason, started_at, finished_at)
             VALUES (?1, ?2, 0, NULL, ?3, NULL)",
            params![archive_id_sql, parser_version, started_at_millis],
        )?;
        Ok(MarkdownParseStart::Started)
    }

    fn read_archived_content(&self, archive_id: u64) -> Result<Vec<u8>, Self::Error> {
        VaultRepository::read_archived_content(self, archive_id)
    }

    fn accept_markdown_parse(
        &mut self,
        archive_id: u64,
        parser_version: &str,
        parsed: &ParsedMarkdownV1,
        finished_at_millis: i64,
    ) -> Result<(), Self::Error> {
        if parsed.contract_version != parser_version {
            return Err(VaultError::InvalidKeyOrCorrupt);
        }
        let encoded = serde_json::to_string(parsed).map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
        let archive_id_sql = to_vault_sql_id(archive_id)?;
        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()?;
        let changed = transaction.execute(
            "UPDATE markdown_parse_attempts
             SET state = 1, failure_reason = NULL, finished_at = ?1
             WHERE archive_id = ?2 AND parser_version = ?3 AND state = 0",
            params![finished_at_millis, archive_id_sql, parser_version],
        )?;
        if changed != 1 {
            return Err(VaultError::InvalidKeyOrCorrupt);
        }
        transaction.execute(
            "INSERT INTO markdown_parse_artifacts
             (archive_id, parser_version, parsed_json, accepted_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![archive_id_sql, parser_version, encoded, finished_at_millis],
        )?;
        transaction.execute(
            "UPDATE archived_evidence
             SET status = 2, unparsed_reason = NULL WHERE id = ?1",
            [archive_id_sql],
        )?;
        transaction.commit()?;
        Ok(())
    }

    fn reject_markdown_parse(
        &mut self,
        archive_id: u64,
        parser_version: &str,
        reason: UnparsedReason,
        finished_at_millis: i64,
    ) -> Result<(), Self::Error> {
        if matches!(
            reason,
            UnparsedReason::UnsupportedFormat | UnparsedReason::ParserInterrupted
        ) {
            return Err(VaultError::InvalidKeyOrCorrupt);
        }
        let archive_id_sql = to_vault_sql_id(archive_id)?;
        let reason_code = encode_unparsed_reason(reason);
        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()?;
        let changed = transaction.execute(
            "UPDATE markdown_parse_attempts
             SET state = 2, failure_reason = ?1, finished_at = ?2
             WHERE archive_id = ?3 AND parser_version = ?4 AND state = 0",
            params![
                reason_code,
                finished_at_millis,
                archive_id_sql,
                parser_version
            ],
        )?;
        if changed != 1 {
            return Err(VaultError::InvalidKeyOrCorrupt);
        }
        transaction.execute(
            "UPDATE archived_evidence
             SET status = 1, unparsed_reason = ?1 WHERE id = ?2",
            params![reason_code, archive_id_sql],
        )?;
        transaction.commit()?;
        Ok(())
    }
}

impl EvidenceExtractionRepository for VaultRepository {
    type Error = VaultError;

    fn load_accepted_markdown(
        &self,
        evidence_id: u64,
        contract_version: &str,
    ) -> Result<AcceptedMarkdownSource, Self::Error> {
        let (encoded, accepted_at_millis) = self
            .connection()
            .query_row(
                "SELECT a.parsed_json, a.accepted_at
                 FROM markdown_parse_artifacts a
                 JOIN markdown_parse_attempts p
                   ON p.archive_id = a.archive_id
                  AND p.parser_version = a.parser_version
                 WHERE a.archive_id = ?1 AND a.parser_version = ?2 AND p.state = 1",
                params![to_vault_sql_id(evidence_id)?, contract_version],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?
            .ok_or(VaultError::InvalidKeyOrCorrupt)?;
        let parsed = serde_json::from_str(&encoded).map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
        AcceptedMarkdownSource::new(
            evidence_id,
            self.read_archived_content(evidence_id)?,
            parsed,
            accepted_at_millis,
        )
        .map_err(|_| VaultError::InvalidKeyOrCorrupt)
    }

    fn commit_extraction(
        &mut self,
        extraction: &ValidatedExtraction,
    ) -> Result<MaterializedExtraction, Self::Error> {
        self.commit_extraction_with_hook(extraction, |_| Ok(()))
    }
}

impl EvidenceBlockQueryRepository for VaultRepository {
    type Error = VaultError;

    fn load_canonical_evidence_block(
        &self,
        reference: EvidenceBlockRef,
    ) -> Result<Option<CanonicalEvidenceBlockSource>, Self::Error> {
        let contract_version = self
            .connection()
            .query_row(
                "SELECT r.contract_version
                 FROM evidence_blocks b
                 JOIN extraction_revisions r ON r.id = b.extraction_revision_id
                 WHERE b.id = ?1 AND b.evidence_id = ?2",
                params![
                    to_vault_sql_id(reference.block_id().get())?,
                    to_vault_sql_id(reference.evidence_id())?,
                ],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(contract_version) = contract_version else {
            return Ok(None);
        };
        let materialized = self
            .materialized_extraction(reference.evidence_id(), &contract_version)?
            .ok_or(VaultError::InvalidKeyOrCorrupt)?;
        let block = materialized
            .blocks()
            .iter()
            .find(|block| block.id() == reference.block_id())
            .cloned()
            .ok_or(VaultError::InvalidKeyOrCorrupt)?;
        Ok(Some(CanonicalEvidenceBlockSource::new(
            block,
            self.read_archived_content(reference.evidence_id())?,
        )))
    }
}

impl MemoryRepository for VaultRepository {
    fn next_evidence_id(&mut self) -> EvidenceId {
        let id = EvidenceId::from_raw(self.next_evidence_id);
        self.next_evidence_id = self
            .next_evidence_id
            .checked_add(1)
            .expect("evidence identifier space exhausted");
        id
    }

    fn next_claim_id(&mut self) -> ClaimId {
        let id = ClaimId::from_raw(self.next_claim_id);
        self.next_claim_id = self
            .next_claim_id
            .checked_add(1)
            .expect("claim identifier space exhausted");
        id
    }

    fn append_evidence(&mut self, evidence: ConversationEvidence) -> Result<(), RepositoryError> {
        self.connection()
            .execute(
                "INSERT INTO conversation_evidence
                 (id, session_id, speaker, verbatim, recorded_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    to_sql_id(evidence.id().get())?,
                    evidence.session_id().as_str(),
                    encode_speaker(evidence.speaker()),
                    evidence.verbatim(),
                    evidence.recorded_at().as_millis(),
                ],
            )
            .map_err(repository_error)?;
        Ok(())
    }

    fn append_claim(&mut self, claim: Claim) -> Result<(), RepositoryError> {
        let (applicable_kind, applicable_start, applicable_end) =
            encode_applicable_time(claim.applicable_time());
        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()
            .map_err(repository_error)?;
        transaction
            .execute(
                "INSERT INTO claims
                 (id, owner, statement, uncertainty, applicable_kind,
                  applicable_start, applicable_end, recorded_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    to_sql_id(claim.id().get())?,
                    encode_owner(claim.owner()),
                    claim.statement(),
                    claim.uncertainty().map(encode_uncertainty),
                    applicable_kind,
                    applicable_start,
                    applicable_end,
                    claim.recorded_at().as_millis(),
                ],
            )
            .map_err(repository_error)?;

        for (ordinal, citation) in claim.support().iter().enumerate() {
            transaction
                .execute(
                    "INSERT INTO claim_support (claim_id, ordinal, evidence_id, quote)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![
                        to_sql_id(claim.id().get())?,
                        i64::try_from(ordinal).map_err(repository_error)?,
                        to_sql_id(citation.evidence_id().get())?,
                        citation.quote(),
                    ],
                )
                .map_err(repository_error)?;
        }
        transaction.commit().map_err(repository_error)?;
        Ok(())
    }

    fn evidence(&self, id: EvidenceId) -> Result<Option<ConversationEvidence>, RepositoryError> {
        let stored = self
            .connection()
            .query_row(
                "SELECT id, session_id, speaker, verbatim, recorded_at
                 FROM conversation_evidence WHERE id = ?1",
                [to_sql_id(id.get())?],
                stored_evidence_from_row,
            )
            .optional()
            .map_err(repository_error)?;
        stored.map(StoredEvidence::decode).transpose()
    }

    fn all_evidence(&self) -> Result<Vec<ConversationEvidence>, RepositoryError> {
        let mut statement = self
            .connection()
            .prepare(
                "SELECT id, session_id, speaker, verbatim, recorded_at
                 FROM conversation_evidence ORDER BY id",
            )
            .map_err(repository_error)?;
        let stored = statement
            .query_map([], stored_evidence_from_row)
            .map_err(repository_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(repository_error)?;
        stored.into_iter().map(StoredEvidence::decode).collect()
    }

    fn all_claims(&self) -> Result<Vec<Claim>, RepositoryError> {
        let stored_claims = {
            let mut statement = self
                .connection()
                .prepare(
                    "SELECT id, owner, statement, uncertainty, applicable_kind,
                            applicable_start, applicable_end, recorded_at
                     FROM claims ORDER BY id",
                )
                .map_err(repository_error)?;
            statement
                .query_map([], stored_claim_from_row)
                .map_err(repository_error)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(repository_error)?
        };

        stored_claims
            .into_iter()
            .map(|stored| stored.decode(self.connection()))
            .collect()
    }
}

impl HostLifecycleRepository for VaultRepository {
    fn begin_host_session(
        &mut self,
        started_at: Timestamp,
        launch_mode: LaunchMode,
    ) -> Result<HostSessionStart, RepositoryError> {
        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()
            .map_err(repository_error)?;
        let previous = transaction
            .query_row(
                "SELECT id, launch_mode, started_at, last_seen_at, ended_at, end_reason
                 FROM host_sessions ORDER BY id DESC LIMIT 1",
                [],
                stored_host_session_from_row,
            )
            .optional()
            .map_err(repository_error)?;
        let session_id =
            HostSessionId::from_raw(next_host_identifier(&transaction, "host_sessions")?);
        transaction
            .execute(
                "INSERT INTO host_sessions
                 (id, launch_mode, started_at, last_seen_at, ended_at, end_reason)
                 VALUES (?1, ?2, ?3, ?3, NULL, NULL)",
                params![
                    to_sql_id(session_id.get())?,
                    encode_launch_mode(launch_mode),
                    started_at.as_millis(),
                ],
            )
            .map_err(repository_error)?;

        let recovered_gap = previous
            .map(StoredHostSession::decode)
            .transpose()?
            .and_then(|previous| recovered_gap_spec(&previous, started_at))
            .map(|(from, to, reason, clock_rollback)| {
                let gap_id =
                    HostGapId::from_raw(next_host_identifier(&transaction, "host_runtime_gaps")?);
                transaction
                    .execute(
                        "INSERT INTO host_runtime_gaps
                         (id, from_at, to_at, reason, clock_rollback,
                          recovered_by_session_id)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                        params![
                            to_sql_id(gap_id.get())?,
                            from.as_millis(),
                            to.as_millis(),
                            encode_gap_reason(reason),
                            i64::from(clock_rollback),
                            to_sql_id(session_id.get())?,
                        ],
                    )
                    .map_err(repository_error)?;
                Ok(HostRuntimeGap::restore(
                    gap_id,
                    from,
                    to,
                    reason,
                    clock_rollback,
                    session_id,
                ))
            })
            .transpose()?;

        transaction.commit().map_err(repository_error)?;
        Ok(HostSessionStart::new(
            HostSession::restore(session_id, launch_mode, started_at, started_at, None, None),
            recovered_gap,
        ))
    }

    fn heartbeat_host_session(
        &mut self,
        session_id: HostSessionId,
        observed_at: Timestamp,
    ) -> Result<HostSession, RepositoryError> {
        let current = current_host_session(self.connection())?
            .ok_or_else(|| RepositoryError::new("host session is not initialized"))?;
        if current.id() != session_id || current.ended_at().is_some() {
            return Err(RepositoryError::new(
                "heartbeat must target the current open host session",
            ));
        }
        let last_seen_at = std::cmp::max(current.last_seen_at(), observed_at);
        self.connection()
            .execute(
                "UPDATE host_sessions SET last_seen_at = ?1 WHERE id = ?2",
                params![last_seen_at.as_millis(), to_sql_id(session_id.get())?],
            )
            .map_err(repository_error)?;
        Ok(HostSession::restore(
            current.id(),
            current.launch_mode(),
            current.started_at(),
            last_seen_at,
            None,
            None,
        ))
    }

    fn finish_host_session(
        &mut self,
        session_id: HostSessionId,
        ended_at: Timestamp,
        reason: ExitReason,
    ) -> Result<HostSession, RepositoryError> {
        let current = current_host_session(self.connection())?
            .ok_or_else(|| RepositoryError::new("host session is not initialized"))?;
        if current.id() != session_id || current.ended_at().is_some() {
            return Err(RepositoryError::new(
                "finish must target the current open host session",
            ));
        }
        let ended_at = std::cmp::max(current.last_seen_at(), ended_at);
        self.connection()
            .execute(
                "UPDATE host_sessions
                 SET last_seen_at = ?1, ended_at = ?1, end_reason = ?2
                 WHERE id = ?3",
                params![
                    ended_at.as_millis(),
                    encode_exit_reason(reason),
                    to_sql_id(session_id.get())?,
                ],
            )
            .map_err(repository_error)?;
        Ok(HostSession::restore(
            current.id(),
            current.launch_mode(),
            current.started_at(),
            ended_at,
            Some(ended_at),
            Some(reason),
        ))
    }

    fn all_host_sessions(&self) -> Result<Vec<HostSession>, RepositoryError> {
        let mut statement = self
            .connection()
            .prepare(
                "SELECT id, launch_mode, started_at, last_seen_at, ended_at, end_reason
                 FROM host_sessions ORDER BY id",
            )
            .map_err(repository_error)?;
        statement
            .query_map([], stored_host_session_from_row)
            .map_err(repository_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(repository_error)?
            .into_iter()
            .map(StoredHostSession::decode)
            .collect()
    }

    fn all_host_runtime_gaps(&self) -> Result<Vec<HostRuntimeGap>, RepositoryError> {
        let mut statement = self
            .connection()
            .prepare(
                "SELECT id, from_at, to_at, reason, clock_rollback,
                        recovered_by_session_id
                 FROM host_runtime_gaps ORDER BY id",
            )
            .map_err(repository_error)?;
        statement
            .query_map([], stored_host_gap_from_row)
            .map_err(repository_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(repository_error)?
            .into_iter()
            .map(StoredHostGap::decode)
            .collect()
    }
}

impl IdentityRepository for VaultRepository {
    fn record_initial_self_introduction(
        &mut self,
        session_id: &SessionId,
        answers: &[IntroductionAnswer],
        recorded_at: Timestamp,
    ) -> Result<InitialSelfIntroduction, RepositoryError> {
        let existing: i64 = self
            .connection()
            .query_row(
                "SELECT count(*) FROM initial_self_introduction",
                [],
                |row| row.get(0),
            )
            .map_err(repository_error)?;
        if existing != 0 {
            return Err(RepositoryError::new(
                "initial self introduction already exists",
            ));
        }

        let mut next_evidence_id = self.next_evidence_id;
        let mut next_claim_id = self.next_claim_id;
        let mut items = Vec::with_capacity(SelfIntroductionCategory::ALL.len());
        for category in SelfIntroductionCategory::ALL {
            let answer = answers
                .iter()
                .find(|answer| answer.category() == category)
                .ok_or_else(|| RepositoryError::new("validated introduction category missing"))?;
            let evidence_id = EvidenceId::from_raw(next_evidence_id);
            let claim_id = ClaimId::from_raw(next_claim_id);
            next_evidence_id = next_evidence_id
                .checked_add(1)
                .ok_or_else(|| RepositoryError::new("evidence identifier space exhausted"))?;
            next_claim_id = next_claim_id
                .checked_add(1)
                .ok_or_else(|| RepositoryError::new("claim identifier space exhausted"))?;
            items.push(IntroductionItem::restore(
                category,
                evidence_id,
                claim_id,
                answer.statement(),
                recorded_at,
            ));
        }

        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()
            .map_err(repository_error)?;
        for item in &items {
            transaction
                .execute(
                    "INSERT INTO conversation_evidence
                     (id, session_id, speaker, verbatim, recorded_at)
                     VALUES (?1, ?2, 0, ?3, ?4)",
                    params![
                        to_sql_id(item.evidence_id().get())?,
                        session_id.as_str(),
                        item.statement(),
                        item.recorded_at().as_millis(),
                    ],
                )
                .map_err(repository_error)?;
            transaction
                .execute(
                    "INSERT INTO claims
                     (id, owner, statement, uncertainty, applicable_kind,
                      applicable_start, applicable_end, recorded_at)
                     VALUES (?1, 0, ?2, NULL, 0, ?3, NULL, ?3)",
                    params![
                        to_sql_id(item.claim_id().get())?,
                        item.statement(),
                        item.recorded_at().as_millis(),
                    ],
                )
                .map_err(repository_error)?;
            transaction
                .execute(
                    "INSERT INTO claim_support (claim_id, ordinal, evidence_id, quote)
                     VALUES (?1, 0, ?2, ?3)",
                    params![
                        to_sql_id(item.claim_id().get())?,
                        to_sql_id(item.evidence_id().get())?,
                        item.statement(),
                    ],
                )
                .map_err(repository_error)?;
            transaction
                .execute(
                    "INSERT INTO initial_self_introduction (category, evidence_id, claim_id)
                     VALUES (?1, ?2, ?3)",
                    params![
                        item.category().code(),
                        to_sql_id(item.evidence_id().get())?,
                        to_sql_id(item.claim_id().get())?,
                    ],
                )
                .map_err(repository_error)?;
        }
        transaction.commit().map_err(repository_error)?;
        self.next_evidence_id = next_evidence_id;
        self.next_claim_id = next_claim_id;
        Ok(InitialSelfIntroduction::restore(session_id.clone(), items))
    }

    fn initial_self_introduction(
        &self,
    ) -> Result<Option<InitialSelfIntroduction>, RepositoryError> {
        let mut statement = self
            .connection()
            .prepare(
                "SELECT e.session_id, i.category, i.evidence_id, i.claim_id,
                        e.verbatim, e.recorded_at
                 FROM initial_self_introduction i
                 JOIN conversation_evidence e ON e.id = i.evidence_id
                 ORDER BY i.category",
            )
            .map_err(repository_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            })
            .map_err(repository_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(repository_error)?;
        if rows.is_empty() {
            return Ok(None);
        }
        if rows.len() != SelfIntroductionCategory::ALL.len() {
            return Err(RepositoryError::new(
                "persisted initial self introduction is incomplete",
            ));
        }

        let session_id = rows[0].0.clone();
        let mut items = Vec::with_capacity(rows.len());
        for (stored_session, category, evidence_id, claim_id, verbatim, recorded_at) in rows {
            if stored_session != session_id {
                return Err(RepositoryError::new(
                    "persisted initial self introduction spans multiple sessions",
                ));
            }
            let category = SelfIntroductionCategory::from_code(category)
                .ok_or_else(|| RepositoryError::new("invalid introduction category"))?;
            items.push(IntroductionItem::restore(
                category,
                EvidenceId::from_raw(u64::try_from(evidence_id).map_err(repository_error)?),
                ClaimId::from_raw(u64::try_from(claim_id).map_err(repository_error)?),
                verbatim,
                Timestamp::from_millis(recorded_at),
            ));
        }
        Ok(Some(InitialSelfIntroduction::restore(
            SessionId::new(session_id),
            items,
        )))
    }

    fn append_identity_state(
        &mut self,
        identity: IdentityStateVersion,
    ) -> Result<(), RepositoryError> {
        let version = to_sql_id(identity.version())?;
        let predecessor_version = identity.predecessor_version().map(to_sql_id).transpose()?;
        let profile = identity.profile();
        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()
            .map_err(repository_error)?;
        transaction
            .execute(
                "INSERT INTO identity_state_versions
                 (version, predecessor_version, name, expression_traits, viewpoints,
                  value_priorities, relationship_posture, own_goals, change_reason, formed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    version,
                    predecessor_version,
                    profile.name(),
                    profile.expression_traits(),
                    profile.viewpoints(),
                    profile.value_priorities(),
                    profile.relationship_posture(),
                    profile.own_goals(),
                    identity.change_reason(),
                    identity.formed_at().as_millis(),
                ],
            )
            .map_err(repository_error)?;
        for (ordinal, evidence_id) in identity.evidence_refs().iter().enumerate() {
            transaction
                .execute(
                    "INSERT INTO identity_state_evidence
                     (identity_version, ordinal, evidence_id) VALUES (?1, ?2, ?3)",
                    params![
                        version,
                        i64::try_from(ordinal).map_err(repository_error)?,
                        to_sql_id(evidence_id.get())?,
                    ],
                )
                .map_err(repository_error)?;
        }
        transaction.commit().map_err(repository_error)?;
        Ok(())
    }

    fn current_identity_state(&self) -> Result<Option<IdentityStateVersion>, RepositoryError> {
        let stored = self
            .connection()
            .query_row(
                "SELECT version, predecessor_version, name, expression_traits, viewpoints,
                        value_priorities, relationship_posture, own_goals, change_reason, formed_at
                 FROM identity_state_versions ORDER BY version DESC LIMIT 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, Option<i64>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, String>(8)?,
                        row.get::<_, i64>(9)?,
                    ))
                },
            )
            .optional()
            .map_err(repository_error)?;
        let Some((
            version,
            predecessor_version,
            name,
            expression_traits,
            viewpoints,
            value_priorities,
            relationship_posture,
            own_goals,
            change_reason,
            formed_at,
        )) = stored
        else {
            return Ok(None);
        };
        let version = u64::try_from(version).map_err(repository_error)?;
        let predecessor_version = predecessor_version
            .map(u64::try_from)
            .transpose()
            .map_err(repository_error)?;
        let evidence_refs = load_identity_evidence(self.connection(), version)?;
        Ok(Some(IdentityStateVersion::restore(
            version,
            predecessor_version,
            IdentityProfile::new(
                name,
                expression_traits,
                viewpoints,
                value_priorities,
                relationship_posture,
                own_goals,
            ),
            change_reason,
            evidence_refs,
            Timestamp::from_millis(formed_at),
        )))
    }
}

impl SelfBundleRepository for VaultRepository {
    fn append_self_bundle(&mut self, bundle: SelfBundleVersion) -> Result<(), RepositoryError> {
        validate_self_bundle_chain(self.connection(), &bundle)?;
        let (wake_trigger, wake_exit) = encode_wake_commit(bundle.wake_commit())?;
        let version = to_sql_id(bundle.version())?;
        let state = bundle.state();
        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()
            .map_err(repository_error)?;
        transaction
            .execute(
                "INSERT INTO self_bundle_versions
                 (version, predecessor_version, constitution_version, identity_state_version,
                  relationship_state, wake_trigger, wake_exit, committed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    version,
                    bundle.predecessor_version().map(to_sql_id).transpose()?,
                    to_sql_id(state.constitution_version())?,
                    to_sql_id(state.identity_state_version())?,
                    state.relationship_state(),
                    wake_trigger,
                    wake_exit,
                    bundle.committed_at().as_millis(),
                ],
            )
            .map_err(repository_error)?;
        insert_self_bundle_children(&transaction, version, state)?;
        transaction.commit().map_err(repository_error)?;
        Ok(())
    }

    fn current_self_bundle(&self) -> Result<Option<SelfBundleVersion>, RepositoryError> {
        let stored = self
            .connection()
            .query_row(
                "SELECT version, predecessor_version, constitution_version,
                        identity_state_version, relationship_state, wake_trigger,
                        wake_exit, committed_at
                 FROM self_bundle_versions ORDER BY version DESC LIMIT 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, Option<i64>>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<i64>>(5)?,
                        row.get::<_, Option<i64>>(6)?,
                        row.get::<_, i64>(7)?,
                    ))
                },
            )
            .optional()
            .map_err(repository_error)?;
        let Some((
            version,
            predecessor_version,
            constitution_version,
            identity_state_version,
            relationship_state,
            wake_trigger,
            wake_exit,
            committed_at,
        )) = stored
        else {
            return Ok(None);
        };
        let version = u64::try_from(version).map_err(repository_error)?;
        let predecessor_version = predecessor_version
            .map(u64::try_from)
            .transpose()
            .map_err(repository_error)?;
        let wake_commit = decode_wake_commit(wake_trigger, wake_exit)?;
        let state = SelfBundleState::new(
            u64::try_from(constitution_version).map_err(repository_error)?,
            u64::try_from(identity_state_version).map_err(repository_error)?,
            load_self_bundle_experiences(self.connection(), version)?,
            load_self_bundle_beliefs(self.connection(), version)?,
            relationship_state,
            load_self_bundle_intentions(self.connection(), version)?,
        )
        .map_err(repository_error)?;

        Ok(Some(SelfBundleVersion::restore(
            version,
            predecessor_version,
            state,
            wake_commit,
            Timestamp::from_millis(committed_at),
        )))
    }
}

fn validate_self_bundle_chain(
    connection: &Connection,
    bundle: &SelfBundleVersion,
) -> Result<(), RepositoryError> {
    let current_version = connection
        .query_row("SELECT MAX(version) FROM self_bundle_versions", [], |row| {
            row.get::<_, Option<i64>>(0)
        })
        .map_err(repository_error)?
        .map(u64::try_from)
        .transpose()
        .map_err(repository_error)?;

    match current_version {
        None if bundle.version() == 1
            && bundle.predecessor_version().is_none()
            && bundle.wake_commit().is_none() =>
        {
            Ok(())
        }
        None => Err(RepositoryError::new(
            "initial Self Bundle must be version 1 without predecessor or wake commit",
        )),
        Some(current) => {
            let expected = current
                .checked_add(1)
                .ok_or_else(|| RepositoryError::new("Self Bundle version space exhausted"))?;
            if bundle.version() == expected
                && bundle.predecessor_version() == Some(current)
                && bundle.wake_commit().is_some()
            {
                Ok(())
            } else {
                Err(RepositoryError::new(
                    "Self Bundle version does not continue the current immutable chain",
                ))
            }
        }
    }
}

fn encode_wake_commit(
    wake_commit: Option<WakeCommit>,
) -> Result<(Option<i64>, Option<i64>), RepositoryError> {
    match wake_commit {
        None => Ok((None, None)),
        Some(commit) => Ok((
            Some(commit.trigger().code()),
            Some(
                commit
                    .exit()
                    .code()
                    .ok_or_else(|| RepositoryError::new("invalid persisted wake exit"))?,
            ),
        )),
    }
}

fn insert_self_bundle_children(
    transaction: &rusqlite::Transaction<'_>,
    version: i64,
    state: &SelfBundleState,
) -> Result<(), RepositoryError> {
    for (ordinal, experience_ref) in state.counterpart_experience_refs().iter().enumerate() {
        transaction
            .execute(
                "INSERT INTO self_bundle_experiences
                 (bundle_version, ordinal, experience_ref) VALUES (?1, ?2, ?3)",
                params![
                    version,
                    i64::try_from(ordinal).map_err(repository_error)?,
                    experience_ref,
                ],
            )
            .map_err(repository_error)?;
    }
    for (ordinal, belief_ref) in state.belief_refs().iter().enumerate() {
        transaction
            .execute(
                "INSERT INTO self_bundle_beliefs
                 (bundle_version, ordinal, claim_id) VALUES (?1, ?2, ?3)",
                params![
                    version,
                    i64::try_from(ordinal).map_err(repository_error)?,
                    to_sql_id(belief_ref.get())?,
                ],
            )
            .map_err(repository_error)?;
    }
    for (ordinal, intention) in state.pending_intentions().iter().enumerate() {
        transaction
            .execute(
                "INSERT INTO self_bundle_pending_intentions
                 (bundle_version, ordinal, intention) VALUES (?1, ?2, ?3)",
                params![
                    version,
                    i64::try_from(ordinal).map_err(repository_error)?,
                    intention,
                ],
            )
            .map_err(repository_error)?;
    }
    Ok(())
}

fn acquire_writer_lock(path: &Path) -> Result<File, VaultError> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?;
    match FileExt::try_lock(&file) {
        Ok(()) => Ok(file),
        Err(TryLockError::WouldBlock) => Err(VaultError::AlreadyOpen),
        Err(TryLockError::Error(error)) => Err(VaultError::Io(error)),
    }
}

fn key_connection(connection: &Connection, vault_key: &VaultKey) -> Result<(), VaultError> {
    let database_key = vault_key.database_key()?;
    let statement = sqlcipher_key_pragma(&database_key);
    connection
        .execute_batch(&statement)
        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
    connection.execute_batch("PRAGMA cipher_log_level = NONE;")?;
    Ok(())
}

fn verify_sqlcipher(connection: &Connection) -> Result<(), VaultError> {
    cipher_version(connection).map(|_| ())
}

fn cipher_version(connection: &Connection) -> Result<String, VaultError> {
    let version = connection
        .pragma_query_value(None, "cipher_version", |row| row.get::<_, String>(0))
        .map_err(|_| VaultError::CipherUnavailable)?;
    if version.trim().is_empty() {
        return Err(VaultError::CipherUnavailable);
    }
    Ok(version)
}

fn verify_key_and_pages(connection: &Connection) -> Result<(), VaultError> {
    connection
        .query_row("SELECT count(*) FROM sqlite_schema", [], |_| Ok(()))
        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?;

    let mut statement = connection
        .prepare("PRAGMA cipher_integrity_check")
        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
    let mut rows = statement
        .query([])
        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
    if rows
        .next()
        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?
        .is_some()
    {
        return Err(VaultError::InvalidKeyOrCorrupt);
    }
    Ok(())
}

fn configure_connection(connection: &Connection) -> Result<(), VaultError> {
    connection.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA trusted_schema = OFF;
         PRAGMA secure_delete = ON;
         PRAGMA temp_store = MEMORY;
         PRAGMA journal_mode = WAL;
         PRAGMA synchronous = FULL;",
    )?;
    Ok(())
}

fn recover_interrupted_markdown_attempts(connection: &mut Connection) -> Result<(), VaultError> {
    let transaction = connection.transaction()?;
    transaction.execute(
        "UPDATE archived_evidence
         SET status = 1, unparsed_reason = 8
         WHERE id IN (
             SELECT archive_id FROM markdown_parse_attempts WHERE state = 0
         )",
        [],
    )?;
    transaction.execute(
        "UPDATE markdown_parse_attempts
         SET state = 3, failure_reason = 8, finished_at = NULL
         WHERE state = 0",
        [],
    )?;
    transaction.commit()?;
    Ok(())
}

fn referenced_object_ids(connection: &Connection) -> Result<HashSet<String>, VaultError> {
    let mut statement = connection.prepare("SELECT DISTINCT object_id FROM archived_evidence")?;
    statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<HashSet<_>, _>>()
        .map_err(VaultError::from)
}

const fn encode_archive_status(status: ArchiveStatus) -> (i64, Option<i64>) {
    match status {
        ArchiveStatus::Archived => (0, None),
        ArchiveStatus::ArchivedUnparsed(reason) => (1, Some(encode_unparsed_reason(reason))),
        ArchiveStatus::Extracted => (2, None),
    }
}

fn decode_archive_status(
    status: i64,
    unparsed_reason: Option<i64>,
) -> Result<ArchiveStatus, VaultError> {
    match (status, unparsed_reason) {
        (0, None) => Ok(ArchiveStatus::Archived),
        (1, Some(reason)) => Ok(ArchiveStatus::ArchivedUnparsed(decode_unparsed_reason(
            reason,
        )?)),
        (2, None) => Ok(ArchiveStatus::Extracted),
        _ => Err(VaultError::InvalidKeyOrCorrupt),
    }
}

const fn encode_unparsed_reason(reason: UnparsedReason) -> i64 {
    match reason {
        UnparsedReason::UnsupportedFormat => 0,
        UnparsedReason::InvalidEncoding => 1,
        UnparsedReason::ResourceLimit(ParseResource::SourceBytes) => 2,
        UnparsedReason::ResourceLimit(ParseResource::Blocks) => 3,
        UnparsedReason::ResourceLimit(ParseResource::NestingDepth) => 4,
        UnparsedReason::ResourceLimit(ParseResource::MetadataBytes) => 5,
        UnparsedReason::ResourceLimit(ParseResource::Links) => 6,
        UnparsedReason::InvalidStructure => 7,
        UnparsedReason::ParserInterrupted => 8,
    }
}

fn decode_unparsed_reason(value: i64) -> Result<UnparsedReason, VaultError> {
    match value {
        0 => Ok(UnparsedReason::UnsupportedFormat),
        1 => Ok(UnparsedReason::InvalidEncoding),
        2 => Ok(UnparsedReason::ResourceLimit(ParseResource::SourceBytes)),
        3 => Ok(UnparsedReason::ResourceLimit(ParseResource::Blocks)),
        4 => Ok(UnparsedReason::ResourceLimit(ParseResource::NestingDepth)),
        5 => Ok(UnparsedReason::ResourceLimit(ParseResource::MetadataBytes)),
        6 => Ok(UnparsedReason::ResourceLimit(ParseResource::Links)),
        7 => Ok(UnparsedReason::InvalidStructure),
        8 => Ok(UnparsedReason::ParserInterrupted),
        _ => Err(VaultError::InvalidKeyOrCorrupt),
    }
}

const fn decode_markdown_parse_state(value: i64) -> Result<MarkdownParseState, VaultError> {
    match value {
        0 => Ok(MarkdownParseState::Started),
        1 => Ok(MarkdownParseState::Accepted),
        2 => Ok(MarkdownParseState::Rejected),
        3 => Ok(MarkdownParseState::Interrupted),
        _ => Err(VaultError::InvalidKeyOrCorrupt),
    }
}

fn materialized_matches(
    materialized: &MaterializedExtraction,
    extraction: &ValidatedExtraction,
) -> bool {
    let revision = materialized.revision();
    if revision.evidence_id() != extraction.evidence_id()
        || revision.contract_version() != extraction.contract_version()
        || revision.canonical_digest() != extraction.canonical_digest()
        || revision.accepted_at_millis() != extraction.accepted_at_millis()
        || materialized.blocks().len() != extraction.blocks().len()
    {
        return false;
    }
    let assigned = extraction
        .blocks()
        .iter()
        .zip(materialized.blocks())
        .map(|(draft, block)| (draft.local_id(), block.id()))
        .collect::<HashMap<_, _>>();
    extraction
        .blocks()
        .iter()
        .zip(materialized.blocks())
        .all(|(draft, block)| {
            let expected_parent = draft
                .parent_local_id()
                .and_then(|local_id| assigned.get(&local_id).copied());
            block.parent_id() == expected_parent
                && block.ordinal() == draft.ordinal()
                && block.kind() == draft.kind()
                && block.anchor() == draft.anchor()
                && block.metadata() == draft.metadata()
        })
}

fn insert_evidence_block(
    transaction: &rusqlite::Transaction<'_>,
    revision: &ExtractionRevision,
    block_id: EvidenceBlockId,
    parent_id: Option<EvidenceBlockId>,
    draft: &EvidenceBlockDraft,
) -> Result<EvidenceBlock, VaultError> {
    let (locator_version, locator_kind, locator_value) =
        encode_markdown_locator(draft.anchor().native_locator());
    let metadata = draft.metadata();
    transaction.execute(
        "INSERT INTO evidence_blocks
         (id, evidence_id, extraction_revision_id, parent_id, ordinal, kind,
          start_byte, end_byte, locator_version, locator_kind, locator_value,
          heading_level, list_start, task_checked, info_string)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11,
                 ?12, ?13, ?14, ?15)",
        params![
            to_vault_sql_id(block_id.get())?,
            to_vault_sql_id(revision.evidence_id())?,
            to_vault_sql_id(revision.id().get())?,
            parent_id.map(|id| to_vault_sql_id(id.get())).transpose()?,
            i64::try_from(draft.ordinal()).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
            encode_markdown_block_kind(draft.kind()),
            i64::try_from(draft.anchor().start_byte())
                .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
            i64::try_from(draft.anchor().end_byte())
                .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
            locator_version,
            locator_kind,
            locator_value,
            metadata.heading_level().map(i64::from),
            metadata.list_start().map(|value| value.to_string()),
            metadata.task_checked().map(i64::from),
            metadata.info_string(),
        ],
    )?;
    EvidenceBlock::new(
        block_id,
        revision.evidence_id(),
        revision.id(),
        parent_id,
        draft.ordinal(),
        draft.kind(),
        draft.anchor().clone(),
        draft.metadata().clone(),
    )
    .map_err(|_| VaultError::InvalidKeyOrCorrupt)
}

fn encode_markdown_locator(
    locator: Option<&MarkdownLocator>,
) -> (Option<&str>, Option<i64>, Option<&str>) {
    match locator {
        None => (None, None, None),
        Some(locator) => match locator.value() {
            MarkdownLocatorValue::Heading { text } => {
                (Some(locator.version()), Some(0), Some(text))
            }
            MarkdownLocatorValue::BlockId { id } => (Some(locator.version()), Some(1), Some(id)),
        },
    }
}

fn decode_markdown_locator(
    version: Option<String>,
    kind: Option<i64>,
    value: Option<String>,
) -> Result<Option<MarkdownLocator>, VaultError> {
    match (version, kind, value) {
        (None, None, None) => Ok(None),
        (Some(version), Some(kind), Some(value)) => {
            if version != MARKDOWN_LOCATOR_VERSION {
                return Err(VaultError::InvalidKeyOrCorrupt);
            }
            let value = match kind {
                0 => MarkdownLocatorValue::Heading { text: value },
                1 => MarkdownLocatorValue::BlockId { id: value },
                _ => return Err(VaultError::InvalidKeyOrCorrupt),
            };
            MarkdownLocator::new(version, value)
                .map(Some)
                .map_err(|_| VaultError::InvalidKeyOrCorrupt)
        }
        _ => Err(VaultError::InvalidKeyOrCorrupt),
    }
}

const fn encode_markdown_block_kind(kind: MarkdownBlockKind) -> i64 {
    match kind {
        MarkdownBlockKind::Paragraph => 0,
        MarkdownBlockKind::Heading => 1,
        MarkdownBlockKind::BlockQuote => 2,
        MarkdownBlockKind::List => 3,
        MarkdownBlockKind::ListItem => 4,
        MarkdownBlockKind::CodeBlock => 5,
        MarkdownBlockKind::Table => 6,
        MarkdownBlockKind::TableHead => 7,
        MarkdownBlockKind::TableRow => 8,
        MarkdownBlockKind::TableCell => 9,
        MarkdownBlockKind::HtmlBlock => 10,
        MarkdownBlockKind::ThematicBreak => 11,
        MarkdownBlockKind::MetadataBlock => 12,
    }
}

const fn decode_markdown_block_kind(value: i64) -> Result<MarkdownBlockKind, VaultError> {
    match value {
        0 => Ok(MarkdownBlockKind::Paragraph),
        1 => Ok(MarkdownBlockKind::Heading),
        2 => Ok(MarkdownBlockKind::BlockQuote),
        3 => Ok(MarkdownBlockKind::List),
        4 => Ok(MarkdownBlockKind::ListItem),
        5 => Ok(MarkdownBlockKind::CodeBlock),
        6 => Ok(MarkdownBlockKind::Table),
        7 => Ok(MarkdownBlockKind::TableHead),
        8 => Ok(MarkdownBlockKind::TableRow),
        9 => Ok(MarkdownBlockKind::TableCell),
        10 => Ok(MarkdownBlockKind::HtmlBlock),
        11 => Ok(MarkdownBlockKind::ThematicBreak),
        12 => Ok(MarkdownBlockKind::MetadataBlock),
        _ => Err(VaultError::InvalidKeyOrCorrupt),
    }
}

const LOAD_EVIDENCE_BLOCKS_QUERY: &str =
    "SELECT id, evidence_id, parent_id, ordinal, kind, start_byte, end_byte,
            locator_version, locator_kind, locator_value, heading_level,
            list_start, task_checked, info_string
     FROM evidence_blocks
     WHERE extraction_revision_id = ?1
     ORDER BY ordinal";

fn load_evidence_blocks(
    connection: &Connection,
    revision: &ExtractionRevision,
    canonical_text: &str,
) -> Result<Vec<EvidenceBlock>, VaultError> {
    let mut statement = connection.prepare(LOAD_EVIDENCE_BLOCKS_QUERY)?;
    let rows = statement
        .query_map([to_vault_sql_id(revision.id().get())?], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<i64>>(8)?,
                row.get::<_, Option<String>>(9)?,
                row.get::<_, Option<i64>>(10)?,
                row.get::<_, Option<String>>(11)?,
                row.get::<_, Option<i64>>(12)?,
                row.get::<_, Option<String>>(13)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(
            |(
                id,
                evidence_id,
                parent_id,
                ordinal,
                kind,
                start_byte,
                end_byte,
                locator_version,
                locator_kind,
                locator_value,
                heading_level,
                list_start,
                task_checked,
                info_string,
            )| {
                let stored_evidence_id =
                    u64::try_from(evidence_id).map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
                if stored_evidence_id != revision.evidence_id() {
                    return Err(VaultError::InvalidKeyOrCorrupt);
                }
                let locator =
                    decode_markdown_locator(locator_version, locator_kind, locator_value)?;
                let anchor = SourceAnchor::new(
                    canonical_text,
                    usize::try_from(start_byte).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                    usize::try_from(end_byte).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                    locator,
                )
                .map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
                let metadata = EvidenceBlockMetadata::new(
                    heading_level
                        .map(u8::try_from)
                        .transpose()
                        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                    list_start
                        .map(|value| value.parse::<u64>())
                        .transpose()
                        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                    task_checked
                        .map(|value| match value {
                            0 => Ok(false),
                            1 => Ok(true),
                            _ => Err(VaultError::InvalidKeyOrCorrupt),
                        })
                        .transpose()?,
                    info_string,
                );
                EvidenceBlock::new(
                    EvidenceBlockId::new(
                        u64::try_from(id).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                    )
                    .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                    stored_evidence_id,
                    revision.id(),
                    parent_id
                        .map(u64::try_from)
                        .transpose()
                        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?
                        .map(EvidenceBlockId::new)
                        .transpose()
                        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                    usize::try_from(ordinal).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                    decode_markdown_block_kind(kind)?,
                    anchor,
                    metadata,
                )
                .map_err(|_| VaultError::InvalidKeyOrCorrupt)
            },
        )
        .collect()
}

fn next_identifier(connection: &Connection, table: &str) -> Result<u64, VaultError> {
    let query = format!("SELECT COALESCE(MAX(id), 0) FROM {table}");
    let maximum: i64 = connection.query_row(&query, [], |row| row.get(0))?;
    let maximum = u64::try_from(maximum).map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
    maximum
        .checked_add(1)
        .ok_or(VaultError::InvalidKeyOrCorrupt)
}

fn to_vault_sql_id(id: u64) -> Result<i64, VaultError> {
    i64::try_from(id).map_err(|_| VaultError::InvalidKeyOrCorrupt)
}

fn next_host_identifier(connection: &Connection, table: &str) -> Result<u64, RepositoryError> {
    let query = format!("SELECT COALESCE(MAX(id), 0) FROM {table}");
    let maximum: i64 = connection
        .query_row(&query, [], |row| row.get(0))
        .map_err(repository_error)?;
    let maximum = u64::try_from(maximum).map_err(repository_error)?;
    let next = maximum
        .checked_add(1)
        .ok_or_else(|| RepositoryError::new("host lifecycle identifier space exhausted"))?;
    Ok(next)
}

fn to_sql_id(id: u64) -> Result<i64, RepositoryError> {
    i64::try_from(id).map_err(repository_error)
}

fn repository_error(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::new(error.to_string())
}

const fn encode_launch_mode(mode: LaunchMode) -> i64 {
    match mode {
        LaunchMode::Foreground => 0,
        LaunchMode::Background => 1,
        LaunchMode::UpdateRelaunch => 2,
    }
}

fn decode_launch_mode(value: i64) -> Result<LaunchMode, RepositoryError> {
    match value {
        0 => Ok(LaunchMode::Foreground),
        1 => Ok(LaunchMode::Background),
        2 => Ok(LaunchMode::UpdateRelaunch),
        _ => Err(RepositoryError::new("invalid persisted host launch mode")),
    }
}

const fn encode_exit_reason(reason: ExitReason) -> i64 {
    match reason {
        ExitReason::Explicit => 0,
        ExitReason::Update => 1,
    }
}

fn decode_exit_reason(value: i64) -> Result<ExitReason, RepositoryError> {
    match value {
        0 => Ok(ExitReason::Explicit),
        1 => Ok(ExitReason::Update),
        _ => Err(RepositoryError::new("invalid persisted host exit reason")),
    }
}

const fn encode_gap_reason(reason: HostGapReason) -> i64 {
    match reason {
        HostGapReason::Crash => 0,
        HostGapReason::ExplicitExit => 1,
        HostGapReason::Update => 2,
    }
}

fn decode_gap_reason(value: i64) -> Result<HostGapReason, RepositoryError> {
    match value {
        0 => Ok(HostGapReason::Crash),
        1 => Ok(HostGapReason::ExplicitExit),
        2 => Ok(HostGapReason::Update),
        _ => Err(RepositoryError::new("invalid persisted host gap reason")),
    }
}

fn recovered_gap_spec(
    previous: &HostSession,
    started_at: Timestamp,
) -> Option<(Timestamp, Timestamp, HostGapReason, bool)> {
    let (candidate_from, reason) = match (previous.ended_at(), previous.end_reason()) {
        (Some(ended_at), Some(ExitReason::Explicit)) => (ended_at, HostGapReason::ExplicitExit),
        (Some(ended_at), Some(ExitReason::Update)) => (ended_at, HostGapReason::Update),
        (None, None) => (previous.last_seen_at(), HostGapReason::Crash),
        _ => return None,
    };
    match candidate_from.cmp(&started_at) {
        std::cmp::Ordering::Less => Some((candidate_from, started_at, reason, false)),
        std::cmp::Ordering::Greater => Some((started_at, started_at, reason, true)),
        std::cmp::Ordering::Equal => None,
    }
}

fn current_host_session(connection: &Connection) -> Result<Option<HostSession>, RepositoryError> {
    connection
        .query_row(
            "SELECT id, launch_mode, started_at, last_seen_at, ended_at, end_reason
             FROM host_sessions ORDER BY id DESC LIMIT 1",
            [],
            stored_host_session_from_row,
        )
        .optional()
        .map_err(repository_error)?
        .map(StoredHostSession::decode)
        .transpose()
}

struct StoredHostSession {
    id: i64,
    launch_mode: i64,
    started_at: i64,
    last_seen_at: i64,
    ended_at: Option<i64>,
    end_reason: Option<i64>,
}

impl StoredHostSession {
    fn decode(self) -> Result<HostSession, RepositoryError> {
        if self.id <= 0 || self.last_seen_at < self.started_at {
            return Err(RepositoryError::new("invalid persisted host session"));
        }
        let end_reason = self.end_reason.map(decode_exit_reason).transpose()?;
        if self.ended_at.is_some() != end_reason.is_some()
            || self.ended_at.is_some_and(|ended| ended < self.last_seen_at)
        {
            return Err(RepositoryError::new("invalid persisted host session end"));
        }
        Ok(HostSession::restore(
            HostSessionId::from_raw(u64::try_from(self.id).map_err(repository_error)?),
            decode_launch_mode(self.launch_mode)?,
            Timestamp::from_millis(self.started_at),
            Timestamp::from_millis(self.last_seen_at),
            self.ended_at.map(Timestamp::from_millis),
            end_reason,
        ))
    }
}

fn stored_host_session_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredHostSession> {
    Ok(StoredHostSession {
        id: row.get(0)?,
        launch_mode: row.get(1)?,
        started_at: row.get(2)?,
        last_seen_at: row.get(3)?,
        ended_at: row.get(4)?,
        end_reason: row.get(5)?,
    })
}

struct StoredHostGap {
    id: i64,
    from: i64,
    to: i64,
    reason: i64,
    clock_rollback: i64,
    recovered_by: i64,
}

impl StoredHostGap {
    fn decode(self) -> Result<HostRuntimeGap, RepositoryError> {
        if self.id <= 0
            || self.recovered_by <= 0
            || self.to < self.from
            || !matches!(self.clock_rollback, 0 | 1)
        {
            return Err(RepositoryError::new("invalid persisted host runtime gap"));
        }
        Ok(HostRuntimeGap::restore(
            HostGapId::from_raw(u64::try_from(self.id).map_err(repository_error)?),
            Timestamp::from_millis(self.from),
            Timestamp::from_millis(self.to),
            decode_gap_reason(self.reason)?,
            self.clock_rollback == 1,
            HostSessionId::from_raw(u64::try_from(self.recovered_by).map_err(repository_error)?),
        ))
    }
}

fn stored_host_gap_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredHostGap> {
    Ok(StoredHostGap {
        id: row.get(0)?,
        from: row.get(1)?,
        to: row.get(2)?,
        reason: row.get(3)?,
        clock_rollback: row.get(4)?,
        recovered_by: row.get(5)?,
    })
}

const fn encode_speaker(speaker: Speaker) -> i64 {
    match speaker {
        Speaker::Person => 0,
        Speaker::Counterpart => 1,
    }
}

fn decode_speaker(value: i64) -> Result<Speaker, RepositoryError> {
    match value {
        0 => Ok(Speaker::Person),
        1 => Ok(Speaker::Counterpart),
        _ => Err(RepositoryError::new("invalid persisted speaker")),
    }
}

const fn encode_owner(owner: ClaimOwner) -> i64 {
    match owner {
        ClaimOwner::Person => 0,
        ClaimOwner::Counterpart => 1,
        ClaimOwner::Shared => 2,
    }
}

fn decode_owner(value: i64) -> Result<ClaimOwner, RepositoryError> {
    match value {
        0 => Ok(ClaimOwner::Person),
        1 => Ok(ClaimOwner::Counterpart),
        2 => Ok(ClaimOwner::Shared),
        _ => Err(RepositoryError::new("invalid persisted claim owner")),
    }
}

const fn encode_uncertainty(value: Uncertainty) -> i64 {
    match value {
        Uncertainty::Low => 0,
        Uncertainty::Medium => 1,
        Uncertainty::High => 2,
    }
}

fn decode_uncertainty(value: i64) -> Result<Uncertainty, RepositoryError> {
    match value {
        0 => Ok(Uncertainty::Low),
        1 => Ok(Uncertainty::Medium),
        2 => Ok(Uncertainty::High),
        _ => Err(RepositoryError::new("invalid persisted uncertainty")),
    }
}

const fn encode_applicable_time(value: ApplicableTime) -> (i64, Option<i64>, Option<i64>) {
    match value {
        ApplicableTime::At(timestamp) => (0, Some(timestamp.as_millis()), None),
        ApplicableTime::Since(timestamp) => (1, Some(timestamp.as_millis()), None),
        ApplicableTime::Between { start, end } => {
            (2, Some(start.as_millis()), Some(end.as_millis()))
        }
        ApplicableTime::Unknown => (3, None, None),
    }
}

fn decode_applicable_time(
    kind: i64,
    start: Option<i64>,
    end: Option<i64>,
) -> Result<ApplicableTime, RepositoryError> {
    match (kind, start, end) {
        (0, Some(value), None) => Ok(ApplicableTime::At(Timestamp::from_millis(value))),
        (1, Some(value), None) => Ok(ApplicableTime::Since(Timestamp::from_millis(value))),
        (2, Some(start), Some(end)) if start <= end => Ok(ApplicableTime::Between {
            start: Timestamp::from_millis(start),
            end: Timestamp::from_millis(end),
        }),
        (3, None, None) => Ok(ApplicableTime::Unknown),
        _ => Err(RepositoryError::new("invalid persisted applicable time")),
    }
}

struct StoredEvidence {
    id: i64,
    session_id: String,
    speaker: i64,
    verbatim: String,
    recorded_at: i64,
}

impl StoredEvidence {
    fn decode(self) -> Result<ConversationEvidence, RepositoryError> {
        let id = u64::try_from(self.id).map_err(repository_error)?;
        Ok(ConversationEvidence::restore(
            EvidenceId::from_raw(id),
            SessionId::new(self.session_id),
            decode_speaker(self.speaker)?,
            self.verbatim,
            Timestamp::from_millis(self.recorded_at),
        ))
    }
}

fn stored_evidence_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredEvidence> {
    Ok(StoredEvidence {
        id: row.get(0)?,
        session_id: row.get(1)?,
        speaker: row.get(2)?,
        verbatim: row.get(3)?,
        recorded_at: row.get(4)?,
    })
}

struct StoredClaim {
    id: i64,
    owner: i64,
    statement: String,
    uncertainty: Option<i64>,
    applicable_kind: i64,
    applicable_start: Option<i64>,
    applicable_end: Option<i64>,
    recorded_at: i64,
}

impl StoredClaim {
    fn decode(self, connection: &Connection) -> Result<Claim, RepositoryError> {
        let id = u64::try_from(self.id).map_err(repository_error)?;
        let claim_id = ClaimId::from_raw(id);
        let support = load_support(connection, claim_id)?;
        Ok(Claim::restore(
            claim_id,
            decode_owner(self.owner)?,
            self.statement,
            support,
            self.uncertainty.map(decode_uncertainty).transpose()?,
            decode_applicable_time(
                self.applicable_kind,
                self.applicable_start,
                self.applicable_end,
            )?,
            Timestamp::from_millis(self.recorded_at),
        ))
    }
}

fn stored_claim_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredClaim> {
    Ok(StoredClaim {
        id: row.get(0)?,
        owner: row.get(1)?,
        statement: row.get(2)?,
        uncertainty: row.get(3)?,
        applicable_kind: row.get(4)?,
        applicable_start: row.get(5)?,
        applicable_end: row.get(6)?,
        recorded_at: row.get(7)?,
    })
}

fn load_support(
    connection: &Connection,
    claim_id: ClaimId,
) -> Result<Vec<EvidenceCitation>, RepositoryError> {
    let mut statement = connection
        .prepare(
            "SELECT evidence_id, quote FROM claim_support
             WHERE claim_id = ?1 ORDER BY ordinal",
        )
        .map_err(repository_error)?;
    let rows = statement
        .query_map([to_sql_id(claim_id.get())?], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(repository_error)?;
    let stored = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(repository_error)?;
    stored
        .into_iter()
        .map(|(evidence_id, quote)| {
            let id = u64::try_from(evidence_id).map_err(repository_error)?;
            Ok(EvidenceCitation::new(EvidenceId::from_raw(id), quote))
        })
        .collect()
}

fn load_identity_evidence(
    connection: &Connection,
    identity_version: u64,
) -> Result<Vec<EvidenceId>, RepositoryError> {
    let mut statement = connection
        .prepare(
            "SELECT evidence_id FROM identity_state_evidence
             WHERE identity_version = ?1 ORDER BY ordinal",
        )
        .map_err(repository_error)?;
    let rows = statement
        .query_map([to_sql_id(identity_version)?], |row| row.get::<_, i64>(0))
        .map_err(repository_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(repository_error)?;
    rows.into_iter()
        .map(|value| {
            u64::try_from(value)
                .map(EvidenceId::from_raw)
                .map_err(repository_error)
        })
        .collect()
}

fn decode_wake_commit(
    trigger: Option<i64>,
    exit: Option<i64>,
) -> Result<Option<WakeCommit>, RepositoryError> {
    match (trigger, exit) {
        (None, None) => Ok(None),
        (Some(trigger), Some(exit)) => {
            let trigger = WakeTrigger::from_code(trigger)
                .ok_or_else(|| RepositoryError::new("invalid persisted wake trigger"))?;
            let exit = WakeExit::from_code(exit)
                .ok_or_else(|| RepositoryError::new("invalid persisted wake exit"))?;
            Ok(Some(WakeCommit::new(trigger, exit)))
        }
        _ => Err(RepositoryError::new(
            "persisted wake commit is structurally incomplete",
        )),
    }
}

fn load_self_bundle_experiences(
    connection: &Connection,
    bundle_version: u64,
) -> Result<Vec<String>, RepositoryError> {
    let mut statement = connection
        .prepare(
            "SELECT experience_ref FROM self_bundle_experiences
             WHERE bundle_version = ?1 ORDER BY ordinal",
        )
        .map_err(repository_error)?;
    statement
        .query_map([to_sql_id(bundle_version)?], |row| row.get::<_, String>(0))
        .map_err(repository_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(repository_error)
}

fn load_self_bundle_beliefs(
    connection: &Connection,
    bundle_version: u64,
) -> Result<Vec<ClaimId>, RepositoryError> {
    let mut statement = connection
        .prepare(
            "SELECT claim_id FROM self_bundle_beliefs
             WHERE bundle_version = ?1 ORDER BY ordinal",
        )
        .map_err(repository_error)?;
    let stored = statement
        .query_map([to_sql_id(bundle_version)?], |row| row.get::<_, i64>(0))
        .map_err(repository_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(repository_error)?;
    stored
        .into_iter()
        .map(|value| {
            u64::try_from(value)
                .map(ClaimId::from_raw)
                .map_err(repository_error)
        })
        .collect()
}

fn load_self_bundle_intentions(
    connection: &Connection,
    bundle_version: u64,
) -> Result<Vec<String>, RepositoryError> {
    let mut statement = connection
        .prepare(
            "SELECT intention FROM self_bundle_pending_intentions
             WHERE bundle_version = ?1 ORDER BY ordinal",
        )
        .map_err(repository_error)?;
    statement
        .query_map([to_sql_id(bundle_version)?], |row| row.get::<_, String>(0))
        .map_err(repository_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(repository_error)
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn close_clears_owned_vault_key_after_sqlcipher_is_closed() {
        let _guard = crate::test_support::sqlcipher_test_lock();
        let directory = tempdir().unwrap();
        let mut repository = VaultRepository::open(directory.path(), VaultKey::new([0x55; 32]))
            .expect("SQLCipher compatibility spike should open");

        repository.close_inner().unwrap();

        assert!(repository.connection.is_none());
        assert!(repository.vault_key.is_zeroed());
        assert!(repository.writer_lock.is_none());
    }

    #[test]
    fn bundled_binding_exposes_sqlcipher_four() {
        let _guard = crate::test_support::sqlcipher_test_lock();
        let directory = tempdir().unwrap();
        let repository = VaultRepository::open(directory.path(), VaultKey::new([0x11; 32]))
            .expect("bundled SQLCipher should open");
        let version = repository.sqlcipher_version().unwrap();

        assert!(version.starts_with("4."), "unexpected SQLCipher: {version}");
    }

    #[test]
    fn database_failure_leaves_recoverable_orphan_removed_on_reopen() {
        let _guard = crate::test_support::sqlcipher_test_lock();
        let directory = tempdir().unwrap();
        let mut repository =
            VaultRepository::open(directory.path(), VaultKey::new([0x29; 32])).unwrap();

        let result = repository.archive_with_hook(
            &ArchiveInput {
                source_locator: "inbox/interrupted.md",
                content: b"durable object before database reference",
                status: ArchiveStatus::Archived,
                archived_at_millis: 100,
            },
            |_| Err(VaultError::ArchiveInterrupted),
        );

        assert!(matches!(result, Err(VaultError::ArchiveInterrupted)));
        assert!(repository.archived_evidence().unwrap().is_empty());
        assert_eq!(repository.object_store.object_file_count().unwrap(), 1);
        repository.close().unwrap();

        let repository =
            VaultRepository::open(directory.path(), VaultKey::new([0x29; 32])).unwrap();
        assert!(repository.archived_evidence().unwrap().is_empty());
        assert_eq!(repository.object_store.object_file_count().unwrap(), 0);
    }

    #[test]
    fn extraction_failure_rolls_back_revision_and_every_block_across_reopen() {
        let _guard = crate::test_support::sqlcipher_test_lock();
        let directory = tempdir().unwrap();
        let key = [0x6b; 32];
        let source = "# 原子提交 😀\n\n正文 e\u{301} 日本語\n";
        let mut repository = VaultRepository::open(directory.path(), VaultKey::new(key)).unwrap();
        let receipt = repository
            .archive(ArchiveInput {
                source_locator: "inbox/atomic.md",
                content: source.as_bytes(),
                status: ArchiveStatus::Archived,
                archived_at_millis: 10,
            })
            .unwrap();
        eam_ingestion::process_archived_markdown(
            &mut repository,
            receipt.archive_id,
            eam_markdown::ParseLimits::default(),
            20,
            30,
        )
        .unwrap();
        let accepted = repository
            .load_accepted_markdown(receipt.archive_id, eam_markdown::CONTRACT_VERSION)
            .unwrap();
        let validated = eam_ingestion::validate_accepted_markdown(
            receipt.archive_id,
            std::str::from_utf8(accepted.canonical_bytes()).unwrap(),
            accepted.parsed(),
            accepted.accepted_at_millis(),
        )
        .unwrap();

        let result = repository
            .commit_extraction_with_hook(&validated, |_| Err(VaultError::ExtractionInterrupted));

        assert!(matches!(result, Err(VaultError::ExtractionInterrupted)));
        assert!(
            repository
                .materialized_extraction(receipt.archive_id, eam_markdown::CONTRACT_VERSION)
                .unwrap()
                .is_none()
        );
        repository.close().unwrap();

        let repository = VaultRepository::open(directory.path(), VaultKey::new(key)).unwrap();
        assert!(
            repository
                .materialized_extraction(receipt.archive_id, eam_markdown::CONTRACT_VERSION)
                .unwrap()
                .is_none()
        );
    }
}
