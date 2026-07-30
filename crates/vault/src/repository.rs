use std::{
    collections::{BTreeSet, HashMap, HashSet},
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
    ArchivedEvidence, BlockLineage, BlockLineageRepository, BlockLineageStatus,
    CanonicalEvidenceBlockSource, CanonicalLineageRevision, EvidenceBlock, EvidenceBlockDraft,
    EvidenceBlockId, EvidenceBlockMetadata, EvidenceBlockQueryRepository, EvidenceBlockRef,
    EvidenceBlockView, EvidenceExtractionRepository, ExtractionRevision, ExtractionRevisionId,
    IncrementalWorkItem, IncrementalWorkPlan, LineageBasis, LineageBatch, LineagePair,
    MARKDOWN_LOCATOR_VERSION, MarkdownArchiveRepository, MarkdownLocator, MarkdownLocatorValue,
    MarkdownParseAttempt, MarkdownParseStart, MarkdownParseState, MaterializedExtraction,
    SourceAnchor, UnparsedReason, ValidatedExtraction,
};
use eam_markdown::{MarkdownBlockKind, MarkdownRelationKind, ParseResource, ParsedMarkdownV1};
use eam_retrieval::{
    AuthoritativeCandidate, AuthoritativeEvidence, CandidateRef, EMBEDDING_MODEL_VERSION,
    IndexBuildReceipt, IndexDisposition, RETRIEVAL_INDEX_VERSION, RecallChannels, RecallHit,
    RetrievalQuery, RetrievalRepository, SourceCurrentness, SourceScope, VECTOR_DIMENSIONS,
    VECTOR_MIN_SCORE_BPS, VectorEmbedding, cosine_similarity_bps, embed_text, search_terms,
};
use eam_source_obsidian::{
    ObsidianSourceRepository, SourceArchiveInput, SourceArchiveReceipt, SourceAvailability,
    SourceDocumentProjection, SourceFileKind, SourceRecord, SourceRecordState, SourceRelation,
    SourceRelationKind, SourceRoot, SourceRootSnapshot,
};
use fs4::{FileExt, TryLockError};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use sha2::{Digest, Sha256};

use crate::{
    VaultError, VaultKey, crypto::sqlcipher_key_pragma, object_store::ObjectStore, schema::migrate,
};

const DATABASE_FILE: &str = "self.db";
const WRITER_LOCK_FILE: &str = "self.db.writer.lock";
const MAX_VECTOR_CANDIDATES: usize = 64;
const TEMPORAL_NEIGHBOR_RADIUS_MILLIS: i64 = 7 * 24 * 60 * 60 * 1_000;
const MAX_TEMPORAL_NEIGHBORS: usize = 4;
const MAX_RELATION_NEIGHBORS: usize = 8;

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

    /// Loads the queryable Obsidian metadata and relation projection for one
    /// accepted evidence version.
    ///
    /// # Errors
    ///
    /// Returns an error when the evidence is not an Obsidian document or its
    /// encrypted projection is structurally invalid.
    pub fn source_document_projection(
        &self,
        evidence_id: u64,
    ) -> Result<SourceDocumentProjection, VaultError> {
        let evidence_id_sql = to_vault_sql_id(evidence_id)?;
        let is_obsidian = self
            .connection()
            .query_row(
                "SELECT 1
                 FROM source_record_versions v
                 JOIN source_records s ON s.id = v.source_record_id
                 WHERE v.evidence_id = ?1 AND s.origin_kind = 1",
                [evidence_id_sql],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !is_obsidian {
            return Err(VaultError::InvalidKeyOrCorrupt);
        }
        let properties = load_string_pairs(
            self.connection(),
            "SELECT name, value FROM obsidian_properties
             WHERE evidence_id = ?1 ORDER BY property_ordinal, value_ordinal",
            evidence_id_sql,
        )?;
        let tags = load_strings(
            self.connection(),
            "SELECT value FROM obsidian_tags WHERE evidence_id = ?1 ORDER BY ordinal",
            evidence_id_sql,
        )?;
        let aliases = load_strings(
            self.connection(),
            "SELECT value FROM obsidian_aliases WHERE evidence_id = ?1 ORDER BY ordinal",
            evidence_id_sql,
        )?;
        let mut statement = self.connection().prepare(
            "SELECT r.relation_kind, r.target, r.alias, r.heading, r.block_id,
                    x.resolved_source_record_id
             FROM obsidian_relations r
             LEFT JOIN obsidian_relation_resolutions x
               ON x.evidence_id = r.evidence_id
              AND x.relation_ordinal = r.ordinal
             WHERE r.evidence_id = ?1 ORDER BY r.ordinal",
        )?;
        let relations = statement
            .query_map([evidence_id_sql], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|(kind, target, alias, heading, block_id, resolved)| {
                Ok(SourceRelation::new(
                    decode_source_relation_kind(kind)?,
                    target,
                    alias,
                    heading,
                    block_id,
                    resolved
                        .map(|id| u64::try_from(id).map_err(|_| VaultError::InvalidKeyOrCorrupt))
                        .transpose()?,
                ))
            })
            .collect::<Result<Vec<_>, VaultError>>()?;
        Ok(SourceDocumentProjection::new(
            evidence_id,
            properties,
            tags,
            aliases,
            relations,
        ))
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
        ensure_source_record_version(
            &transaction,
            input.source_locator,
            to_vault_sql_id(archive_id)?,
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

    fn commit_lineage_batch_with_hook<F>(
        &mut self,
        batch: &LineageBatch,
        before_commit: F,
    ) -> Result<LineageBatch, VaultError>
    where
        F: FnOnce(&rusqlite::Transaction<'_>) -> Result<(), VaultError>,
    {
        if let Some(existing) =
            self.load_lineage_batch(batch.to_revision_id(), batch.rule_version())?
        {
            if existing == *batch {
                return Ok(existing);
            }
            return Err(VaultError::InvalidKeyOrCorrupt);
        }
        let pair = self
            .load_lineage_pair(batch.to_revision_id())?
            .ok_or(VaultError::InvalidKeyOrCorrupt)?;
        if pair.source_record_id() != batch.source_record_id()
            || pair.previous().extraction().revision().id() != batch.from_revision_id()
        {
            return Err(VaultError::InvalidKeyOrCorrupt);
        }

        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()?;
        let batch_id = next_identifier(&transaction, "block_lineage_batches")?;
        transaction.execute(
            "INSERT INTO block_lineage_batches
             (id, source_record_id, from_revision_id, to_revision_id,
              decided_at, rule_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                to_vault_sql_id(batch_id)?,
                to_vault_sql_id(batch.source_record_id())?,
                to_vault_sql_id(batch.from_revision_id().get())?,
                to_vault_sql_id(batch.to_revision_id().get())?,
                batch.decided_at_millis(),
                batch.rule_version(),
            ],
        )?;
        for (ordinal, lineage) in batch.lineages().iter().enumerate() {
            insert_block_lineage(&transaction, batch_id, ordinal, lineage)?;
        }
        for (ordinal, item) in batch.work_plan().items().iter().enumerate() {
            insert_incremental_work_item(&transaction, batch_id, ordinal, item)?;
        }
        before_commit(&transaction)?;
        transaction.commit()?;
        Ok(batch.clone())
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
                "SELECT CASE WHEN s.origin_kind = 1 THEN s.current_locator
                             ELSE a.source_locator END
                 FROM archived_evidence a
                 JOIN source_record_versions v ON v.evidence_id = a.id
                 JOIN source_records s ON s.id = v.source_record_id
                 WHERE a.id = ?1",
                [archive_id_sql],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten()
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
        persist_obsidian_parse_projection(&transaction, archive_id_sql, parsed)?;
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

impl BlockLineageRepository for VaultRepository {
    type Error = VaultError;

    fn load_lineage_pair(
        &self,
        to_revision_id: ExtractionRevisionId,
    ) -> Result<Option<LineagePair>, Self::Error> {
        let current = self
            .connection()
            .query_row(
                "SELECT v.source_record_id, v.version_ordinal
                 FROM extraction_revisions r
                 JOIN source_record_versions v ON v.evidence_id = r.evidence_id
                 WHERE r.id = ?1",
                [to_vault_sql_id(to_revision_id.get())?],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?
            .ok_or(VaultError::InvalidKeyOrCorrupt)?;
        let source_record_id =
            u64::try_from(current.0).map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
        let previous_revision_id = self
            .connection()
            .query_row(
                "SELECT r.id
                 FROM extraction_revisions r
                 JOIN source_record_versions v ON v.evidence_id = r.evidence_id
                 WHERE v.source_record_id = ?1
                   AND r.id != ?2
                   AND (
                       v.version_ordinal < ?3
                       OR (v.version_ordinal = ?3 AND r.id < ?2)
                   )
                 ORDER BY v.version_ordinal DESC, r.id DESC
                 LIMIT 1",
                params![
                    to_vault_sql_id(source_record_id)?,
                    to_vault_sql_id(to_revision_id.get())?,
                    current.1,
                ],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        let Some(previous_revision_id) = previous_revision_id else {
            return Ok(None);
        };
        let previous_revision_id = ExtractionRevisionId::new(
            u64::try_from(previous_revision_id).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
        )
        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
        let previous = load_canonical_lineage_revision(self, previous_revision_id)?;
        let current = load_canonical_lineage_revision(self, to_revision_id)?;
        LineagePair::new(source_record_id, previous, current)
            .map(Some)
            .map_err(|_| VaultError::InvalidKeyOrCorrupt)
    }

    fn commit_lineage_batch(&mut self, batch: &LineageBatch) -> Result<LineageBatch, Self::Error> {
        self.commit_lineage_batch_with_hook(batch, |_| Ok(()))
    }

    fn load_lineage_batch(
        &self,
        to_revision_id: ExtractionRevisionId,
        rule_version: &str,
    ) -> Result<Option<LineageBatch>, Self::Error> {
        load_lineage_batch(self.connection(), to_revision_id, rule_version)
    }
}

impl ObsidianSourceRepository for VaultRepository {
    type Error = VaultError;

    fn register_source_root(
        &mut self,
        root_locator: &str,
        observed_at_millis: i64,
    ) -> Result<SourceRoot, Self::Error> {
        if root_locator.trim().is_empty() {
            return Err(VaultError::InvalidKeyOrCorrupt);
        }
        if let Some(root_id) = self
            .connection()
            .query_row(
                "SELECT id FROM source_roots WHERE root_kind = 0 AND root_locator = ?1",
                [root_locator],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
        {
            return load_source_root_snapshot(
                self.connection(),
                u64::try_from(root_id).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
            )
            .map(|snapshot| snapshot.root().clone());
        }
        let root_id = next_identifier(self.connection(), "source_roots")?;
        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()?;
        transaction.execute(
            "INSERT INTO source_roots
             (id, root_kind, root_locator, availability, first_seen_at, last_reconciled_at)
             VALUES (?1, 0, ?2, 0, ?3, NULL)",
            params![to_vault_sql_id(root_id)?, root_locator, observed_at_millis],
        )?;
        insert_source_root_event(
            &transaction,
            root_id,
            SourceAvailability::Available,
            observed_at_millis,
        )?;
        transaction.commit()?;
        load_source_root_snapshot(self.connection(), root_id)
            .map(|snapshot| snapshot.root().clone())
    }

    fn load_source_root(&self, root_id: u64) -> Result<SourceRootSnapshot, Self::Error> {
        load_source_root_snapshot(self.connection(), root_id)
    }

    fn mark_source_unavailable(
        &mut self,
        root_id: u64,
        observed_at_millis: i64,
    ) -> Result<SourceRoot, Self::Error> {
        let root_id_sql = to_vault_sql_id(root_id)?;
        let current = self
            .connection()
            .query_row(
                "SELECT availability FROM source_roots WHERE id = ?1",
                [root_id_sql],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .ok_or(VaultError::InvalidKeyOrCorrupt)?;
        if current != encode_source_availability(SourceAvailability::SourceUnavailable) {
            let transaction = self
                .connection
                .as_mut()
                .expect("an open vault always owns a database connection")
                .transaction()?;
            transaction.execute(
                "UPDATE source_roots SET availability = 1 WHERE id = ?1",
                [root_id_sql],
            )?;
            insert_source_root_event(
                &transaction,
                root_id,
                SourceAvailability::SourceUnavailable,
                observed_at_millis,
            )?;
            transaction.commit()?;
        }
        load_source_root_snapshot(self.connection(), root_id)
            .map(|snapshot| snapshot.root().clone())
    }

    fn archive_source_file(
        &mut self,
        input: SourceArchiveInput<'_>,
    ) -> Result<SourceArchiveReceipt, Self::Error> {
        if !is_normalized_source_locator(input.relative_path) || input.root_id == 0 {
            return Err(VaultError::InvalidKeyOrCorrupt);
        }
        let stored = self.object_store.store(input.content)?;
        let selected = select_source_record(
            self.connection(),
            &input,
            stored.id.as_str(),
            next_identifier(self.connection(), "source_records")?,
        )?;
        let existing_version =
            find_source_version(self.connection(), selected.id, stored.id.as_str())?;
        let archive_id = commit_source_file_observation(
            self.connection
                .as_mut()
                .expect("an open vault always owns a database connection"),
            &input,
            &selected,
            &stored.id,
            existing_version,
            self.next_archive_id,
        )?;
        if existing_version.is_none() {
            self.next_archive_id = archive_id
                .checked_add(1)
                .ok_or(VaultError::InvalidKeyOrCorrupt)?;
        }
        Ok(SourceArchiveReceipt::new(
            selected.id,
            archive_id,
            selected.previous_locator,
            stored.reused,
            existing_version.is_some(),
        ))
    }

    fn finish_source_reconciliation(
        &mut self,
        root_id: u64,
        observed_source_record_ids: &[u64],
        observed_at_millis: i64,
    ) -> Result<SourceRootSnapshot, Self::Error> {
        let root_id_sql = to_vault_sql_id(root_id)?;
        let observed = observed_source_record_ids
            .iter()
            .copied()
            .collect::<HashSet<_>>();
        if observed.len() != observed_source_record_ids.len() {
            return Err(VaultError::InvalidKeyOrCorrupt);
        }
        let mut statement = self.connection().prepare(
            "SELECT id, current_locator, record_state FROM source_records
             WHERE origin_kind = 1 AND root_id = ?1 ORDER BY id",
        )?;
        let records = statement
            .query_map([root_id_sql], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        let known = records
            .iter()
            .map(|(id, _, _)| u64::try_from(*id))
            .collect::<Result<HashSet<_>, _>>()
            .map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
        if !observed.is_subset(&known) {
            return Err(VaultError::InvalidKeyOrCorrupt);
        }
        let availability = self
            .connection()
            .query_row(
                "SELECT availability FROM source_roots WHERE id = ?1",
                [root_id_sql],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .ok_or(VaultError::InvalidKeyOrCorrupt)?;
        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()?;
        if availability != encode_source_availability(SourceAvailability::Available) {
            insert_source_root_event(
                &transaction,
                root_id,
                SourceAvailability::Available,
                observed_at_millis,
            )?;
        }
        transaction.execute(
            "UPDATE source_roots
             SET availability = 0, last_reconciled_at = ?1 WHERE id = ?2",
            params![observed_at_millis, root_id_sql],
        )?;
        for (id, locator, state) in records {
            let id = u64::try_from(id).map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
            if !observed.contains(&id)
                && state != encode_source_record_state(SourceRecordState::SourceRemoved)
            {
                transaction.execute(
                    "UPDATE source_records SET record_state = 1 WHERE id = ?1",
                    [to_vault_sql_id(id)?],
                )?;
                insert_source_record_event(
                    &transaction,
                    id,
                    SourceRecordState::SourceRemoved,
                    &locator,
                    observed_at_millis,
                )?;
            }
        }
        transaction.commit()?;
        load_source_root_snapshot(self.connection(), root_id)
    }

    fn refresh_source_relations(&mut self, root_id: u64) -> Result<(), Self::Error> {
        refresh_obsidian_relation_resolutions(
            self.connection.as_mut().expect("open vault"),
            root_id,
        )
    }
}

impl RetrievalRepository for VaultRepository {
    type Error = VaultError;

    fn ensure_retrieval_index(&mut self) -> Result<IndexBuildReceipt, Self::Error> {
        let authority = load_retrieval_authority(self)?;
        let stored = self
            .connection()
            .query_row(
                "SELECT contract_version, authority_digest, index_digest,
                        evidence_block_count, ledger_claim_count, relation_count
                 FROM retrieval_index_metadata WHERE id = 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                },
            )
            .optional()?;
        let current_index_digest = retrieval_index_digest(self.connection()).ok();
        let vector_index_complete =
            retrieval_vector_index_is_complete(self.connection(), authority.blocks.len())
                .unwrap_or(false);
        let is_current = stored.is_some_and(
            |(version, authority_digest, index_digest, blocks, claims, relations)| {
                version == RETRIEVAL_INDEX_VERSION
                    && authority_digest.as_slice() == authority.digest
                    && current_index_digest
                        .as_ref()
                        .is_some_and(|actual| index_digest.as_slice() == actual)
                    && usize::try_from(blocks).ok() == Some(authority.blocks.len())
                    && usize::try_from(claims).ok() == Some(authority.claims.len())
                    && usize::try_from(relations).ok() == Some(authority.relations.len())
                    && vector_index_complete
            },
        );
        if is_current {
            return Ok(authority.receipt(IndexDisposition::Current));
        }
        rebuild_retrieval_index(self, &authority)
    }

    fn recall_candidates(&self, query: &RetrievalQuery) -> Result<Vec<RecallHit>, Self::Error> {
        recall_retrieval_candidates(self.connection(), query)
    }

    fn recall_neighbors(
        &self,
        reference: CandidateRef,
        _scope: SourceScope,
    ) -> Result<Vec<RecallHit>, Self::Error> {
        recall_retrieval_neighbors(self.connection(), reference)
    }

    fn resolve_authoritative(
        &self,
        reference: CandidateRef,
        scope: SourceScope,
    ) -> Result<Option<AuthoritativeCandidate>, Self::Error> {
        resolve_retrieval_candidate(self, reference, scope)
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

#[derive(Clone)]
struct RetrievalBlockAuthority {
    reference: EvidenceBlockRef,
    source_record_id: u64,
    version_ordinal: u64,
    recorded_at_millis: i64,
    start_byte: usize,
    end_byte: usize,
    quote: String,
}

#[derive(Clone)]
struct RetrievalEntityAuthority {
    source_record_id: u64,
    terms: Vec<String>,
}

#[derive(Clone, Copy)]
struct RetrievalRelationAuthority {
    from_ref: EvidenceBlockRef,
    relation_ordinal: u64,
    to_source_record_id: u64,
    relation_kind: i64,
}

struct RetrievalAuthority {
    digest: [u8; 32],
    blocks: Vec<RetrievalBlockAuthority>,
    claims: Vec<Claim>,
    entities: Vec<RetrievalEntityAuthority>,
    relations: Vec<RetrievalRelationAuthority>,
    built_at_millis: i64,
}

impl RetrievalAuthority {
    fn receipt(&self, disposition: IndexDisposition) -> IndexBuildReceipt {
        IndexBuildReceipt::new(
            disposition,
            self.blocks.len(),
            self.claims.len(),
            self.relations.len(),
        )
    }
}

fn load_retrieval_authority(
    repository: &VaultRepository,
) -> Result<RetrievalAuthority, VaultError> {
    let mut statement = repository.connection().prepare(
        "SELECT r.evidence_id, r.contract_version, v.source_record_id,
                v.version_ordinal, a.archived_at
         FROM extraction_revisions r
         JOIN archived_evidence a ON a.id = r.evidence_id
         JOIN source_record_versions v ON v.evidence_id = r.evidence_id
         ORDER BY r.evidence_id, r.id",
    )?;
    let revisions = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);

    let mut blocks = Vec::new();
    let mut built_at_millis = i64::MIN;
    for (evidence_id, contract, source_record_id, version_ordinal, recorded_at_millis) in revisions
    {
        let evidence_id =
            u64::try_from(evidence_id).map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
        let materialized = repository
            .materialized_extraction(evidence_id, &contract)?
            .ok_or(VaultError::InvalidKeyOrCorrupt)?;
        let canonical = String::from_utf8(repository.read_archived_content(evidence_id)?)
            .map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
        for block in materialized.blocks() {
            blocks.push(RetrievalBlockAuthority {
                reference: block.reference(),
                source_record_id: u64::try_from(source_record_id)
                    .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                version_ordinal: u64::try_from(version_ordinal)
                    .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                recorded_at_millis,
                start_byte: block.anchor().start_byte(),
                end_byte: block.anchor().end_byte(),
                quote: block
                    .anchor()
                    .quote(&canonical)
                    .map_err(|_| VaultError::InvalidKeyOrCorrupt)?
                    .to_owned(),
            });
        }
        built_at_millis = built_at_millis.max(recorded_at_millis);
    }

    let claims =
        MemoryRepository::all_claims(repository).map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
    for claim in &claims {
        built_at_millis = built_at_millis.max(claim.recorded_at().as_millis());
    }
    if built_at_millis == i64::MIN {
        built_at_millis = 0;
    }
    let entities = load_retrieval_entities(repository.connection())?;
    let relations = load_retrieval_relations(repository.connection(), &blocks)?;
    let digest = retrieval_authority_digest(&blocks, &claims, &entities, &relations);
    Ok(RetrievalAuthority {
        digest,
        blocks,
        claims,
        entities,
        relations,
        built_at_millis,
    })
}

fn load_retrieval_entities(
    connection: &Connection,
) -> Result<Vec<RetrievalEntityAuthority>, VaultError> {
    let mut statement = connection.prepare(
        "SELECT s.id, COALESCE(s.current_locator, s.source_locator),
                (SELECT v.evidence_id FROM source_record_versions v
                 WHERE v.source_record_id = s.id
                 ORDER BY v.version_ordinal DESC LIMIT 1)
         FROM source_records s ORDER BY s.id",
    )?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<i64>>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    rows.into_iter()
        .map(|(source_record_id, locator, evidence_id)| {
            let mut values = vec![locator];
            if let Some(evidence_id) = evidence_id {
                values.extend(load_strings(
                    connection,
                    "SELECT value FROM obsidian_aliases WHERE evidence_id = ?1 ORDER BY ordinal",
                    evidence_id,
                )?);
                values.extend(load_strings(
                    connection,
                    "SELECT value FROM obsidian_tags WHERE evidence_id = ?1 ORDER BY ordinal",
                    evidence_id,
                )?);
                let properties = load_string_pairs(
                    connection,
                    "SELECT name, value FROM obsidian_properties
                     WHERE evidence_id = ?1 ORDER BY property_ordinal, value_ordinal",
                    evidence_id,
                )?;
                for (name, value) in properties {
                    values.push(name);
                    values.push(value);
                }
            }
            let terms = values
                .iter()
                .flat_map(|value| search_terms(value))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            Ok(RetrievalEntityAuthority {
                source_record_id: u64::try_from(source_record_id)
                    .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                terms,
            })
        })
        .collect()
}

fn load_retrieval_relations(
    connection: &Connection,
    blocks: &[RetrievalBlockAuthority],
) -> Result<Vec<RetrievalRelationAuthority>, VaultError> {
    let mut statement = connection.prepare(
        "SELECT r.evidence_id, r.ordinal, r.relation_kind,
                r.start_byte, r.end_byte, x.resolved_source_record_id
         FROM obsidian_relations r
         JOIN obsidian_relation_resolutions x
           ON x.evidence_id = r.evidence_id AND x.relation_ordinal = r.ordinal
         ORDER BY r.evidence_id, r.ordinal",
    )?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(evidence_id, ordinal, kind, start, end, target)| {
            let evidence_id =
                u64::try_from(evidence_id).map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
            let start = usize::try_from(start).map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
            let end = usize::try_from(end).map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
            let from_ref = blocks
                .iter()
                .filter(|block| {
                    block.reference.evidence_id() == evidence_id
                        && block.start_byte <= start
                        && block.end_byte >= end
                })
                .min_by_key(|block| block.end_byte.saturating_sub(block.start_byte))
                .map(|block| block.reference)
                .ok_or(VaultError::InvalidKeyOrCorrupt)?;
            Ok(RetrievalRelationAuthority {
                from_ref,
                relation_ordinal: u64::try_from(ordinal)
                    .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                to_source_record_id: u64::try_from(target)
                    .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                relation_kind: kind,
            })
        })
        .collect()
}

fn retrieval_authority_digest(
    blocks: &[RetrievalBlockAuthority],
    claims: &[Claim],
    entities: &[RetrievalEntityAuthority],
    relations: &[RetrievalRelationAuthority],
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hash_bytes(&mut hasher, RETRIEVAL_INDEX_VERSION.as_bytes());
    for block in blocks {
        hash_u64(&mut hasher, block.reference.evidence_id());
        hash_u64(&mut hasher, block.reference.block_id().get());
        hash_u64(&mut hasher, block.source_record_id);
        hash_u64(&mut hasher, block.version_ordinal);
        hash_i64(&mut hasher, block.recorded_at_millis);
        hash_u64(
            &mut hasher,
            u64::try_from(block.start_byte).unwrap_or(u64::MAX),
        );
        hash_u64(
            &mut hasher,
            u64::try_from(block.end_byte).unwrap_or(u64::MAX),
        );
        hash_bytes(&mut hasher, block.quote.as_bytes());
    }
    for claim in claims {
        hash_u64(&mut hasher, claim.id().get());
        hash_i64(&mut hasher, encode_owner(claim.owner()));
        hash_bytes(&mut hasher, claim.statement().as_bytes());
        hash_i64(
            &mut hasher,
            claim.uncertainty().map_or(-1, encode_uncertainty),
        );
        let (kind, start, end) = encode_applicable_time(claim.applicable_time());
        hash_i64(&mut hasher, kind);
        hash_optional_i64(&mut hasher, start);
        hash_optional_i64(&mut hasher, end);
        hash_i64(&mut hasher, claim.recorded_at().as_millis());
        for citation in claim.support() {
            hash_u64(&mut hasher, citation.evidence_id().get());
            hash_bytes(&mut hasher, citation.quote().as_bytes());
        }
    }
    for entity in entities {
        hash_u64(&mut hasher, entity.source_record_id);
        for term in &entity.terms {
            hash_bytes(&mut hasher, term.as_bytes());
        }
    }
    for relation in relations {
        hash_u64(&mut hasher, relation.from_ref.evidence_id());
        hash_u64(&mut hasher, relation.from_ref.block_id().get());
        hash_u64(&mut hasher, relation.relation_ordinal);
        hash_u64(&mut hasher, relation.to_source_record_id);
        hash_i64(&mut hasher, relation.relation_kind);
    }
    hasher.finalize().into()
}

fn rebuild_retrieval_index(
    repository: &mut VaultRepository,
    authority: &RetrievalAuthority,
) -> Result<IndexBuildReceipt, VaultError> {
    let transaction = repository
        .connection
        .as_mut()
        .expect("an open vault always owns a database connection")
        .transaction()?;
    clear_retrieval_index(&transaction)?;
    insert_retrieval_blocks(&transaction, &authority.blocks)?;
    insert_retrieval_claims(&transaction, &authority.claims)?;
    insert_retrieval_graph(&transaction, &authority.entities, &authority.relations)?;
    let index_digest = retrieval_index_digest(&transaction)?;
    transaction.execute(
        "INSERT INTO retrieval_index_metadata
         (id, contract_version, authority_digest, index_digest, built_at,
          evidence_block_count, ledger_claim_count, relation_count)
         VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            RETRIEVAL_INDEX_VERSION,
            authority.digest.as_slice(),
            index_digest.as_slice(),
            authority.built_at_millis,
            i64::try_from(authority.blocks.len()).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
            i64::try_from(authority.claims.len()).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
            i64::try_from(authority.relations.len())
                .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
        ],
    )?;
    transaction.commit()?;
    Ok(authority.receipt(IndexDisposition::Rebuilt))
}

fn clear_retrieval_index(transaction: &rusqlite::Transaction<'_>) -> Result<(), VaultError> {
    for table in [
        "retrieval_index_metadata",
        "retrieval_block_vectors",
        "retrieval_relation_edges",
        "retrieval_entity_terms",
        "retrieval_claim_terms",
        "retrieval_claim_documents",
        "retrieval_block_terms",
        "retrieval_block_documents",
        "retrieval_evidence_availability",
    ] {
        transaction.execute(&format!("DELETE FROM {table}"), [])?;
    }
    Ok(())
}

fn insert_retrieval_blocks(
    transaction: &rusqlite::Transaction<'_>,
    blocks: &[RetrievalBlockAuthority],
) -> Result<(), VaultError> {
    let mut available_evidence = HashSet::new();
    for block in blocks {
        if available_evidence.insert(block.reference.evidence_id()) {
            transaction.execute(
                "INSERT INTO retrieval_evidence_availability (evidence_id, state)
                 VALUES (?1, 1)",
                [to_vault_sql_id(block.reference.evidence_id())?],
            )?;
        }
        let content_digest: [u8; 32] = Sha256::digest(block.quote.as_bytes()).into();
        transaction.execute(
            "INSERT INTO retrieval_block_documents
             (evidence_id, block_id, source_record_id, version_ordinal,
              recorded_at, content_digest)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                to_vault_sql_id(block.reference.evidence_id())?,
                to_vault_sql_id(block.reference.block_id().get())?,
                to_vault_sql_id(block.source_record_id)?,
                i64::try_from(block.version_ordinal)
                    .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                block.recorded_at_millis,
                content_digest.as_slice(),
            ],
        )?;
        for term in search_terms(&block.quote) {
            transaction.execute(
                "INSERT INTO retrieval_block_terms (term, evidence_id, block_id)
                 VALUES (?1, ?2, ?3)",
                params![
                    term,
                    to_vault_sql_id(block.reference.evidence_id())?,
                    to_vault_sql_id(block.reference.block_id().get())?,
                ],
            )?;
        }
        let embedding = embed_text(&block.quote);
        transaction.execute(
            "INSERT INTO retrieval_block_vectors
             (evidence_id, block_id, model_version, dimensions, embedding)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                to_vault_sql_id(block.reference.evidence_id())?,
                to_vault_sql_id(block.reference.block_id().get())?,
                EMBEDDING_MODEL_VERSION,
                i64::try_from(VECTOR_DIMENSIONS).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                embedding.to_le_bytes(),
            ],
        )?;
    }
    Ok(())
}

fn insert_retrieval_claims(
    transaction: &rusqlite::Transaction<'_>,
    claims: &[Claim],
) -> Result<(), VaultError> {
    for claim in claims {
        let (start, end, unknown) = retrieval_claim_interval(claim.applicable_time());
        let statement_digest: [u8; 32] = Sha256::digest(claim.statement().as_bytes()).into();
        transaction.execute(
            "INSERT INTO retrieval_claim_documents
             (claim_id, applicable_start, applicable_end, applicable_unknown,
              recorded_at, statement_digest)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                to_vault_sql_id(claim.id().get())?,
                start,
                end,
                i64::from(unknown),
                claim.recorded_at().as_millis(),
                statement_digest.as_slice(),
            ],
        )?;
        for term in search_terms(claim.statement()) {
            transaction.execute(
                "INSERT INTO retrieval_claim_terms (term, claim_id) VALUES (?1, ?2)",
                params![term, to_vault_sql_id(claim.id().get())?],
            )?;
        }
    }
    Ok(())
}

fn insert_retrieval_graph(
    transaction: &rusqlite::Transaction<'_>,
    entities: &[RetrievalEntityAuthority],
    relations: &[RetrievalRelationAuthority],
) -> Result<(), VaultError> {
    for entity in entities {
        for term in &entity.terms {
            transaction.execute(
                "INSERT INTO retrieval_entity_terms (term, source_record_id)
                 VALUES (?1, ?2)",
                params![term, to_vault_sql_id(entity.source_record_id)?],
            )?;
        }
    }
    for relation in relations {
        transaction.execute(
            "INSERT INTO retrieval_relation_edges
             (from_evidence_id, from_block_id, relation_ordinal,
              to_source_record_id, relation_kind)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                to_vault_sql_id(relation.from_ref.evidence_id())?,
                to_vault_sql_id(relation.from_ref.block_id().get())?,
                i64::try_from(relation.relation_ordinal)
                    .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                to_vault_sql_id(relation.to_source_record_id)?,
                relation.relation_kind,
            ],
        )?;
    }
    Ok(())
}

const fn retrieval_claim_interval(value: ApplicableTime) -> (Option<i64>, Option<i64>, bool) {
    match value {
        ApplicableTime::At(value) => (Some(value.as_millis()), Some(value.as_millis()), false),
        ApplicableTime::Since(value) => (Some(value.as_millis()), None, false),
        ApplicableTime::Between { start, end } => {
            (Some(start.as_millis()), Some(end.as_millis()), false)
        }
        ApplicableTime::Unknown => (None, None, true),
    }
}

fn hash_bytes(hasher: &mut Sha256, value: &[u8]) {
    hasher.update(u64::try_from(value.len()).unwrap_or(u64::MAX).to_le_bytes());
    hasher.update(value);
}

fn hash_u64(hasher: &mut Sha256, value: u64) {
    hasher.update(value.to_le_bytes());
}

fn hash_i64(hasher: &mut Sha256, value: i64) {
    hasher.update(value.to_le_bytes());
}

fn hash_optional_i64(hasher: &mut Sha256, value: Option<i64>) {
    hasher.update([u8::from(value.is_some())]);
    if let Some(value) = value {
        hash_i64(hasher, value);
    }
}

fn retrieval_index_digest(connection: &Connection) -> Result<[u8; 32], VaultError> {
    let mut hasher = Sha256::new();
    hash_bytes(&mut hasher, RETRIEVAL_INDEX_VERSION.as_bytes());
    hash_retrieval_block_index(connection, &mut hasher)?;
    hash_retrieval_vector_index(connection, &mut hasher)?;
    hash_retrieval_claim_index(connection, &mut hasher)?;
    hash_retrieval_graph_index(connection, &mut hasher)?;
    Ok(hasher.finalize().into())
}

fn hash_retrieval_vector_index(
    connection: &Connection,
    hasher: &mut Sha256,
) -> Result<(), VaultError> {
    let mut statement = connection.prepare(
        "SELECT evidence_id, block_id, model_version, dimensions, embedding
         FROM retrieval_block_vectors ORDER BY evidence_id, block_id",
    )?;
    for (evidence_id, block_id, model, dimensions, embedding) in statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Vec<u8>>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?
    {
        hash_i64(hasher, evidence_id);
        hash_i64(hasher, block_id);
        hash_bytes(hasher, model.as_bytes());
        hash_i64(hasher, dimensions);
        hash_bytes(hasher, &embedding);
    }
    Ok(())
}

fn retrieval_vector_index_is_complete(
    connection: &Connection,
    expected_blocks: usize,
) -> Result<bool, VaultError> {
    let (count, invalid): (i64, i64) = connection.query_row(
        "SELECT count(*),
                COALESCE(sum(CASE
                    WHEN model_version = ?1 AND dimensions = ?2 AND length(embedding) = 512
                    THEN 0 ELSE 1 END), 0)
         FROM retrieval_block_vectors",
        params![
            EMBEDDING_MODEL_VERSION,
            i64::try_from(VECTOR_DIMENSIONS).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
        ],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok(usize::try_from(count).ok() == Some(expected_blocks) && invalid == 0)
}

fn hash_retrieval_block_index(
    connection: &Connection,
    hasher: &mut Sha256,
) -> Result<(), VaultError> {
    let mut statement = connection.prepare(
        "SELECT evidence_id, state FROM retrieval_evidence_availability ORDER BY evidence_id",
    )?;
    for (evidence_id, state) in statement
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?
    {
        hash_i64(hasher, evidence_id);
        hash_i64(hasher, state);
    }

    let mut statement = connection.prepare(
        "SELECT evidence_id, block_id, source_record_id, version_ordinal,
                recorded_at, content_digest
         FROM retrieval_block_documents ORDER BY evidence_id, block_id",
    )?;
    for (evidence_id, block_id, source_id, version, recorded_at, digest) in statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, Vec<u8>>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?
    {
        for value in [evidence_id, block_id, source_id, version, recorded_at] {
            hash_i64(hasher, value);
        }
        hash_bytes(hasher, &digest);
    }

    let mut statement = connection.prepare(
        "SELECT term, evidence_id, block_id
         FROM retrieval_block_terms ORDER BY term, evidence_id, block_id",
    )?;
    for (term, evidence_id, block_id) in statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?
    {
        hash_bytes(hasher, term.as_bytes());
        hash_i64(hasher, evidence_id);
        hash_i64(hasher, block_id);
    }
    Ok(())
}

fn hash_retrieval_claim_index(
    connection: &Connection,
    hasher: &mut Sha256,
) -> Result<(), VaultError> {
    let mut statement = connection.prepare(
        "SELECT claim_id, applicable_start, applicable_end, applicable_unknown,
                recorded_at, statement_digest
         FROM retrieval_claim_documents ORDER BY claim_id",
    )?;
    for (claim_id, start, end, unknown, recorded_at, digest) in statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, Vec<u8>>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?
    {
        hash_i64(hasher, claim_id);
        hash_optional_i64(hasher, start);
        hash_optional_i64(hasher, end);
        hash_i64(hasher, unknown);
        hash_i64(hasher, recorded_at);
        hash_bytes(hasher, &digest);
    }

    let mut statement = connection
        .prepare("SELECT term, claim_id FROM retrieval_claim_terms ORDER BY term, claim_id")?;
    for (term, claim_id) in statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?
    {
        hash_bytes(hasher, term.as_bytes());
        hash_i64(hasher, claim_id);
    }
    Ok(())
}

fn hash_retrieval_graph_index(
    connection: &Connection,
    hasher: &mut Sha256,
) -> Result<(), VaultError> {
    let mut statement = connection.prepare(
        "SELECT term, source_record_id
         FROM retrieval_entity_terms ORDER BY term, source_record_id",
    )?;
    for (term, source_id) in statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?
    {
        hash_bytes(hasher, term.as_bytes());
        hash_i64(hasher, source_id);
    }

    let mut statement = connection.prepare(
        "SELECT from_evidence_id, from_block_id, relation_ordinal,
                to_source_record_id, relation_kind
         FROM retrieval_relation_edges
         ORDER BY from_evidence_id, relation_ordinal",
    )?;
    for row in statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?
    {
        for value in [row.0, row.1, row.2, row.3, row.4] {
            hash_i64(hasher, value);
        }
    }
    Ok(())
}

fn recall_retrieval_candidates(
    connection: &Connection,
    query: &RetrievalQuery,
) -> Result<Vec<RecallHit>, VaultError> {
    let mut hits = Vec::new();
    append_lexical_hits(connection, query.text(), &mut hits)?;
    append_vector_hits(connection, query.text(), &mut hits)?;
    append_temporal_hits(connection, query.time(), &mut hits)?;
    append_entity_hits(connection, query.entities(), &mut hits)?;
    let has_non_temporal_filter = query
        .text()
        .is_some_and(|text| !search_terms(text).is_empty())
        || query
            .entities()
            .iter()
            .any(|entity| !search_terms(entity).is_empty());
    if query.time().is_some() && has_non_temporal_filter {
        let temporally_valid = hits
            .iter()
            .filter(|hit| hit.channels().contains_temporal())
            .map(|hit| hit.reference())
            .collect::<BTreeSet<_>>();
        let recalled_by_content_or_relation = hits
            .iter()
            .filter(|hit| {
                hit.channels().contains_lexical()
                    || hit.channels().contains_vector()
                    || hit.channels().contains_relation()
            })
            .map(|hit| hit.reference())
            .collect::<BTreeSet<_>>();
        hits.retain(|hit| {
            temporally_valid.contains(&hit.reference())
                && recalled_by_content_or_relation.contains(&hit.reference())
        });
    }
    Ok(hits)
}

fn append_lexical_hits(
    connection: &Connection,
    text: Option<&str>,
    hits: &mut Vec<RecallHit>,
) -> Result<(), VaultError> {
    if let Some(text) = text {
        for term in search_terms(text) {
            let mut statement = connection.prepare(
                "SELECT evidence_id, block_id FROM retrieval_block_terms WHERE term = ?1
                 ORDER BY evidence_id, block_id",
            )?;
            for (evidence_id, block_id) in statement
                .query_map([&term], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?
            {
                hits.push(RecallHit::new(
                    CandidateRef::Evidence {
                        evidence_id: u64::try_from(evidence_id)
                            .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                        block_id: u64::try_from(block_id)
                            .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                    },
                    RecallChannels::lexical(),
                    1,
                ));
            }
            let mut statement = connection.prepare(
                "SELECT claim_id FROM retrieval_claim_terms WHERE term = ?1 ORDER BY claim_id",
            )?;
            for claim_id in statement
                .query_map([&term], |row| row.get::<_, i64>(0))?
                .collect::<Result<Vec<_>, _>>()?
            {
                hits.push(RecallHit::new(
                    CandidateRef::Ledger {
                        claim_id: u64::try_from(claim_id)
                            .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                    },
                    RecallChannels::lexical(),
                    1,
                ));
            }
        }
    }
    Ok(())
}

fn append_vector_hits(
    connection: &Connection,
    text: Option<&str>,
    hits: &mut Vec<RecallHit>,
) -> Result<(), VaultError> {
    let Some(text) = text else {
        return Ok(());
    };
    let query_vector = embed_text(text);
    if query_vector.is_zero() {
        return Ok(());
    }
    let mut statement = connection.prepare(
        "SELECT evidence_id, block_id, dimensions, embedding
         FROM retrieval_block_vectors WHERE model_version = ?1
         ORDER BY evidence_id, block_id",
    )?;
    let mut ranked = Vec::new();
    for (evidence_id, block_id, dimensions, bytes) in statement
        .query_map([EMBEDDING_MODEL_VERSION], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Vec<u8>>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?
    {
        if usize::try_from(dimensions).ok() != Some(VECTOR_DIMENSIONS) {
            return Err(VaultError::InvalidKeyOrCorrupt);
        }
        let vector =
            VectorEmbedding::from_le_bytes(&bytes).map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
        let score = cosine_similarity_bps(&query_vector, &vector);
        if score >= VECTOR_MIN_SCORE_BPS {
            ranked.push((
                CandidateRef::Evidence {
                    evidence_id: u64::try_from(evidence_id)
                        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                    block_id: u64::try_from(block_id)
                        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                },
                score,
            ));
        }
    }
    ranked.sort_by_key(|(reference, score)| (std::cmp::Reverse(*score), *reference));
    hits.extend(
        ranked
            .into_iter()
            .take(MAX_VECTOR_CANDIDATES)
            .map(|(reference, score)| RecallHit::vector(reference, score)),
    );
    Ok(())
}

fn append_temporal_hits(
    connection: &Connection,
    time: Option<eam_retrieval::TimeRange>,
    hits: &mut Vec<RecallHit>,
) -> Result<(), VaultError> {
    if let Some(time) = time {
        let mut statement = connection.prepare(
            "SELECT evidence_id, block_id FROM retrieval_block_documents
             WHERE recorded_at BETWEEN ?1 AND ?2 ORDER BY evidence_id, block_id",
        )?;
        for (evidence_id, block_id) in statement
            .query_map(params![time.start_millis(), time.end_millis()], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?
        {
            hits.push(RecallHit::new(
                CandidateRef::Evidence {
                    evidence_id: u64::try_from(evidence_id)
                        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                    block_id: u64::try_from(block_id)
                        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                },
                RecallChannels::temporal(),
                0,
            ));
        }
        let mut statement = connection.prepare(
            "SELECT claim_id FROM retrieval_claim_documents
             WHERE applicable_unknown = 0
               AND applicable_start <= ?2
               AND (applicable_end IS NULL OR applicable_end >= ?1)
             ORDER BY claim_id",
        )?;
        for claim_id in statement
            .query_map(params![time.start_millis(), time.end_millis()], |row| {
                row.get::<_, i64>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?
        {
            hits.push(RecallHit::new(
                CandidateRef::Ledger {
                    claim_id: u64::try_from(claim_id)
                        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                },
                RecallChannels::temporal(),
                0,
            ));
        }
    }
    Ok(())
}

fn append_entity_hits(
    connection: &Connection,
    entities: &[String],
    hits: &mut Vec<RecallHit>,
) -> Result<(), VaultError> {
    let mut related_sources = BTreeSet::new();
    for entity in entities {
        for term in search_terms(entity) {
            let mut statement = connection.prepare(
                "SELECT source_record_id FROM retrieval_entity_terms
                 WHERE term = ?1 ORDER BY source_record_id",
            )?;
            for source_id in statement
                .query_map([&term], |row| row.get::<_, i64>(0))?
                .collect::<Result<Vec<_>, _>>()?
            {
                related_sources
                    .insert(u64::try_from(source_id).map_err(|_| VaultError::InvalidKeyOrCorrupt)?);
            }
        }
    }
    for source_id in related_sources {
        append_relation_hits(connection, source_id, hits)?;
    }
    Ok(())
}

fn append_relation_hits(
    connection: &Connection,
    source_record_id: u64,
    hits: &mut Vec<RecallHit>,
) -> Result<(), VaultError> {
    let source_id = to_vault_sql_id(source_record_id)?;
    let queries = [
        "SELECT evidence_id, block_id FROM retrieval_block_documents
         WHERE source_record_id = ?1 ORDER BY evidence_id, block_id",
        "SELECT from_evidence_id, from_block_id FROM retrieval_relation_edges
         WHERE to_source_record_id = ?1 ORDER BY from_evidence_id, from_block_id",
        "SELECT target.evidence_id, target.block_id
         FROM retrieval_relation_edges edge
         JOIN retrieval_block_documents source
           ON source.evidence_id = edge.from_evidence_id
          AND source.block_id = edge.from_block_id
         JOIN retrieval_block_documents target
           ON target.source_record_id = edge.to_source_record_id
         WHERE source.source_record_id = ?1
         ORDER BY target.evidence_id, target.block_id",
    ];
    for query in queries {
        let mut statement = connection.prepare(query)?;
        for (evidence_id, block_id) in statement
            .query_map([source_id], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?
        {
            hits.push(RecallHit::new(
                CandidateRef::Evidence {
                    evidence_id: u64::try_from(evidence_id)
                        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                    block_id: u64::try_from(block_id)
                        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                },
                RecallChannels::relation(),
                0,
            ));
        }
    }
    Ok(())
}

fn recall_retrieval_neighbors(
    connection: &Connection,
    reference: CandidateRef,
) -> Result<Vec<RecallHit>, VaultError> {
    let CandidateRef::Evidence {
        evidence_id,
        block_id,
    } = reference
    else {
        return Ok(Vec::new());
    };
    let evidence_id_sql = to_vault_sql_id(evidence_id)?;
    let block_id_sql = to_vault_sql_id(block_id)?;
    let (source_record_id, recorded_at, ordinal) = connection
        .query_row(
            "SELECT d.source_record_id, d.recorded_at, b.ordinal
             FROM retrieval_block_documents d
             JOIN evidence_blocks b
               ON b.evidence_id = d.evidence_id AND b.id = d.block_id
             WHERE d.evidence_id = ?1 AND d.block_id = ?2",
            params![evidence_id_sql, block_id_sql],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()?
        .ok_or(VaultError::InvalidKeyOrCorrupt)?;
    let mut hits = structural_neighbor_hits(connection, evidence_id_sql, block_id_sql, ordinal)?;
    hits.extend(temporal_neighbor_hits(
        connection,
        source_record_id,
        recorded_at,
        reference,
    )?);
    let mut relation_hits = Vec::new();
    append_relation_hits(
        connection,
        u64::try_from(source_record_id).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
        &mut relation_hits,
    )?;
    let mut relation_count = 0;
    for hit in relation_hits {
        if hit.reference() == reference
            || candidate_source_record_id(connection, hit.reference())? == source_record_id
        {
            continue;
        }
        hits.push(hit);
        relation_count += 1;
        if relation_count == MAX_RELATION_NEIGHBORS {
            break;
        }
    }
    Ok(hits)
}

fn structural_neighbor_hits(
    connection: &Connection,
    evidence_id: i64,
    block_id: i64,
    ordinal: i64,
) -> Result<Vec<RecallHit>, VaultError> {
    let mut statement = connection.prepare(
        "SELECT d.evidence_id, d.block_id
         FROM retrieval_block_documents d
         JOIN evidence_blocks b
           ON b.evidence_id = d.evidence_id AND b.id = d.block_id
         WHERE d.evidence_id = ?1 AND d.block_id != ?2
           AND b.ordinal BETWEEN ?3 - 1 AND ?3 + 1
         ORDER BY b.ordinal",
    )?;
    statement
        .query_map(params![evidence_id, block_id, ordinal], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })?
        .map(|row| {
            let (evidence_id, block_id) = row?;
            Ok(RecallHit::new(
                CandidateRef::Evidence {
                    evidence_id: u64::try_from(evidence_id)
                        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                    block_id: u64::try_from(block_id)
                        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                },
                RecallChannels::default(),
                0,
            ))
        })
        .collect()
}

fn temporal_neighbor_hits(
    connection: &Connection,
    source_record_id: i64,
    recorded_at: i64,
    seed: CandidateRef,
) -> Result<Vec<RecallHit>, VaultError> {
    let CandidateRef::Evidence {
        evidence_id: seed_evidence_id,
        ..
    } = seed
    else {
        return Ok(Vec::new());
    };
    let start = recorded_at.saturating_sub(TEMPORAL_NEIGHBOR_RADIUS_MILLIS);
    let end = recorded_at.saturating_add(TEMPORAL_NEIGHBOR_RADIUS_MILLIS);
    let mut statement = connection.prepare(
        "SELECT evidence_id, block_id, recorded_at
         FROM retrieval_block_documents
         WHERE source_record_id = ?1 AND recorded_at BETWEEN ?2 AND ?3
           AND evidence_id != ?4
         ORDER BY abs(recorded_at - ?5), evidence_id, block_id",
    )?;
    let mut hits = Vec::new();
    for (evidence_id, block_id, _) in statement
        .query_map(
            params![
                source_record_id,
                start,
                end,
                to_vault_sql_id(seed_evidence_id)?,
                recorded_at
            ],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )?
        .collect::<Result<Vec<_>, _>>()?
    {
        let reference = CandidateRef::Evidence {
            evidence_id: u64::try_from(evidence_id).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
            block_id: u64::try_from(block_id).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
        };
        if reference != seed {
            hits.push(RecallHit::new(reference, RecallChannels::temporal(), 0));
            if hits.len() == MAX_TEMPORAL_NEIGHBORS {
                break;
            }
        }
    }
    Ok(hits)
}

fn candidate_source_record_id(
    connection: &Connection,
    reference: CandidateRef,
) -> Result<i64, VaultError> {
    let CandidateRef::Evidence {
        evidence_id,
        block_id,
    } = reference
    else {
        return Err(VaultError::InvalidKeyOrCorrupt);
    };
    connection
        .query_row(
            "SELECT source_record_id FROM retrieval_block_documents
             WHERE evidence_id = ?1 AND block_id = ?2",
            params![to_vault_sql_id(evidence_id)?, to_vault_sql_id(block_id)?,],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(VaultError::InvalidKeyOrCorrupt)
}

fn resolve_retrieval_candidate(
    repository: &VaultRepository,
    candidate: CandidateRef,
    scope: SourceScope,
) -> Result<Option<AuthoritativeCandidate>, VaultError> {
    match candidate {
        CandidateRef::Evidence {
            evidence_id,
            block_id,
        } => resolve_retrieval_evidence(repository, evidence_id, block_id, scope),
        CandidateRef::Ledger { claim_id } => {
            resolve_retrieval_claim(repository, claim_id).map(Some)
        }
    }
}

fn resolve_retrieval_evidence(
    repository: &VaultRepository,
    evidence_id: u64,
    block_id: u64,
    scope: SourceScope,
) -> Result<Option<AuthoritativeCandidate>, VaultError> {
    let evidence_id_sql = to_vault_sql_id(evidence_id)?;
    let source = repository
        .connection()
        .query_row(
            "SELECT v.source_record_id,
                    CASE WHEN s.origin_kind = 1 THEN s.current_locator
                         ELSE s.source_locator END,
                    s.record_state, a.archived_at,
                    v.version_ordinal = (
                        SELECT MAX(latest.version_ordinal)
                        FROM source_record_versions latest
                        WHERE latest.source_record_id = v.source_record_id
                    )
             FROM source_record_versions v
             JOIN source_records s ON s.id = v.source_record_id
             JOIN archived_evidence a ON a.id = v.evidence_id
             JOIN retrieval_evidence_availability available
               ON available.evidence_id = v.evidence_id AND available.state = 1
             WHERE v.evidence_id = ?1",
            [evidence_id_sql],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, bool>(4)?,
                ))
            },
        )
        .optional()?
        .ok_or(VaultError::InvalidKeyOrCorrupt)?;
    let currentness = match source.2 {
        0 => SourceCurrentness::Present,
        1 => SourceCurrentness::SourceRemoved,
        _ => return Err(VaultError::InvalidKeyOrCorrupt),
    };
    if scope == SourceScope::Current
        && (currentness == SourceCurrentness::SourceRemoved || !source.4)
    {
        return Ok(None);
    }
    let reference = EvidenceBlockRef::new(
        evidence_id,
        EvidenceBlockId::new(block_id).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
    )
    .map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
    let canonical =
        EvidenceBlockQueryRepository::load_canonical_evidence_block(repository, reference)?
            .ok_or(VaultError::InvalidKeyOrCorrupt)?;
    let canonical_text = std::str::from_utf8(canonical.canonical_bytes())
        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
    let view = EvidenceBlockView::new(canonical.block().clone(), canonical_text)
        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
    let indexed_digest = repository
        .connection()
        .query_row(
            "SELECT content_digest FROM retrieval_block_documents
             WHERE evidence_id = ?1 AND block_id = ?2",
            params![evidence_id_sql, to_vault_sql_id(block_id)?],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()?
        .ok_or(VaultError::InvalidKeyOrCorrupt)?;
    let authority_digest: [u8; 32] = Sha256::digest(view.verbatim().as_bytes()).into();
    if indexed_digest.as_slice() != authority_digest {
        return Err(VaultError::InvalidKeyOrCorrupt);
    }
    Ok(Some(AuthoritativeCandidate::Evidence(
        AuthoritativeEvidence::new(
            view,
            u64::try_from(source.0).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
            source.1.ok_or(VaultError::InvalidKeyOrCorrupt)?,
            currentness,
            source.3,
        ),
    )))
}

fn resolve_retrieval_claim(
    repository: &VaultRepository,
    claim_id: u64,
) -> Result<AuthoritativeCandidate, VaultError> {
    let claim = MemoryRepository::all_claims(repository)
        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?
        .into_iter()
        .find(|claim| claim.id() == ClaimId::from_raw(claim_id))
        .ok_or(VaultError::InvalidKeyOrCorrupt)?;
    if claim.support().is_empty() {
        return Err(VaultError::InvalidKeyOrCorrupt);
    }
    for citation in claim.support() {
        let evidence = MemoryRepository::evidence(repository, citation.evidence_id())
            .map_err(|_| VaultError::InvalidKeyOrCorrupt)?
            .ok_or(VaultError::InvalidKeyOrCorrupt)?;
        if citation.quote().is_empty() || !evidence.verbatim().contains(citation.quote()) {
            return Err(VaultError::InvalidKeyOrCorrupt);
        }
    }
    let indexed_digest = repository
        .connection()
        .query_row(
            "SELECT statement_digest FROM retrieval_claim_documents WHERE claim_id = ?1",
            [to_vault_sql_id(claim_id)?],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()?
        .ok_or(VaultError::InvalidKeyOrCorrupt)?;
    let authority_digest: [u8; 32] = Sha256::digest(claim.statement().as_bytes()).into();
    if indexed_digest.as_slice() != authority_digest {
        return Err(VaultError::InvalidKeyOrCorrupt);
    }
    Ok(AuthoritativeCandidate::Ledger(claim))
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

struct SelectedSourceRecord {
    id: u64,
    is_new: bool,
    previous_locator: Option<String>,
}

fn select_source_record(
    connection: &Connection,
    input: &SourceArchiveInput<'_>,
    object_id: &str,
    next_source_record_id: u64,
) -> Result<SelectedSourceRecord, VaultError> {
    let root_id_sql = to_vault_sql_id(input.root_id)?;
    let root_exists = connection
        .query_row(
            "SELECT 1 FROM source_roots WHERE id = ?1",
            [root_id_sql],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !root_exists {
        return Err(VaultError::InvalidKeyOrCorrupt);
    }
    let exact = connection
        .query_row(
            "SELECT id FROM source_records
             WHERE origin_kind = 1 AND root_id = ?1 AND current_locator = ?2",
            params![root_id_sql, input.relative_path],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .map(|id| u64::try_from(id).map_err(|_| VaultError::InvalidKeyOrCorrupt))
        .transpose()?;
    if let Some(id) = exact {
        return Ok(SelectedSourceRecord {
            id,
            is_new: false,
            previous_locator: None,
        });
    }
    let observed = input
        .observed_relative_paths
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let claimed = input
        .claimed_source_record_ids
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    let mut statement = connection.prepare(
        "SELECT s.id, s.current_locator
         FROM source_records s
         JOIN source_record_versions v ON v.source_record_id = s.id
         JOIN archived_evidence a ON a.id = v.evidence_id
         WHERE s.origin_kind = 1 AND s.root_id = ?1
           AND a.object_id = ?2
           AND v.version_ordinal = (
               SELECT MAX(v2.version_ordinal)
               FROM source_record_versions v2
               WHERE v2.source_record_id = s.id
           )",
    )?;
    let mut move_candidates = statement
        .query_map(params![root_id_sql, object_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter_map(|(id, locator)| {
            let id = u64::try_from(id).ok()?;
            (!claimed.contains(&id) && !observed.contains(locator.as_str()))
                .then_some((id, locator))
        })
        .collect::<Vec<_>>();
    if move_candidates.len() == 1 {
        let (id, previous_locator) = move_candidates.pop().expect("one move candidate");
        return Ok(SelectedSourceRecord {
            id,
            is_new: false,
            previous_locator: Some(previous_locator),
        });
    }
    Ok(SelectedSourceRecord {
        id: next_source_record_id,
        is_new: true,
        previous_locator: None,
    })
}

fn find_source_version(
    connection: &Connection,
    source_record_id: u64,
    object_id: &str,
) -> Result<Option<u64>, VaultError> {
    connection
        .query_row(
            "SELECT a.id
             FROM source_record_versions v
             JOIN archived_evidence a ON a.id = v.evidence_id
             WHERE v.source_record_id = ?1 AND a.object_id = ?2",
            params![to_vault_sql_id(source_record_id)?, object_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .map(|id| u64::try_from(id).map_err(|_| VaultError::InvalidKeyOrCorrupt))
        .transpose()
}

fn commit_source_file_observation(
    connection: &mut Connection,
    input: &SourceArchiveInput<'_>,
    selected: &SelectedSourceRecord,
    object_id: &str,
    existing_version: Option<u64>,
    next_archive_id: u64,
) -> Result<u64, VaultError> {
    let transaction = connection.transaction()?;
    commit_source_record_state(&transaction, input, selected)?;
    let archive_id = if let Some(existing) = existing_version {
        existing
    } else {
        insert_source_archive_version(
            &transaction,
            input,
            selected.id,
            object_id,
            next_archive_id,
        )?;
        next_archive_id
    };
    transaction.commit()?;
    Ok(archive_id)
}

fn commit_source_record_state(
    transaction: &rusqlite::Transaction<'_>,
    input: &SourceArchiveInput<'_>,
    selected: &SelectedSourceRecord,
) -> Result<(), VaultError> {
    if selected.is_new {
        let stable_locator = format!("obsidian:{}:{}", input.root_id, selected.id);
        transaction.execute(
            "INSERT INTO source_records
             (id, source_kind, source_locator, origin_kind, root_id,
              current_locator, record_state, first_seen_at, last_seen_at)
             VALUES (?1, 0, ?2, 1, ?3, ?4, 0, ?5, ?5)",
            params![
                to_vault_sql_id(selected.id)?,
                stable_locator,
                to_vault_sql_id(input.root_id)?,
                input.relative_path,
                input.observed_at_millis,
            ],
        )?;
        return insert_source_record_event(
            transaction,
            selected.id,
            SourceRecordState::Present,
            input.relative_path,
            input.observed_at_millis,
        );
    }
    let (state, old_locator) = transaction.query_row(
        "SELECT record_state, current_locator FROM source_records WHERE id = ?1",
        [to_vault_sql_id(selected.id)?],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
    )?;
    transaction.execute(
        "UPDATE source_records
         SET current_locator = ?1, record_state = 0, last_seen_at = ?2
         WHERE id = ?3",
        params![
            input.relative_path,
            input.observed_at_millis,
            to_vault_sql_id(selected.id)?
        ],
    )?;
    if state != encode_source_record_state(SourceRecordState::Present)
        || old_locator != input.relative_path
    {
        insert_source_record_event(
            transaction,
            selected.id,
            SourceRecordState::Present,
            input.relative_path,
            input.observed_at_millis,
        )?;
    }
    Ok(())
}

fn insert_source_archive_version(
    transaction: &rusqlite::Transaction<'_>,
    input: &SourceArchiveInput<'_>,
    source_record_id: u64,
    object_id: &str,
    archive_id: u64,
) -> Result<(), VaultError> {
    let archive_status = match input.kind {
        SourceFileKind::Markdown => ArchiveStatus::Archived,
        SourceFileKind::Attachment => {
            ArchiveStatus::ArchivedUnparsed(UnparsedReason::UnsupportedFormat)
        }
    };
    let (status, unparsed_reason) = encode_archive_status(archive_status);
    let stable_locator = format!("obsidian:{}:{source_record_id}", input.root_id);
    transaction.execute(
        "INSERT INTO archived_evidence
         (id, source_kind, source_locator, object_id, content_length,
          status, unparsed_reason, archived_at)
         VALUES (?1, 0, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            to_vault_sql_id(archive_id)?,
            stable_locator,
            object_id,
            i64::try_from(input.content.len()).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
            status,
            unparsed_reason,
            input.observed_at_millis,
        ],
    )?;
    let version_ordinal: i64 = transaction.query_row(
        "SELECT COALESCE(MAX(version_ordinal), -1) + 1
         FROM source_record_versions WHERE source_record_id = ?1",
        [to_vault_sql_id(source_record_id)?],
        |row| row.get(0),
    )?;
    transaction.execute(
        "INSERT INTO source_record_versions
         (source_record_id, evidence_id, version_ordinal) VALUES (?1, ?2, ?3)",
        params![
            to_vault_sql_id(source_record_id)?,
            to_vault_sql_id(archive_id)?,
            version_ordinal,
        ],
    )?;
    Ok(())
}

fn load_source_root_snapshot(
    connection: &Connection,
    root_id: u64,
) -> Result<SourceRootSnapshot, VaultError> {
    let root_id_sql = to_vault_sql_id(root_id)?;
    let root = connection
        .query_row(
            "SELECT root_locator, availability, first_seen_at, last_reconciled_at
             FROM source_roots WHERE id = ?1",
            [root_id_sql],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                ))
            },
        )
        .optional()?
        .ok_or(VaultError::InvalidKeyOrCorrupt)?;
    let root = SourceRoot::new(
        root_id,
        root.0,
        decode_source_availability(root.1)?,
        root.2,
        root.3,
    )
    .map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
    let mut statement = connection.prepare(
        "SELECT s.id, s.current_locator, s.record_state, s.first_seen_at,
                s.last_seen_at,
                (SELECT v.evidence_id FROM source_record_versions v
                 WHERE v.source_record_id = s.id
                 ORDER BY v.version_ordinal DESC LIMIT 1)
         FROM source_records s
         WHERE s.origin_kind = 1 AND s.root_id = ?1 ORDER BY s.id",
    )?;
    let records = statement
        .query_map([root_id_sql], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, Option<i64>>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|(id, locator, state, first_seen, last_seen, evidence_id)| {
            SourceRecord::new(
                u64::try_from(id).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                root_id,
                locator.ok_or(VaultError::InvalidKeyOrCorrupt)?,
                decode_source_record_state(state)?,
                first_seen,
                last_seen,
                evidence_id
                    .map(|value| u64::try_from(value).map_err(|_| VaultError::InvalidKeyOrCorrupt))
                    .transpose()?,
            )
            .map_err(|_| VaultError::InvalidKeyOrCorrupt)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(SourceRootSnapshot::new(root, records))
}

fn insert_source_root_event(
    transaction: &rusqlite::Transaction<'_>,
    root_id: u64,
    availability: SourceAvailability,
    occurred_at_millis: i64,
) -> Result<(), VaultError> {
    let ordinal: i64 = transaction.query_row(
        "SELECT COALESCE(MAX(ordinal), -1) + 1
         FROM source_root_state_events WHERE root_id = ?1",
        [to_vault_sql_id(root_id)?],
        |row| row.get(0),
    )?;
    transaction.execute(
        "INSERT INTO source_root_state_events
         (root_id, ordinal, availability, occurred_at) VALUES (?1, ?2, ?3, ?4)",
        params![
            to_vault_sql_id(root_id)?,
            ordinal,
            encode_source_availability(availability),
            occurred_at_millis,
        ],
    )?;
    Ok(())
}

fn insert_source_record_event(
    transaction: &rusqlite::Transaction<'_>,
    source_record_id: u64,
    state: SourceRecordState,
    locator: &str,
    occurred_at_millis: i64,
) -> Result<(), VaultError> {
    let ordinal: i64 = transaction.query_row(
        "SELECT COALESCE(MAX(ordinal), -1) + 1
         FROM source_record_state_events WHERE source_record_id = ?1",
        [to_vault_sql_id(source_record_id)?],
        |row| row.get(0),
    )?;
    transaction.execute(
        "INSERT INTO source_record_state_events
         (source_record_id, ordinal, record_state, locator, occurred_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            to_vault_sql_id(source_record_id)?,
            ordinal,
            encode_source_record_state(state),
            locator,
            occurred_at_millis,
        ],
    )?;
    Ok(())
}

fn persist_obsidian_parse_projection(
    transaction: &rusqlite::Transaction<'_>,
    evidence_id: i64,
    parsed: &ParsedMarkdownV1,
) -> Result<(), VaultError> {
    let is_obsidian = transaction
        .query_row(
            "SELECT 1
             FROM source_record_versions v
             JOIN source_records s ON s.id = v.source_record_id
             WHERE v.evidence_id = ?1 AND s.origin_kind = 1",
            [evidence_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !is_obsidian {
        return Ok(());
    }
    let mut alias_ordinal = 0_i64;
    for (property_ordinal, property) in parsed.properties.iter().enumerate() {
        for (value_ordinal, value) in property.values.iter().enumerate() {
            transaction.execute(
                "INSERT INTO obsidian_properties
                 (evidence_id, property_ordinal, value_ordinal, name, value)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    evidence_id,
                    i64::try_from(property_ordinal).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                    i64::try_from(value_ordinal).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                    property.name,
                    value,
                ],
            )?;
            if property.name.eq_ignore_ascii_case("aliases") {
                transaction.execute(
                    "INSERT INTO obsidian_aliases (evidence_id, ordinal, value)
                     VALUES (?1, ?2, ?3)",
                    params![evidence_id, alias_ordinal, value],
                )?;
                alias_ordinal += 1;
            }
        }
    }
    for (ordinal, tag) in parsed.tags.iter().enumerate() {
        transaction.execute(
            "INSERT INTO obsidian_tags (evidence_id, ordinal, value)
             VALUES (?1, ?2, ?3)",
            params![
                evidence_id,
                i64::try_from(ordinal).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                tag.value,
            ],
        )?;
    }
    for (ordinal, relation) in parsed.relations.iter().enumerate() {
        transaction.execute(
            "INSERT INTO obsidian_relations
             (evidence_id, ordinal, relation_kind, target, alias, heading,
              block_id, start_byte, end_byte)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                evidence_id,
                i64::try_from(ordinal).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                encode_markdown_relation_kind(relation.kind),
                relation.target,
                relation.alias,
                relation.heading,
                relation.block_id,
                i64::try_from(relation.source_span.start_byte)
                    .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                i64::try_from(relation.source_span.end_byte)
                    .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
            ],
        )?;
    }
    Ok(())
}

#[derive(Clone)]
struct RelationTargetCandidate {
    id: u64,
    locator: String,
    aliases: Vec<String>,
}

fn refresh_obsidian_relation_resolutions(
    connection: &mut Connection,
    root_id: u64,
) -> Result<(), VaultError> {
    let root_id_sql = to_vault_sql_id(root_id)?;
    let mut candidate_statement = connection.prepare(
        "SELECT s.id, s.current_locator,
                (SELECT v.evidence_id FROM source_record_versions v
                 WHERE v.source_record_id = s.id
                 ORDER BY v.version_ordinal DESC LIMIT 1)
         FROM source_records s
         WHERE s.origin_kind = 1 AND s.root_id = ?1 AND s.record_state = 0
         ORDER BY s.id",
    )?;
    let candidate_rows = candidate_statement
        .query_map([root_id_sql], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<i64>>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(candidate_statement);
    let mut candidates = Vec::with_capacity(candidate_rows.len());
    for (id, locator, evidence_id) in candidate_rows {
        let aliases = if let Some(evidence_id) = evidence_id {
            load_strings(
                connection,
                "SELECT value FROM obsidian_aliases
                 WHERE evidence_id = ?1 ORDER BY ordinal",
                evidence_id,
            )?
        } else {
            Vec::new()
        };
        candidates.push(RelationTargetCandidate {
            id: u64::try_from(id).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
            locator,
            aliases,
        });
    }
    let mut relation_statement = connection.prepare(
        "SELECT r.evidence_id, r.ordinal, r.target, s.id, s.current_locator
         FROM obsidian_relations r
         JOIN source_record_versions v ON v.evidence_id = r.evidence_id
         JOIN source_records s ON s.id = v.source_record_id
         WHERE s.root_id = ?1 ORDER BY r.evidence_id, r.ordinal",
    )?;
    let relations = relation_statement
        .query_map([root_id_sql], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(relation_statement);
    let transaction = connection.transaction()?;
    transaction.execute(
        "DELETE FROM obsidian_relation_resolutions
         WHERE evidence_id IN (
             SELECT v.evidence_id
             FROM source_record_versions v
             JOIN source_records s ON s.id = v.source_record_id
             WHERE s.root_id = ?1
         )",
        [root_id_sql],
    )?;
    for (evidence_id, ordinal, target, source_record_id, source_locator) in relations {
        let source_record_id =
            u64::try_from(source_record_id).map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
        if let Some(resolved) =
            resolve_relation_target(&target, source_record_id, &source_locator, &candidates)
        {
            transaction.execute(
                "INSERT INTO obsidian_relation_resolutions
                 (evidence_id, relation_ordinal, resolved_source_record_id)
                 VALUES (?1, ?2, ?3)",
                params![evidence_id, ordinal, to_vault_sql_id(resolved)?],
            )?;
        }
    }
    transaction.commit()?;
    Ok(())
}

fn resolve_relation_target(
    target: &str,
    source_record_id: u64,
    source_locator: &str,
    candidates: &[RelationTargetCandidate],
) -> Option<u64> {
    let target = target.trim();
    if target.is_empty() {
        return Some(source_record_id);
    }
    let lower = target.to_lowercase();
    if lower.contains("://") || lower.starts_with("mailto:") || lower.starts_with("data:") {
        return None;
    }
    let target_variants = relation_target_variants(source_locator, target);
    let mut matches = candidates
        .iter()
        .filter(|candidate| {
            let locator = candidate.locator.to_lowercase();
            let without_markdown = locator.strip_suffix(".md").unwrap_or(&locator);
            let stem = locator
                .rsplit('/')
                .next()
                .and_then(|name| name.strip_suffix(".md"))
                .unwrap_or_else(|| locator.rsplit('/').next().unwrap_or(&locator));
            target_variants
                .iter()
                .any(|target| target == &locator || target == without_markdown || target == stem)
                || candidate
                    .aliases
                    .iter()
                    .any(|alias| alias.to_lowercase() == lower)
        })
        .map(|candidate| candidate.id)
        .collect::<Vec<_>>();
    matches.sort_unstable();
    matches.dedup();
    (matches.len() == 1).then(|| matches[0])
}

fn relation_target_variants(source_locator: &str, target: &str) -> HashSet<String> {
    let target = target.replace('\\', "/");
    let mut variants = HashSet::from([target.trim_start_matches('/').to_lowercase()]);
    let parent = source_locator
        .rsplit_once('/')
        .map_or("", |(parent, _)| parent);
    let joined = if parent.is_empty() {
        target.clone()
    } else {
        format!("{parent}/{target}")
    };
    if let Some(normalized) = normalize_relative_segments(&joined) {
        variants.insert(normalized.to_lowercase());
    }
    variants
}

fn normalize_relative_segments(value: &str) -> Option<String> {
    let mut parts = Vec::new();
    for part in value.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            value => parts.push(value),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn load_strings(connection: &Connection, query: &str, id: i64) -> Result<Vec<String>, VaultError> {
    let mut statement = connection.prepare(query)?;
    statement
        .query_map([id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(VaultError::from)
}

fn load_string_pairs(
    connection: &Connection,
    query: &str,
    id: i64,
) -> Result<Vec<(String, String)>, VaultError> {
    let mut statement = connection.prepare(query)?;
    statement
        .query_map([id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(VaultError::from)
}

const fn encode_source_availability(value: SourceAvailability) -> i64 {
    match value {
        SourceAvailability::Available => 0,
        SourceAvailability::SourceUnavailable => 1,
    }
}

const fn decode_source_availability(value: i64) -> Result<SourceAvailability, VaultError> {
    match value {
        0 => Ok(SourceAvailability::Available),
        1 => Ok(SourceAvailability::SourceUnavailable),
        _ => Err(VaultError::InvalidKeyOrCorrupt),
    }
}

const fn encode_source_record_state(value: SourceRecordState) -> i64 {
    match value {
        SourceRecordState::Present => 0,
        SourceRecordState::SourceRemoved => 1,
    }
}

const fn decode_source_record_state(value: i64) -> Result<SourceRecordState, VaultError> {
    match value {
        0 => Ok(SourceRecordState::Present),
        1 => Ok(SourceRecordState::SourceRemoved),
        _ => Err(VaultError::InvalidKeyOrCorrupt),
    }
}

const fn encode_markdown_relation_kind(value: MarkdownRelationKind) -> i64 {
    match value {
        MarkdownRelationKind::Link => 0,
        MarkdownRelationKind::Image => 1,
        MarkdownRelationKind::Autolink => 2,
        MarkdownRelationKind::Wikilink => 3,
        MarkdownRelationKind::Embed => 4,
    }
}

const fn decode_source_relation_kind(value: i64) -> Result<SourceRelationKind, VaultError> {
    match value {
        0 => Ok(SourceRelationKind::Link),
        1 => Ok(SourceRelationKind::Image),
        2 => Ok(SourceRelationKind::Autolink),
        3 => Ok(SourceRelationKind::Wikilink),
        4 => Ok(SourceRelationKind::Embed),
        _ => Err(VaultError::InvalidKeyOrCorrupt),
    }
}

fn is_normalized_source_locator(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.ends_with('/')
        && !value.contains('\\')
        && value
            .split('/')
            .all(|component| !component.is_empty() && component != "." && component != "..")
}

fn ensure_source_record_version(
    transaction: &rusqlite::Transaction<'_>,
    source_locator: &str,
    evidence_id: i64,
) -> Result<u64, VaultError> {
    let existing = transaction
        .query_row(
            "SELECT id FROM source_records WHERE source_kind = 0 AND source_locator = ?1",
            [source_locator],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    let source_record_id = if let Some(id) = existing {
        u64::try_from(id).map_err(|_| VaultError::InvalidKeyOrCorrupt)?
    } else {
        let id = next_identifier(transaction, "source_records")?;
        transaction.execute(
            "INSERT INTO source_records (id, source_kind, source_locator)
             VALUES (?1, 0, ?2)",
            params![to_vault_sql_id(id)?, source_locator],
        )?;
        id
    };
    let version_ordinal: i64 = transaction.query_row(
        "SELECT COALESCE(MAX(version_ordinal), -1) + 1
         FROM source_record_versions WHERE source_record_id = ?1",
        [to_vault_sql_id(source_record_id)?],
        |row| row.get(0),
    )?;
    transaction.execute(
        "INSERT INTO source_record_versions
         (source_record_id, evidence_id, version_ordinal) VALUES (?1, ?2, ?3)",
        params![
            to_vault_sql_id(source_record_id)?,
            evidence_id,
            version_ordinal
        ],
    )?;
    Ok(source_record_id)
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

fn load_canonical_lineage_revision(
    repository: &VaultRepository,
    revision_id: ExtractionRevisionId,
) -> Result<CanonicalLineageRevision, VaultError> {
    let (evidence_id, contract_version) = repository
        .connection()
        .query_row(
            "SELECT evidence_id, contract_version FROM extraction_revisions WHERE id = ?1",
            [to_vault_sql_id(revision_id.get())?],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
        .ok_or(VaultError::InvalidKeyOrCorrupt)?;
    let evidence_id = u64::try_from(evidence_id).map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
    let extraction = repository
        .materialized_extraction(evidence_id, &contract_version)?
        .ok_or(VaultError::InvalidKeyOrCorrupt)?;
    if extraction.revision().id() != revision_id {
        return Err(VaultError::InvalidKeyOrCorrupt);
    }
    let canonical_text = String::from_utf8(repository.read_archived_content(evidence_id)?)
        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
    CanonicalLineageRevision::new(extraction, canonical_text)
        .map_err(|_| VaultError::InvalidKeyOrCorrupt)
}

fn insert_block_lineage(
    transaction: &rusqlite::Transaction<'_>,
    batch_id: u64,
    ordinal: usize,
    lineage: &BlockLineage,
) -> Result<(), VaultError> {
    let from = lineage.from_ref();
    let to = lineage.to_ref();
    let (basis_kind, similarity_basis_points, candidates) = match lineage.basis() {
        LineageBasis::UniqueNativeLocator => (0, None, &[][..]),
        LineageBasis::UniqueExactFingerprint => (1, None, &[][..]),
        LineageBasis::ModifiedSimilarity { score_basis_points } => {
            (2, Some(i64::from(*score_basis_points)), &[][..])
        }
        LineageBasis::NoCandidate => (3, None, &[][..]),
        LineageBasis::AmbiguousCandidates { candidates } => (4, None, candidates.as_slice()),
    };
    transaction.execute(
        "INSERT INTO block_lineages
         (batch_id, ordinal, from_evidence_id, from_block_id,
          to_evidence_id, to_block_id, status, basis_kind, similarity_basis_points)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            to_vault_sql_id(batch_id)?,
            i64::try_from(ordinal).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
            to_vault_sql_id(from.evidence_id())?,
            to_vault_sql_id(from.block_id().get())?,
            to.map(|reference| to_vault_sql_id(reference.evidence_id()))
                .transpose()?,
            to.map(|reference| to_vault_sql_id(reference.block_id().get()))
                .transpose()?,
            encode_lineage_status(lineage.status()),
            basis_kind,
            similarity_basis_points,
        ],
    )?;
    for (candidate_ordinal, candidate) in candidates.iter().enumerate() {
        transaction.execute(
            "INSERT INTO block_lineage_candidates
             (batch_id, lineage_ordinal, candidate_ordinal,
              candidate_evidence_id, candidate_block_id)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                to_vault_sql_id(batch_id)?,
                i64::try_from(ordinal).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                i64::try_from(candidate_ordinal).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                to_vault_sql_id(candidate.evidence_id())?,
                to_vault_sql_id(candidate.block_id().get())?,
            ],
        )?;
    }
    Ok(())
}

fn insert_incremental_work_item(
    transaction: &rusqlite::Transaction<'_>,
    batch_id: u64,
    ordinal: usize,
    item: &IncrementalWorkItem,
) -> Result<(), VaultError> {
    let (action, from, to, review_reason) = match item {
        IncrementalWorkItem::AdvanceCurrentProjection { from_ref, to_ref } => {
            (0, Some(*from_ref), Some(*to_ref), None)
        }
        IncrementalWorkItem::ReuseIndexPayload { from_ref, to_ref } => {
            (1, Some(*from_ref), Some(*to_ref), None)
        }
        IncrementalWorkItem::RebuildIndex { to_ref } => (2, None, Some(*to_ref), None),
        IncrementalWorkItem::ReviewMemory { from_ref, reason } => (
            3,
            Some(*from_ref),
            None,
            Some(encode_lineage_status(*reason)),
        ),
    };
    transaction.execute(
        "INSERT INTO incremental_work_items
         (batch_id, ordinal, action, from_evidence_id, from_block_id,
          to_evidence_id, to_block_id, review_reason)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            to_vault_sql_id(batch_id)?,
            i64::try_from(ordinal).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
            action,
            from.map(|reference| to_vault_sql_id(reference.evidence_id()))
                .transpose()?,
            from.map(|reference| to_vault_sql_id(reference.block_id().get()))
                .transpose()?,
            to.map(|reference| to_vault_sql_id(reference.evidence_id()))
                .transpose()?,
            to.map(|reference| to_vault_sql_id(reference.block_id().get()))
                .transpose()?,
            review_reason,
        ],
    )?;
    Ok(())
}

fn load_lineage_batch(
    connection: &Connection,
    to_revision_id: ExtractionRevisionId,
    rule_version: &str,
) -> Result<Option<LineageBatch>, VaultError> {
    let stored = connection
        .query_row(
            "SELECT id, source_record_id, from_revision_id, decided_at
             FROM block_lineage_batches
             WHERE to_revision_id = ?1 AND rule_version = ?2",
            params![to_vault_sql_id(to_revision_id.get())?, rule_version],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .optional()?;
    let Some((batch_id, source_record_id, from_revision_id, decided_at_millis)) = stored else {
        return Ok(None);
    };
    let lineages = load_block_lineages(connection, batch_id)?;
    let work_plan = IncrementalWorkPlan::new(load_incremental_work_items(connection, batch_id)?);
    LineageBatch::new(
        u64::try_from(source_record_id).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
        ExtractionRevisionId::new(
            u64::try_from(from_revision_id).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
        )
        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
        to_revision_id,
        decided_at_millis,
        rule_version.to_owned(),
        lineages,
        work_plan,
    )
    .map(Some)
    .map_err(|_| VaultError::InvalidKeyOrCorrupt)
}

fn load_block_lineages(
    connection: &Connection,
    batch_id: i64,
) -> Result<Vec<BlockLineage>, VaultError> {
    let mut statement = connection.prepare(
        "SELECT ordinal, from_evidence_id, from_block_id,
                to_evidence_id, to_block_id, status, basis_kind,
                similarity_basis_points
         FROM block_lineages WHERE batch_id = ?1 ORDER BY ordinal",
    )?;
    let rows = statement
        .query_map([batch_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, Option<i64>>(7)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(
            |(
                ordinal,
                from_evidence_id,
                from_block_id,
                to_evidence_id,
                to_block_id,
                status,
                basis_kind,
                similarity_basis_points,
            )| {
                let from_ref = decode_evidence_block_ref(from_evidence_id, from_block_id)?;
                let to_ref = match (to_evidence_id, to_block_id) {
                    (Some(evidence_id), Some(block_id)) => {
                        Some(decode_evidence_block_ref(evidence_id, block_id)?)
                    }
                    (None, None) => None,
                    _ => return Err(VaultError::InvalidKeyOrCorrupt),
                };
                let basis = match basis_kind {
                    0 if similarity_basis_points.is_none() => LineageBasis::UniqueNativeLocator,
                    1 if similarity_basis_points.is_none() => LineageBasis::UniqueExactFingerprint,
                    2 => LineageBasis::ModifiedSimilarity {
                        score_basis_points: u16::try_from(
                            similarity_basis_points.ok_or(VaultError::InvalidKeyOrCorrupt)?,
                        )
                        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                    },
                    3 if similarity_basis_points.is_none() => LineageBasis::NoCandidate,
                    4 if similarity_basis_points.is_none() => LineageBasis::AmbiguousCandidates {
                        candidates: load_lineage_candidates(connection, batch_id, ordinal)?,
                    },
                    _ => return Err(VaultError::InvalidKeyOrCorrupt),
                };
                BlockLineage::new(from_ref, to_ref, decode_lineage_status(status)?, basis)
                    .map_err(|_| VaultError::InvalidKeyOrCorrupt)
            },
        )
        .collect()
}

fn load_lineage_candidates(
    connection: &Connection,
    batch_id: i64,
    lineage_ordinal: i64,
) -> Result<Vec<EvidenceBlockRef>, VaultError> {
    let mut statement = connection.prepare(
        "SELECT candidate_evidence_id, candidate_block_id
         FROM block_lineage_candidates
         WHERE batch_id = ?1 AND lineage_ordinal = ?2
         ORDER BY candidate_ordinal",
    )?;
    statement
        .query_map(params![batch_id, lineage_ordinal], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|(evidence_id, block_id)| decode_evidence_block_ref(evidence_id, block_id))
        .collect()
}

fn load_incremental_work_items(
    connection: &Connection,
    batch_id: i64,
) -> Result<Vec<IncrementalWorkItem>, VaultError> {
    let mut statement = connection.prepare(
        "SELECT action, from_evidence_id, from_block_id,
                to_evidence_id, to_block_id, review_reason
         FROM incremental_work_items WHERE batch_id = ?1 ORDER BY ordinal",
    )?;
    let rows = statement
        .query_map([batch_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, Option<i64>>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(
            |(action, from_evidence_id, from_block_id, to_evidence_id, to_block_id, reason)| {
                let from = decode_optional_evidence_block_ref(from_evidence_id, from_block_id)?;
                let to = decode_optional_evidence_block_ref(to_evidence_id, to_block_id)?;
                match (action, from, to, reason) {
                    (0, Some(from_ref), Some(to_ref), None) => {
                        Ok(IncrementalWorkItem::AdvanceCurrentProjection { from_ref, to_ref })
                    }
                    (1, Some(from_ref), Some(to_ref), None) => {
                        Ok(IncrementalWorkItem::ReuseIndexPayload { from_ref, to_ref })
                    }
                    (2, None, Some(to_ref), None) => {
                        Ok(IncrementalWorkItem::RebuildIndex { to_ref })
                    }
                    (3, Some(from_ref), None, Some(reason)) => {
                        Ok(IncrementalWorkItem::ReviewMemory {
                            from_ref,
                            reason: decode_lineage_status(reason)?,
                        })
                    }
                    _ => Err(VaultError::InvalidKeyOrCorrupt),
                }
            },
        )
        .collect()
}

fn decode_optional_evidence_block_ref(
    evidence_id: Option<i64>,
    block_id: Option<i64>,
) -> Result<Option<EvidenceBlockRef>, VaultError> {
    match (evidence_id, block_id) {
        (Some(evidence_id), Some(block_id)) => {
            decode_evidence_block_ref(evidence_id, block_id).map(Some)
        }
        (None, None) => Ok(None),
        _ => Err(VaultError::InvalidKeyOrCorrupt),
    }
}

fn decode_evidence_block_ref(
    evidence_id: i64,
    block_id: i64,
) -> Result<EvidenceBlockRef, VaultError> {
    EvidenceBlockRef::new(
        u64::try_from(evidence_id).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
        EvidenceBlockId::new(u64::try_from(block_id).map_err(|_| VaultError::InvalidKeyOrCorrupt)?)
            .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
    )
    .map_err(|_| VaultError::InvalidKeyOrCorrupt)
}

const fn encode_lineage_status(status: BlockLineageStatus) -> i64 {
    match status {
        BlockLineageStatus::Unchanged => 0,
        BlockLineageStatus::Moved => 1,
        BlockLineageStatus::Modified => 2,
        BlockLineageStatus::Removed => 3,
        BlockLineageStatus::Ambiguous => 4,
    }
}

const fn decode_lineage_status(value: i64) -> Result<BlockLineageStatus, VaultError> {
    match value {
        0 => Ok(BlockLineageStatus::Unchanged),
        1 => Ok(BlockLineageStatus::Moved),
        2 => Ok(BlockLineageStatus::Modified),
        3 => Ok(BlockLineageStatus::Removed),
        4 => Ok(BlockLineageStatus::Ambiguous),
        _ => Err(VaultError::InvalidKeyOrCorrupt),
    }
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

    #[test]
    fn lineage_failure_rolls_back_edges_candidates_and_work_plan_across_reopen() {
        let _guard = crate::test_support::sqlcipher_test_lock();
        let directory = tempdir().unwrap();
        let key = [0x71; 32];
        let previous_source = "# 谱系\n\nRepeated evidence.\n\nRepeated evidence.\n";
        let current_source =
            "# 谱系\n\nNew separator.\n\nRepeated evidence.\n\nRepeated evidence.\n";
        let mut repository = VaultRepository::open(directory.path(), VaultKey::new(key)).unwrap();

        let previous_receipt = repository
            .archive(ArchiveInput {
                source_locator: "inbox/lineage.md",
                content: previous_source.as_bytes(),
                status: ArchiveStatus::Archived,
                archived_at_millis: 10,
            })
            .unwrap();
        eam_ingestion::process_archived_markdown(
            &mut repository,
            previous_receipt.archive_id,
            eam_markdown::ParseLimits::default(),
            20,
            30,
        )
        .unwrap();
        eam_ingestion::materialize_accepted_markdown(
            &mut repository,
            previous_receipt.archive_id,
            eam_markdown::CONTRACT_VERSION,
        )
        .unwrap();

        let current_receipt = repository
            .archive(ArchiveInput {
                source_locator: "inbox/lineage.md",
                content: current_source.as_bytes(),
                status: ArchiveStatus::Archived,
                archived_at_millis: 40,
            })
            .unwrap();
        eam_ingestion::process_archived_markdown(
            &mut repository,
            current_receipt.archive_id,
            eam_markdown::ParseLimits::default(),
            50,
            60,
        )
        .unwrap();
        let current = eam_ingestion::materialize_accepted_markdown(
            &mut repository,
            current_receipt.archive_id,
            eam_markdown::CONTRACT_VERSION,
        )
        .unwrap();
        let pair = repository
            .load_lineage_pair(current.revision().id())
            .unwrap()
            .unwrap();
        let batch = eam_ingestion::compute_block_lineage(
            pair.source_record_id(),
            pair.previous().extraction(),
            pair.previous().canonical_text(),
            pair.current().extraction(),
            pair.current().canonical_text(),
            70,
        )
        .unwrap();

        let result = repository
            .commit_lineage_batch_with_hook(&batch, |_| Err(VaultError::LineageInterrupted));

        assert!(matches!(result, Err(VaultError::LineageInterrupted)));
        assert!(
            repository
                .load_lineage_batch(current.revision().id(), batch.rule_version())
                .unwrap()
                .is_none()
        );
        repository.close().unwrap();

        let repository = VaultRepository::open(directory.path(), VaultKey::new(key)).unwrap();
        assert!(
            repository
                .load_lineage_batch(current.revision().id(), batch.rule_version())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn corrupt_retrieval_index_rebuilds_without_mutating_authority() {
        let _guard = crate::test_support::sqlcipher_test_lock();
        let directory = tempdir().unwrap();
        let mut repository =
            VaultRepository::open(directory.path(), VaultKey::new([0x91; 32])).unwrap();
        let source = "# Rebuild\n\nAuthoritative retrieval evidence.\n";
        let receipt = repository
            .archive(ArchiveInput {
                source_locator: "inbox/rebuild.md",
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
        eam_ingestion::materialize_accepted_markdown(
            &mut repository,
            receipt.archive_id,
            eam_markdown::CONTRACT_VERSION,
        )
        .unwrap();
        let canonical_before = repository
            .read_archived_content(receipt.archive_id)
            .unwrap();
        let extraction_before = repository
            .materialized_extraction(receipt.archive_id, eam_markdown::CONTRACT_VERSION)
            .unwrap();

        let first = repository.ensure_retrieval_index().unwrap();
        assert_eq!(first.disposition(), IndexDisposition::Rebuilt);
        repository
            .connection()
            .execute(
                "UPDATE retrieval_block_documents SET content_digest = zeroblob(32)",
                [],
            )
            .unwrap();
        let rebuilt = repository.ensure_retrieval_index().unwrap();

        assert_eq!(rebuilt.disposition(), IndexDisposition::Rebuilt);
        assert_eq!(
            repository
                .read_archived_content(receipt.archive_id)
                .unwrap(),
            canonical_before
        );
        assert_eq!(
            repository
                .materialized_extraction(receipt.archive_id, eam_markdown::CONTRACT_VERSION)
                .unwrap(),
            extraction_before
        );
        assert_eq!(
            repository.ensure_retrieval_index().unwrap().disposition(),
            IndexDisposition::Current
        );
        repository
            .connection()
            .execute(
                "UPDATE retrieval_block_vectors SET embedding = zeroblob(512)",
                [],
            )
            .unwrap();
        assert_eq!(
            repository.ensure_retrieval_index().unwrap().disposition(),
            IndexDisposition::Rebuilt
        );
        assert_eq!(
            repository
                .read_archived_content(receipt.archive_id)
                .unwrap(),
            canonical_before
        );
    }
}
