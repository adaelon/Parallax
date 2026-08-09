use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    time::Duration,
};

use eam_capture_browser::{
    BrowserCaptureReceipt, BrowserCaptureRepository, BrowserSubmission, BrowserSubmissionPayload,
    BrowserVisit, BrowserVisitId, PageContentPayload, UntrustedPageContent,
};
use eam_capture_windows::{
    ActivitySnapshot, ActivityTimelineRepository, CaptureCheckpoint, CaptureGapReason, CaptureMode,
    CaptureRecovery, CaptureSpan, CaptureSpanId, CaptureSpanKind, IdleState,
};
use eam_core::{
    AgreementWithdrawal, AgreementWithdrawalActor, ApplicableTime, Claim, ClaimCorrectionReceipt,
    ClaimCorrectionRepository, ClaimId, ClaimOwner, ClaimStatus, ConversationEvidence,
    CounterpartReadiness, CounterpartReplyAttribution, DisputeState, EvidenceCitation, EvidenceId,
    ForgetReceipt, ForgetRepository, ForgetTarget, IdentityEvolutionRepository,
    IdentityProfileSnapshot, IdentityRevisionCommit, IdentityRevisionReceipt,
    IdentityRuntimeContext, IdentityStateSnapshot, MAX_OPEN_REFLECTION_INVITATIONS,
    MemoryRepository, PatternMaturityCommitOutcome, PatternMaturityProposal,
    PatternMaturityReceipt, ReflectionImportance, ReflectionInvitation, ReflectionInvitationBasis,
    ReflectionInvitationId, ReflectionInvitationReceipt, ReflectionInvitationRepository,
    ReflectionInvitationState, RelationalConstraintDeparture, RepositoryError, SelfBundleSnapshot,
    SessionId, SharedAgreementCandidate, SharedAgreementCandidateId,
    SharedAgreementCandidateStatus, SharedAgreementDecision, SharedAgreementResolution,
    SharedExperience, SharedExperienceKind, SharedExperienceRepository, Speaker, Timestamp,
    Uncertainty,
};
use eam_desktop_host::{
    ExitReason, HostGapId, HostGapReason, HostLifecycleRepository, HostRuntimeGap, HostSession,
    HostSessionId, HostSessionStart, LaunchMode,
};
use eam_identity::{
    CounterpartRepository, IdentityProfile, IdentityRepository, IdentityStateVersion,
    InitialSelfIntroduction, IntroductionAnswer, IntroductionItem, SelfBundleRepository,
    SelfBundleState, SelfBundleVersion, SelfIntroductionCategory, WakeCommit, WakeExit,
    WakeTrigger,
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
use eam_memory::{
    LongTermMemoryRepository, MAX_DISPUTE_EVIDENCE, MAX_MEMORY_SOURCES, MemoryBasis,
    MemoryConfidence, MemoryDispute, MemoryDisputeId, MemoryDisputeOutcome,
    MemoryDisputeResolution, MemoryDisputeReviewRecord, MemoryError, MemoryId, MemoryKind,
    MemoryStatus, MemorySubject, MemoryTarget, MemoryVersion, PatternMaturityRecord,
    ValidatedMemoryDispute, ValidatedMemoryDisputeReview, ValidatedMemoryProposal,
    ValidatedPatternMaturityProposal, commit_pattern_maturity as commit_pattern_maturity_domain,
};
use eam_retrieval::{
    AuthoritativeCandidate, AuthoritativeEvidence, CandidateRef, DisputedMemoryRecall,
    EMBEDDING_MODEL_VERSION, IndexBuildReceipt, IndexDisposition, RETRIEVAL_INDEX_VERSION,
    RecallChannels, RecallHit, RetrievalQuery, RetrievalRepository, SourceCurrentness, SourceScope,
    VECTOR_DIMENSIONS, VECTOR_MIN_SCORE_BPS, VectorEmbedding, cosine_similarity_bps, embed_text,
    search_terms,
};
use eam_runtime_gateway::{RuntimeTarget, validate_responses_bearer_token};
use eam_source_obsidian::{
    ObsidianSourceRepository, SourceArchiveInput, SourceArchiveReceipt, SourceAvailability,
    SourceDocumentProjection, SourceFileKind, SourceRecord, SourceRecordState, SourceRelation,
    SourceRelationKind, SourceRoot, SourceRootLifecycle, SourceRootSnapshot,
};
use eam_understanding::{
    ProjectionBuild, ProjectionContent, ProjectionId, ProjectionKind, ProjectionRecipe,
    ProjectionSource, ProjectionStatus, ProjectionTrigger, ProjectionTriggerKind, SourcedStatement,
    StoredProjection, StoredProjectionRecipe, UNDERSTANDING_CONTRACT_VERSION,
    UnderstandingRepository,
};
use fs4::{FileExt, TryLockError};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::{
    VaultError, VaultKey,
    crypto::{sqlcipher_key_pragma, sqlcipher_rekey_pragma},
    object_store::ObjectStore,
    schema::migrate,
};

const DATABASE_FILE: &str = "self.db";
const WRITER_LOCK_FILE: &str = "self.db.writer.lock";
const MAX_VECTOR_CANDIDATES: usize = 64;
const TEMPORAL_NEIGHBOR_RADIUS_MILLIS: i64 = 7 * 24 * 60 * 60 * 1_000;
const MAX_TEMPORAL_NEIGHBORS: usize = 4;
const MAX_RELATION_NEIGHBORS: usize = 8;
const MAX_UNDERSTANDING_CANDIDATES: usize = 128;
const MAX_LONG_TERM_MEMORY_CANDIDATES: usize = 128;
const FORGET_TARGET_CONVERSATION_EVIDENCE: i64 = 0;
const FORGET_TARGET_ARCHIVED_EVIDENCE: i64 = 1;
const RUNTIME_PROFILE_SINGLETON_ID: i64 = 1;

pub const DEFAULT_RUNTIME_BASE_URL: &str = "http://127.0.0.1:11434/v1";
pub const DEFAULT_RUNTIME_MODEL: &str = "gpt-oss-20b";

/// The only allowed mutation of the persisted bearer key.
///
/// This type intentionally omits `Debug` so a replacement key cannot be
/// emitted by routine request logging.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RuntimeProfileKeyAction<'a> {
    Keep,
    Replace(&'a str),
    Clear,
}

/// Complete trusted-host view of the singleton runtime profile.
///
/// The bearer key is zeroized with this value and the type intentionally
/// omits `Clone`, `Debug`, and serialization traits.
pub struct RuntimeProfile {
    base_url: String,
    model: String,
    bearer_key: Option<Zeroizing<String>>,
}

impl RuntimeProfile {
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    #[must_use]
    pub fn bearer_key(&self) -> Option<&str> {
        self.bearer_key.as_ref().map(|key| key.as_str())
    }
}

/// Command-safe runtime profile view that can never contain the complete key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeProfileView {
    base_url: String,
    model: String,
    api_key_configured: bool,
    api_key_last_four: Option<String>,
}

impl RuntimeProfileView {
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    #[must_use]
    pub const fn api_key_configured(&self) -> bool {
        self.api_key_configured
    }

    #[must_use]
    pub fn api_key_last_four(&self) -> Option<&str> {
        self.api_key_last_four.as_deref()
    }
}

pub(crate) struct BackupDeletionRecord {
    pub intent_id: u64,
    pub target_kind: u8,
    pub target_id: u64,
    pub requested_at_millis: i64,
}

pub struct VaultRepository {
    connection: Option<Connection>,
    writer_lock: Option<File>,
    object_store: ObjectStore,
    vault_key: VaultKey,
    database_path: PathBuf,
    next_evidence_id: u64,
    next_claim_id: u64,
    next_shared_agreement_candidate_id: u64,
    next_reflection_invitation_id: u64,
    next_archive_id: u64,
    next_browser_visit_id: u64,
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

        let next_evidence_id = next_identifier_with_deletion_watermark(
            &connection,
            "conversation_evidence",
            FORGET_TARGET_CONVERSATION_EVIDENCE,
        )?;
        let next_claim_id = next_identifier(&connection, "claims")?;
        let next_shared_agreement_candidate_id =
            next_identifier(&connection, "shared_agreement_candidates")?;
        let next_reflection_invitation_id = next_identifier(&connection, "reflection_invitations")?;
        let next_archive_id = next_identifier_with_deletion_watermark(
            &connection,
            "archived_evidence",
            FORGET_TARGET_ARCHIVED_EVIDENCE,
        )?;
        let next_browser_visit_id = next_identifier(&connection, "browser_visits")?;

        Ok(Self {
            connection: Some(connection),
            writer_lock: Some(writer_lock),
            object_store,
            vault_key,
            database_path,
            next_evidence_id,
            next_claim_id,
            next_shared_agreement_candidate_id,
            next_reflection_invitation_id,
            next_archive_id,
            next_browser_visit_id,
        })
    }

    #[must_use]
    pub fn database_path(&self) -> &Path {
        &self.database_path
    }

    pub(crate) fn vault_root(&self) -> Result<&Path, VaultError> {
        self.database_path
            .parent()
            .ok_or(VaultError::InvalidKeyOrCorrupt)
    }

    pub(crate) fn backup_key(&self) -> Result<eam_backup::BackupKey, VaultError> {
        self.vault_key.backup_key()
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

    /// Reads the complete singleton runtime profile for trusted host wiring.
    ///
    /// # Errors
    ///
    /// Fails closed if the singleton is missing, malformed, or no longer
    /// satisfies the supported runtime target and bearer boundaries.
    pub fn runtime_profile(&self) -> Result<RuntimeProfile, VaultError> {
        load_runtime_profile(self.connection())
    }

    /// Reads a command-safe view containing only key presence and, when that
    /// cannot reveal the complete key, its last four Unicode scalar values.
    ///
    /// # Errors
    ///
    /// Returns the same fail-closed storage errors as [`Self::runtime_profile`].
    pub fn runtime_profile_view(&self) -> Result<RuntimeProfileView, VaultError> {
        let profile = self.runtime_profile()?;
        Ok(runtime_profile_view(&profile))
    }

    /// Atomically replaces the target fields and applies one explicit bearer
    /// key action.
    ///
    /// Base URL and model normalization reuse [`RuntimeTarget::new`]; bearer
    /// replacement reuses the exact runtime transport field validator.
    ///
    /// # Errors
    ///
    /// Rejects invalid candidates before opening a transaction and fails
    /// closed if the singleton row cannot be updated exactly once.
    pub fn update_runtime_profile(
        &mut self,
        base_url: &str,
        model: &str,
        key_action: RuntimeProfileKeyAction<'_>,
    ) -> Result<RuntimeProfileView, VaultError> {
        self.update_runtime_profile_with_hook(base_url, model, key_action, |_| Ok(()))
    }

    fn update_runtime_profile_with_hook<F>(
        &mut self,
        base_url: &str,
        model: &str,
        key_action: RuntimeProfileKeyAction<'_>,
        before_commit: F,
    ) -> Result<RuntimeProfileView, VaultError>
    where
        F: FnOnce(&rusqlite::Transaction<'_>) -> Result<(), VaultError>,
    {
        let target =
            RuntimeTarget::new(base_url, model).map_err(|_| VaultError::InvalidRuntimeProfile)?;
        if let RuntimeProfileKeyAction::Replace(key) = key_action {
            validate_responses_bearer_token(Some(key))
                .map_err(|_| VaultError::InvalidRuntimeProfile)?;
        }

        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()?;
        let changed = match key_action {
            RuntimeProfileKeyAction::Keep => transaction.execute(
                "UPDATE runtime_profiles SET base_url = ?1, model = ?2
                 WHERE singleton_id = ?3",
                params![
                    target.base_url(),
                    target.model(),
                    RUNTIME_PROFILE_SINGLETON_ID
                ],
            )?,
            RuntimeProfileKeyAction::Replace(key) => transaction.execute(
                "UPDATE runtime_profiles SET base_url = ?1, model = ?2, bearer_key = ?3
                 WHERE singleton_id = ?4",
                params![
                    target.base_url(),
                    target.model(),
                    key,
                    RUNTIME_PROFILE_SINGLETON_ID
                ],
            )?,
            RuntimeProfileKeyAction::Clear => transaction.execute(
                "UPDATE runtime_profiles SET base_url = ?1, model = ?2, bearer_key = NULL
                 WHERE singleton_id = ?3",
                params![
                    target.base_url(),
                    target.model(),
                    RUNTIME_PROFILE_SINGLETON_ID
                ],
            )?,
        };
        if changed != 1 {
            return Err(VaultError::InvalidKeyOrCorrupt);
        }
        before_commit(&transaction)?;
        transaction.commit()?;
        self.runtime_profile_view()
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

    /// Lists committed deletion intents in replay order for S30 recovery.
    ///
    /// # Errors
    ///
    /// Returns an error when encrypted intent state is malformed or unreadable.
    pub fn deletion_intents(&self) -> Result<Vec<ForgetReceipt>, VaultError> {
        load_deletion_intents(self.connection())
    }

    pub(crate) fn backup_deletion_records(&self) -> Result<Vec<BackupDeletionRecord>, VaultError> {
        let mut statement = self.connection().prepare(
            "SELECT id, target_kind, target_id, requested_at
             FROM deletion_intents ORDER BY id",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(intent_id, target_kind, target_id, requested_at_millis)| {
                Ok(BackupDeletionRecord {
                    intent_id: u64::try_from(intent_id)
                        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                    target_kind: u8::try_from(target_kind)
                        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                    target_id: u64::try_from(target_id)
                        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                    requested_at_millis,
                })
            })
            .collect()
    }

    pub(crate) fn create_backup_snapshot(
        &mut self,
        database_snapshot: &Path,
    ) -> Result<Vec<(String, Vec<u8>)>, VaultError> {
        if database_snapshot.exists() {
            return Err(VaultError::InvalidBackup);
        }
        let mut destination = Connection::open_with_flags(
            database_snapshot,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
        )?;
        key_connection(&destination, &self.vault_key)?;
        verify_sqlcipher(&destination)?;
        configure_connection(&destination)?;
        {
            let backup = rusqlite::backup::Backup::new(self.connection(), &mut destination)?;
            backup.run_to_completion(128, Duration::ZERO, None)?;
        }
        destination.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        destination
            .close()
            .map_err(|(_, error)| VaultError::Sqlite(error))?;

        let mut files = vec![("self.db".to_owned(), fs::read(database_snapshot)?)];
        for object_id in referenced_object_ids(self.connection())? {
            self.object_store
                .read(&object_id)
                .map_err(|_| VaultError::InvalidBackup)?;
            let bytes = fs::read(self.object_store.root().join(&object_id))
                .map_err(|_| VaultError::InvalidBackup)?;
            files.push((format!("objects/{object_id}"), bytes));
        }
        files.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(files)
    }

    pub(crate) fn replay_backup_deletion(
        &mut self,
        intent_id: u64,
        target: ForgetTarget,
        requested_at: Timestamp,
    ) -> Result<(), VaultError> {
        if let Some(existing) = load_deletion_intent(self.connection(), target)? {
            if existing.deletion_intent_id() > intent_id {
                return Err(VaultError::InvalidBackup);
            }
            return Ok(());
        }
        if forget_target_exists(self.connection(), target)? {
            let receipt = self
                .forget_with_hook(target, requested_at, |_| Ok(()))?
                .ok_or(VaultError::InvalidBackup)?;
            if receipt.deletion_intent_id() != intent_id {
                return Err(VaultError::InvalidBackup);
            }
            return Ok(());
        }

        let expected = next_identifier(self.connection(), "deletion_intents")?;
        if expected != intent_id {
            return Err(VaultError::InvalidBackup);
        }
        let receipt = ForgetReceipt::new(intent_id, target, 0, 0, 0);
        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()?;
        insert_deletion_intent(&transaction, receipt, requested_at)?;
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn rebuild_retrieval_after_restore(&mut self) -> Result<(), VaultError> {
        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()?;
        clear_retrieval_index(&transaction)?;
        transaction.commit()?;
        <Self as RetrievalRepository>::ensure_retrieval_index(self)?;
        Ok(())
    }

    pub(crate) fn rotate_after_restore(&mut self, new_key: &VaultKey) -> Result<(), VaultError> {
        let objects_root = self.object_store.root().to_path_buf();
        let parent = objects_root.parent().ok_or(VaultError::InvalidBackup)?;
        let rotating_root = parent.join("objects.rotating");
        let previous_root = parent.join("objects.previous");
        if rotating_root.exists() || previous_root.exists() {
            return Err(VaultError::InvalidBackup);
        }
        let new_store = ObjectStore::open_directory(rotating_root.clone(), new_key.objects_key()?)?;
        let mut replacements = Vec::new();
        for old_id in referenced_object_ids(self.connection())? {
            let plaintext = self
                .object_store
                .read(&old_id)
                .map_err(|_| VaultError::InvalidBackup)?;
            let stored = new_store.store(&plaintext)?;
            replacements.push((old_id, stored.id));
        }

        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()?;
        for (old_id, new_id) in &replacements {
            transaction.execute(
                "UPDATE archived_evidence SET object_id = ?1 WHERE object_id = ?2",
                params![new_id, old_id],
            )?;
        }
        transaction.commit()?;

        self.connection().execute_batch(
            "PRAGMA wal_checkpoint(TRUNCATE);
             PRAGMA journal_mode = DELETE;",
        )?;
        let new_database_key = new_key.database_key()?;
        let rekey = sqlcipher_rekey_pragma(&new_database_key);
        self.connection().execute_batch(&rekey)?;
        verify_key_and_pages(self.connection())?;
        self.connection()
            .execute_batch("PRAGMA journal_mode = WAL;")?;

        fs::rename(&objects_root, &previous_root)?;
        if let Err(error) = fs::rename(&rotating_root, &objects_root) {
            let _ = fs::rename(&previous_root, &objects_root);
            return Err(VaultError::Io(error));
        }
        fs::remove_dir_all(previous_root)?;
        Ok(())
    }

    fn forget_with_hook<F>(
        &mut self,
        target: ForgetTarget,
        requested_at: Timestamp,
        before_commit: F,
    ) -> Result<Option<ForgetReceipt>, VaultError>
    where
        F: FnOnce(&rusqlite::Transaction<'_>) -> Result<(), VaultError>,
    {
        if let Some(receipt) = load_deletion_intent(self.connection(), target)? {
            if matches!(target, ForgetTarget::ArchivedEvidence(_)) {
                self.object_store
                    .cleanup_unreferenced(&referenced_object_ids(self.connection())?)?;
            }
            return Ok(Some(receipt));
        }
        if !forget_target_exists(self.connection(), target)? {
            return Ok(None);
        }

        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()?;
        if let Some(receipt) = load_deletion_intent(&transaction, target)? {
            transaction.commit()?;
            return Ok(Some(receipt));
        }
        let intent_id = next_identifier(&transaction, "deletion_intents")?;
        let counts = match target {
            ForgetTarget::ConversationEvidence(evidence_id) => {
                delete_conversation_evidence_closure(&transaction, evidence_id)?
            }
            ForgetTarget::ArchivedEvidence(archive_id) => {
                delete_archived_evidence_closure(&transaction, archive_id)?
            }
        };
        let receipt = ForgetReceipt::new(
            intent_id,
            target,
            counts.authority,
            counts.derived,
            counts.object_references,
        );
        insert_deletion_intent(&transaction, receipt, requested_at)?;
        before_commit(&transaction)?;
        transaction.commit()?;

        if matches!(target, ForgetTarget::ArchivedEvidence(_)) {
            self.object_store
                .cleanup_unreferenced(&referenced_object_ids(self.connection())?)?;
        }
        Ok(Some(receipt))
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

    /// Deletes only the disposable artifact for one understanding projection.
    /// The durable recipe and authoritative references remain rebuildable.
    ///
    /// # Errors
    ///
    /// Returns the storage error or `InvalidKeyOrCorrupt` for an unknown id.
    pub fn delete_understanding_artifact(&mut self, id: ProjectionId) -> Result<(), VaultError> {
        let changed = self.connection().execute(
            "DELETE FROM understanding_projection_artifacts WHERE projection_id = ?1",
            [to_vault_sql_id(id.get())?],
        )?;
        if changed == 0 {
            return Err(VaultError::InvalidKeyOrCorrupt);
        }
        Ok(())
    }

    /// Reports whether the disposable projection artifact is currently present.
    ///
    /// # Errors
    ///
    /// Returns the encrypted database query error.
    pub fn understanding_artifact_present(&self, id: ProjectionId) -> Result<bool, VaultError> {
        self.connection()
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM understanding_projection_artifacts WHERE projection_id = ?1
                 )",
                [to_vault_sql_id(id.get())?],
                |row| row.get(0),
            )
            .map_err(VaultError::from)
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

    fn append_pattern_maturity_with_hook<F>(
        &mut self,
        proposal: &ValidatedPatternMaturityProposal,
        proposed_at: Timestamp,
        before_commit: F,
    ) -> Result<MemoryVersion, RepositoryError>
    where
        F: FnOnce(&rusqlite::Transaction<'_>) -> Result<(), RepositoryError>,
    {
        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()
            .map_err(repository_error)?;
        let current = load_current_memory(&transaction, proposal.memory_id())?
            .ok_or_else(|| RepositoryError::new("memory does not exist"))?;
        if current.version() != proposal.expected_version() {
            return Err(RepositoryError::new("stale memory version"));
        }
        if current.status() != MemoryStatus::ProvisionalPattern
            || current.basis() != MemoryBasis::PatternCandidate
        {
            return Err(RepositoryError::new(
                "only a provisional pattern can mature",
            ));
        }
        validate_persisted_pattern_maturity(&transaction, &current, proposal)?;
        let next_version = current
            .version()
            .checked_add(1)
            .ok_or_else(|| RepositoryError::new("memory version space exhausted"))?;
        insert_memory_state_event(
            &transaction,
            current.id(),
            current.version(),
            MemoryStatus::Superseded,
            proposed_at,
        )?;
        insert_matured_pattern_version(
            &transaction,
            &current,
            next_version,
            proposal,
            proposed_at,
        )?;
        insert_pattern_maturity_record(&transaction, proposal, next_version, proposed_at)?;
        before_commit(&transaction)?;
        transaction.commit().map_err(repository_error)?;
        load_memory_version(self.connection(), current.id(), next_version)
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

    fn record_browser_submission_with_hook<F>(
        &mut self,
        host_session_id: HostSessionId,
        submission: &BrowserSubmission,
        before_commit: F,
    ) -> Result<BrowserCaptureReceipt, RepositoryError>
    where
        F: FnOnce(&rusqlite::Transaction<'_>) -> Result<(), RepositoryError>,
    {
        require_current_open_host_session(self.connection(), host_session_id)?;
        if let Some(existing) = load_browser_visit_by_submission(self, submission.submission_id())?
        {
            if existing.submission() != submission {
                return Err(RepositoryError::new(
                    "browser submission identifier conflicts with an existing event",
                ));
            }
            return Ok(BrowserCaptureReceipt::new(
                existing.id(),
                existing.content_archive_id(),
                true,
            ));
        }

        let stored_content = submission
            .page_content()
            .map(|content| self.object_store.store(content.body_text().as_bytes()))
            .transpose()
            .map_err(repository_error)?;
        let visit_id = BrowserVisitId::from_raw(self.next_browser_visit_id);
        let content_archive_id = stored_content.as_ref().map(|_| self.next_archive_id);
        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()
            .map_err(repository_error)?;
        require_current_open_host_session(&transaction, host_session_id)?;

        if let (Some(content), Some(stored), Some(archive_id)) = (
            submission.page_content(),
            stored_content.as_ref(),
            content_archive_id,
        ) {
            let source_locator = format!("browser/{}.txt", submission.submission_id());
            let (status, reason) = encode_archive_status(ArchiveStatus::ArchivedUnparsed(
                UnparsedReason::UnsupportedFormat,
            ));
            transaction
                .execute(
                    "INSERT INTO archived_evidence
                     (id, source_kind, source_locator, object_id, content_length,
                      status, unparsed_reason, archived_at)
                     VALUES (?1, 0, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        to_sql_id(archive_id)?,
                        source_locator,
                        stored.id,
                        i64::try_from(content.body_text().len()).map_err(repository_error)?,
                        status,
                        reason,
                        content.captured_at().as_millis(),
                    ],
                )
                .map_err(repository_error)?;
            ensure_source_record_version(&transaction, &source_locator, to_sql_id(archive_id)?)
                .map_err(repository_error)?;
        }

        transaction
            .execute(
                "INSERT INTO browser_visits
                 (id, host_session_id, submission_id, url, title, visited_at, dwell_millis,
                  content_evidence_id, content_captured_at, content_authorized_origin)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    to_sql_id(visit_id.get())?,
                    to_sql_id(host_session_id.get())?,
                    submission.submission_id(),
                    submission.url(),
                    submission.title(),
                    submission.visited_at().as_millis(),
                    submission.dwell_millis(),
                    content_archive_id.map(to_sql_id).transpose()?,
                    submission
                        .page_content()
                        .map(|content| content.captured_at().as_millis()),
                    submission
                        .page_content()
                        .map(UntrustedPageContent::authorized_origin),
                ],
            )
            .map_err(repository_error)?;
        before_commit(&transaction)?;
        transaction.commit().map_err(repository_error)?;

        self.next_browser_visit_id = visit_id
            .get()
            .checked_add(1)
            .ok_or_else(|| RepositoryError::new("browser visit identifier space exhausted"))?;
        if let Some(archive_id) = content_archive_id {
            self.next_archive_id = archive_id.checked_add(1).ok_or_else(|| {
                RepositoryError::new("browser content archive identifier space exhausted")
            })?;
        }
        Ok(BrowserCaptureReceipt::new(
            visit_id,
            content_archive_id,
            false,
        ))
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
        reconcile_understanding_projections(&transaction, &self.object_store, batch)?;
        before_commit(&transaction)?;
        transaction.commit()?;
        Ok(batch.clone())
    }

    fn activate_source_root_with_hook<F>(
        &mut self,
        root_id: u64,
        observed_at_millis: i64,
        before_commit: F,
    ) -> Result<SourceRootSnapshot, VaultError>
    where
        F: FnOnce(&rusqlite::Transaction<'_>) -> Result<(), VaultError>,
    {
        let root_id_sql = to_vault_sql_id(root_id)?;
        let candidate = self
            .connection()
            .query_row(
                "SELECT lifecycle_state, availability, last_reconciled_at
                 FROM source_roots WHERE id = ?1",
                [root_id_sql],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                    ))
                },
            )
            .optional()?
            .ok_or(VaultError::InvalidKeyOrCorrupt)?;
        let lifecycle = decode_source_root_lifecycle(candidate.0)?;
        let availability = decode_source_availability(candidate.1)?;
        if lifecycle == SourceRootLifecycle::Active {
            return load_source_root_snapshot(self.connection(), root_id);
        }
        if availability != SourceAvailability::Available || candidate.2.is_none() {
            return Err(VaultError::InvalidKeyOrCorrupt);
        }

        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()?;
        let previous_active = transaction
            .query_row(
                "SELECT id FROM source_roots WHERE lifecycle_state = 1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        if let Some(previous_active) = previous_active {
            let previous_active =
                u64::try_from(previous_active).map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
            let changed = transaction.execute(
                "UPDATE source_roots SET lifecycle_state = 2
                 WHERE id = ?1 AND lifecycle_state = 1",
                [to_vault_sql_id(previous_active)?],
            )?;
            if changed != 1 {
                return Err(VaultError::InvalidKeyOrCorrupt);
            }
            insert_source_root_lifecycle_event(
                &transaction,
                previous_active,
                SourceRootLifecycle::Detached,
                observed_at_millis,
            )?;
        }
        let changed = transaction.execute(
            "UPDATE source_roots SET lifecycle_state = 1
             WHERE id = ?1 AND lifecycle_state IN (0, 2)",
            [root_id_sql],
        )?;
        if changed != 1 {
            return Err(VaultError::InvalidKeyOrCorrupt);
        }
        insert_source_root_lifecycle_event(
            &transaction,
            root_id,
            SourceRootLifecycle::Active,
            observed_at_millis,
        )?;
        before_commit(&transaction)?;
        transaction.commit()?;
        load_source_root_snapshot(self.connection(), root_id)
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

impl BrowserCaptureRepository for VaultRepository {
    fn record_browser_submission(
        &mut self,
        host_session_id: HostSessionId,
        submission: &BrowserSubmission,
    ) -> Result<BrowserCaptureReceipt, RepositoryError> {
        self.record_browser_submission_with_hook(host_session_id, submission, |_| Ok(()))
    }

    fn all_browser_visits(&self) -> Result<Vec<BrowserVisit>, RepositoryError> {
        load_all_browser_visits(self)
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
             (id, root_kind, root_locator, availability, first_seen_at,
              last_reconciled_at, lifecycle_state)
             VALUES (?1, 0, ?2, 0, ?3, NULL, 0)",
            params![to_vault_sql_id(root_id)?, root_locator, observed_at_millis],
        )?;
        insert_source_root_event(
            &transaction,
            root_id,
            SourceAvailability::Available,
            observed_at_millis,
        )?;
        insert_source_root_lifecycle_event(
            &transaction,
            root_id,
            SourceRootLifecycle::Staged,
            observed_at_millis,
        )?;
        transaction.commit()?;
        load_source_root_snapshot(self.connection(), root_id)
            .map(|snapshot| snapshot.root().clone())
    }

    fn load_source_root(&self, root_id: u64) -> Result<SourceRootSnapshot, Self::Error> {
        load_source_root_snapshot(self.connection(), root_id)
    }

    fn load_active_source_root(&self) -> Result<Option<SourceRootSnapshot>, Self::Error> {
        load_active_source_root_snapshot(self.connection())
    }

    fn activate_source_root(
        &mut self,
        root_id: u64,
        observed_at_millis: i64,
    ) -> Result<SourceRootSnapshot, Self::Error> {
        self.activate_source_root_with_hook(root_id, observed_at_millis, |_| Ok(()))
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

    fn recall_long_term_memory_candidates(
        &self,
        query: &RetrievalQuery,
    ) -> Result<Vec<RecallHit>, Self::Error> {
        recall_long_term_memory_candidates(self.connection(), query)
    }

    fn recall_disputed_memories(
        &self,
        query: &RetrievalQuery,
    ) -> Result<Vec<DisputedMemoryRecall>, Self::Error> {
        recall_disputed_memories(self.connection(), query)
    }

    fn recall_understanding_candidates(
        &self,
        query: &RetrievalQuery,
    ) -> Result<Vec<RecallHit>, Self::Error> {
        recall_understanding_candidates(self.connection(), query)
    }

    fn resolve_authoritative(
        &self,
        reference: CandidateRef,
        scope: SourceScope,
    ) -> Result<Option<AuthoritativeCandidate>, Self::Error> {
        resolve_retrieval_candidate(self, reference, scope)
    }
}

impl UnderstandingRepository for VaultRepository {
    type Error = VaultError;

    fn resolve_projection_source(
        &self,
        reference: EvidenceBlockRef,
    ) -> Result<Option<ProjectionSource>, Self::Error> {
        resolve_understanding_source(self.connection(), &self.object_store, reference)
    }

    fn commit_projection(
        &mut self,
        build: &ProjectionBuild,
    ) -> Result<StoredProjection, Self::Error> {
        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()?;
        let projection_id = next_identifier(&transaction, "understanding_projections")?;
        insert_understanding_projection(&transaction, projection_id, build)?;
        transaction.commit()?;
        stored_projection(
            projection_id,
            1,
            ProjectionStatus::Active,
            build.material_digest(),
        )
    }

    fn load_projection_recipe(
        &self,
        id: ProjectionId,
    ) -> Result<Option<StoredProjectionRecipe>, Self::Error> {
        load_understanding_projection(self.connection(), id)
    }

    fn replace_projection_artifact(
        &mut self,
        id: ProjectionId,
        build: &ProjectionBuild,
    ) -> Result<StoredProjection, Self::Error> {
        let stored = load_understanding_projection(self.connection(), id)?
            .ok_or(VaultError::InvalidKeyOrCorrupt)?;
        if stored.projection().status() != ProjectionStatus::Active
            || stored.recipe() != build.recipe()
        {
            return Err(VaultError::InvalidKeyOrCorrupt);
        }
        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()?;
        transaction.execute(
            "UPDATE understanding_projections SET material_digest = ?1 WHERE id = ?2",
            params![
                build.material_digest().as_slice(),
                to_vault_sql_id(id.get())?
            ],
        )?;
        replace_understanding_artifact(&transaction, id.get(), build)?;
        transaction.commit()?;
        stored_projection(
            id.get(),
            stored.projection().generation(),
            ProjectionStatus::Active,
            build.material_digest(),
        )
    }
}

impl LongTermMemoryRepository for VaultRepository {
    fn claim(&self, id: ClaimId) -> Result<Option<Claim>, RepositoryError> {
        load_claim(self.connection(), id)
    }

    fn append_memory(
        &mut self,
        proposal: ValidatedMemoryProposal,
        formed_at: Timestamp,
    ) -> Result<MemoryVersion, RepositoryError> {
        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()
            .map_err(repository_error)?;
        validate_persisted_memory_sources(&transaction, &proposal)?;
        let (memory_id, version, predecessor_version) = match proposal.target() {
            MemoryTarget::New => {
                let id = next_identifier(&transaction, "long_term_memories")
                    .map_err(repository_error)?;
                transaction
                    .execute(
                        "INSERT INTO long_term_memories (id, created_at) VALUES (?1, ?2)",
                        params![to_sql_id(id)?, formed_at.as_millis()],
                    )
                    .map_err(repository_error)?;
                (id, 1, None)
            }
            MemoryTarget::Revise {
                memory_id,
                expected_version,
            } => {
                let current = load_current_memory(&transaction, memory_id)?
                    .ok_or_else(|| RepositoryError::new("memory does not exist"))?;
                if current.version() != expected_version {
                    return Err(RepositoryError::new("stale memory version"));
                }
                if current.subject() != proposal.subject() {
                    return Err(RepositoryError::new(
                        "memory revision cannot change ledger attribution",
                    ));
                }
                insert_memory_state_event(
                    &transaction,
                    memory_id,
                    expected_version,
                    MemoryStatus::Superseded,
                    formed_at,
                )?;
                let version = expected_version
                    .checked_add(1)
                    .ok_or_else(|| RepositoryError::new("memory version space exhausted"))?;
                (memory_id.get(), version, Some(expected_version))
            }
        };
        let memory_id =
            MemoryId::new(memory_id).ok_or_else(|| RepositoryError::new("invalid memory id"))?;
        insert_memory_version(
            &transaction,
            memory_id,
            version,
            predecessor_version,
            &proposal,
            formed_at,
        )?;
        transaction.commit().map_err(repository_error)?;
        Ok(MemoryVersion::restore(
            memory_id,
            version,
            predecessor_version,
            proposal.statement().to_owned(),
            proposal.subject(),
            proposal.kind(),
            proposal.source_claim_ids().to_vec(),
            proposal.applicable_time(),
            proposal.confidence(),
            proposal.salience_reason().to_owned(),
            proposal.basis(),
            proposal.initial_status(),
            formed_at,
            proposal.pattern_counterexample_review().cloned(),
        ))
    }

    fn append_pattern_maturity(
        &mut self,
        proposal: ValidatedPatternMaturityProposal,
        proposed_at: Timestamp,
    ) -> Result<MemoryVersion, RepositoryError> {
        self.append_pattern_maturity_with_hook(&proposal, proposed_at, |_| Ok(()))
    }

    fn current_memory(&self, id: MemoryId) -> Result<Option<MemoryVersion>, RepositoryError> {
        load_current_memory(self.connection(), id)
    }

    fn memory_versions(&self, id: MemoryId) -> Result<Vec<MemoryVersion>, RepositoryError> {
        load_memory_versions(self.connection(), Some(id))
    }

    fn pattern_maturity_records(
        &self,
        id: MemoryId,
    ) -> Result<Vec<PatternMaturityRecord>, RepositoryError> {
        load_pattern_maturity_records(self.connection(), id)
    }

    fn all_memory_versions(&self) -> Result<Vec<MemoryVersion>, RepositoryError> {
        load_memory_versions(self.connection(), None)
    }

    fn evidence(&self, id: EvidenceId) -> Result<Option<ConversationEvidence>, RepositoryError> {
        MemoryRepository::evidence(self, id)
    }

    fn append_memory_dispute(
        &mut self,
        dispute: ValidatedMemoryDispute,
        raised_at: Timestamp,
    ) -> Result<MemoryDispute, RepositoryError> {
        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()
            .map_err(repository_error)?;
        validate_persisted_evidence_citations(&transaction, dispute.counter_evidence())?;
        let current = load_current_memory(&transaction, dispute.memory_id())?
            .ok_or_else(|| RepositoryError::new("memory does not exist"))?;
        if current.version() != dispute.memory_version() {
            return Err(RepositoryError::new("stale memory version"));
        }
        if !matches!(
            current.status(),
            MemoryStatus::Active
                | MemoryStatus::Provisional
                | MemoryStatus::ProvisionalPattern
                | MemoryStatus::SupportedCounterpartView
                | MemoryStatus::Weakened
        ) {
            return Err(RepositoryError::new("memory is not disputable"));
        }
        let dispute_id =
            next_identifier(&transaction, "memory_disputes").map_err(repository_error)?;
        transaction
            .execute(
                "INSERT INTO memory_disputes
                 (id, memory_id, memory_version, reason, raised_at, outcome)
                 VALUES (?1, ?2, ?3, ?4, ?5, 0)",
                params![
                    to_sql_id(dispute_id)?,
                    to_sql_id(dispute.memory_id().get())?,
                    to_sql_id(dispute.memory_version())?,
                    dispute.reason(),
                    raised_at.as_millis(),
                ],
            )
            .map_err(repository_error)?;
        insert_dispute_evidence(
            &transaction,
            "memory_dispute_counter_evidence",
            dispute_id,
            dispute.counter_evidence(),
        )?;
        insert_dispute_terms(
            &transaction,
            dispute_id,
            std::iter::once(dispute.reason()).chain(
                dispute
                    .counter_evidence()
                    .iter()
                    .map(EvidenceCitation::quote),
            ),
        )?;
        insert_memory_state_event(
            &transaction,
            dispute.memory_id(),
            dispute.memory_version(),
            MemoryStatus::Disputed,
            raised_at,
        )?;
        transaction.commit().map_err(repository_error)?;
        let dispute_id = MemoryDisputeId::new(dispute_id)
            .ok_or_else(|| RepositoryError::new("invalid memory dispute id"))?;
        load_memory_dispute(self.connection(), dispute_id)?
            .ok_or_else(|| RepositoryError::new("committed memory dispute could not be reloaded"))
    }

    fn memory_dispute(
        &self,
        id: MemoryDisputeId,
    ) -> Result<Option<MemoryDispute>, RepositoryError> {
        load_memory_dispute(self.connection(), id)
    }

    fn memory_disputes(&self, id: MemoryId) -> Result<Vec<MemoryDispute>, RepositoryError> {
        load_memory_disputes(self.connection(), id)
    }

    fn complete_memory_dispute(
        &mut self,
        review: ValidatedMemoryDisputeReview,
        reviewed_at: Timestamp,
    ) -> Result<MemoryDisputeResolution, RepositoryError> {
        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()
            .map_err(repository_error)?;
        validate_persisted_evidence_citations(&transaction, review.evidence())?;
        let dispute = load_memory_dispute(&transaction, review.dispute_id())?
            .ok_or_else(|| RepositoryError::new("memory dispute does not exist"))?;
        if dispute.outcome() != MemoryDisputeOutcome::Open {
            return Err(RepositoryError::new("memory dispute is already resolved"));
        }
        let current = load_current_memory(&transaction, dispute.memory_id())?
            .ok_or_else(|| RepositoryError::new("memory does not exist"))?;
        if current.version() != dispute.memory_version()
            || current.status() != MemoryStatus::Disputed
        {
            return Err(RepositoryError::new("memory dispute state is stale"));
        }

        let revised_version = match review.outcome() {
            MemoryDisputeOutcome::Maintained => None,
            MemoryDisputeOutcome::Retracted => {
                insert_memory_state_event(
                    &transaction,
                    current.id(),
                    current.version(),
                    MemoryStatus::Retracted,
                    reviewed_at,
                )?;
                None
            }
            MemoryDisputeOutcome::Weakened => {
                insert_memory_state_event(
                    &transaction,
                    current.id(),
                    current.version(),
                    MemoryStatus::Weakened,
                    reviewed_at,
                )?;
                None
            }
            MemoryDisputeOutcome::Revised => {
                let proposal = review
                    .revision()
                    .ok_or_else(|| RepositoryError::new("revised dispute has no proposal"))?;
                validate_persisted_memory_sources(&transaction, proposal)?;
                let MemoryTarget::Revise {
                    memory_id,
                    expected_version,
                } = proposal.target()
                else {
                    return Err(RepositoryError::new("dispute revision target is invalid"));
                };
                if memory_id != current.id() || expected_version != current.version() {
                    return Err(RepositoryError::new("dispute revision target is stale"));
                }
                insert_memory_state_event(
                    &transaction,
                    current.id(),
                    current.version(),
                    MemoryStatus::Superseded,
                    reviewed_at,
                )?;
                let next_version = current
                    .version()
                    .checked_add(1)
                    .ok_or_else(|| RepositoryError::new("memory version space exhausted"))?;
                insert_memory_version(
                    &transaction,
                    current.id(),
                    next_version,
                    Some(current.version()),
                    proposal,
                    reviewed_at,
                )?;
                Some(next_version)
            }
            MemoryDisputeOutcome::Open => {
                return Err(RepositoryError::new("review outcome cannot remain open"));
            }
        };
        persist_dispute_review(&transaction, &review, reviewed_at, revised_version)?;
        transaction.commit().map_err(repository_error)?;

        let stored_dispute = load_memory_dispute(self.connection(), review.dispute_id())?
            .ok_or_else(|| RepositoryError::new("resolved dispute could not be reloaded"))?;
        let memory = load_current_memory(self.connection(), stored_dispute.memory_id())?
            .ok_or_else(|| RepositoryError::new("resolved memory could not be reloaded"))?;
        Ok(MemoryDisputeResolution::new(stored_dispute, memory))
    }

    fn retracted_memory_sources(
        &self,
        statement: &str,
    ) -> Result<Vec<(MemoryId, Vec<ClaimId>)>, RepositoryError> {
        let mut query = self
            .connection()
            .prepare(
                "SELECT v.memory_id
                 FROM long_term_memory_versions v
                 WHERE v.version = (
                         SELECT MAX(latest.version) FROM long_term_memory_versions latest
                         WHERE latest.memory_id = v.memory_id
                       )
                   AND trim(v.statement) = trim(?1)
                   AND (SELECT e.status FROM long_term_memory_state_events e
                        WHERE e.memory_id = v.memory_id AND e.version = v.version
                        ORDER BY e.ordinal DESC LIMIT 1) = 5
                 ORDER BY v.memory_id",
            )
            .map_err(repository_error)?;
        let ids = query
            .query_map([statement], |row| row.get::<_, i64>(0))
            .map_err(repository_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(repository_error)?;
        ids.into_iter()
            .map(|id| {
                let id = u64::try_from(id).map_err(repository_error)?;
                let id = MemoryId::new(id)
                    .ok_or_else(|| RepositoryError::new("invalid persisted memory id"))?;
                let current = load_current_memory(self.connection(), id)?
                    .ok_or_else(|| RepositoryError::new("retracted memory does not exist"))?;
                Ok((id, current.source_claim_ids().to_vec()))
            })
            .collect()
    }
}

fn validate_claim_correction(
    previous: &Claim,
    evidence: &ConversationEvidence,
    replacement: &Claim,
) -> Result<(), RepositoryError> {
    if previous.owner() != ClaimOwner::Person || replacement.owner() != ClaimOwner::Person {
        return Err(RepositoryError::new("only person claims can be corrected"));
    }
    if previous.status() != ClaimStatus::Current || previous.superseded_by().is_some() {
        return Err(RepositoryError::new("claim is not current"));
    }
    if replacement.status() != ClaimStatus::Current
        || replacement.supersedes() != Some(previous.id())
        || replacement.superseded_by().is_some()
    {
        return Err(RepositoryError::new("invalid correction successor"));
    }
    if replacement.statement().trim().is_empty()
        || replacement.statement().trim() == previous.statement().trim()
    {
        return Err(RepositoryError::new(
            "correction must provide a changed statement",
        ));
    }
    if !replacement.applicable_time().is_valid() {
        return Err(RepositoryError::new("correction time is invalid"));
    }
    if evidence.speaker() != Speaker::Person
        || evidence.verbatim() != replacement.statement()
        || evidence.recorded_at() != replacement.recorded_at()
    {
        return Err(RepositoryError::new(
            "correction evidence does not match its successor claim",
        ));
    }
    let [citation] = replacement.support() else {
        return Err(RepositoryError::new(
            "correction successor requires exactly one citation",
        ));
    };
    if citation.evidence_id() != evidence.id() || citation.quote() != evidence.verbatim() {
        return Err(RepositoryError::new(
            "correction citation does not match retained evidence",
        ));
    }
    Ok(())
}

fn insert_correction_evidence(
    connection: &Connection,
    evidence: &ConversationEvidence,
) -> Result<(), RepositoryError> {
    connection
        .execute(
            "INSERT INTO conversation_evidence
             (id, session_id, speaker, verbatim, recorded_at, counterpart_identity_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                to_sql_id(evidence.id().get())?,
                evidence.session_id().as_str(),
                encode_speaker(evidence.speaker()),
                evidence.verbatim(),
                evidence.recorded_at().as_millis(),
                evidence
                    .counterpart_reply_attribution()
                    .and_then(CounterpartReplyAttribution::identity_version)
                    .map(to_sql_id)
                    .transpose()?,
            ],
        )
        .map_err(repository_error)?;
    Ok(())
}

fn insert_correction_claim(connection: &Connection, claim: &Claim) -> Result<(), RepositoryError> {
    let supersedes = claim
        .supersedes()
        .ok_or_else(|| RepositoryError::new("correction successor has no predecessor"))?;
    let (applicable_kind, applicable_start, applicable_end) =
        encode_applicable_time(claim.applicable_time());
    connection
        .execute(
            "INSERT INTO claims
             (id, owner, statement, uncertainty, applicable_kind,
              applicable_start, applicable_end, recorded_at, supersedes_claim_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                to_sql_id(claim.id().get())?,
                encode_owner(claim.owner()),
                claim.statement(),
                claim.uncertainty().map(encode_uncertainty),
                applicable_kind,
                applicable_start,
                applicable_end,
                claim.recorded_at().as_millis(),
                to_sql_id(supersedes.get())?,
            ],
        )
        .map_err(repository_error)?;
    for (ordinal, citation) in claim.support().iter().enumerate() {
        connection
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
    Ok(())
}

fn propagate_claim_correction_to_memories(
    connection: &Connection,
    superseded_claim_id: ClaimId,
    replacement: &Claim,
) -> Result<(usize, usize), RepositoryError> {
    let affected = {
        let mut statement = connection
            .prepare(
                "SELECT DISTINCT v.memory_id, v.version
                 FROM long_term_memory_versions v
                 JOIN long_term_memory_sources s
                   ON s.memory_id = v.memory_id AND s.version = v.version
                 WHERE s.claim_id = ?1
                   AND v.version = (
                       SELECT MAX(latest.version) FROM long_term_memory_versions latest
                       WHERE latest.memory_id = v.memory_id
                   )
                   AND (SELECT e.status FROM long_term_memory_state_events e
                        WHERE e.memory_id = v.memory_id AND e.version = v.version
                        ORDER BY e.ordinal DESC LIMIT 1) IN (0, 1, 2, 4, 6, 7)
                 ORDER BY v.memory_id",
            )
            .map_err(repository_error)?;
        statement
            .query_map([to_sql_id(superseded_claim_id.get())?], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(repository_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(repository_error)?
    };

    let mut rebuilt = 0_usize;
    for (memory_id, version) in &affected {
        let memory_id = MemoryId::new(u64::try_from(*memory_id).map_err(repository_error)?)
            .ok_or_else(|| RepositoryError::new("invalid affected memory id"))?;
        let version = u64::try_from(*version).map_err(repository_error)?;
        let memory = load_memory_version(connection, memory_id, version)?;
        insert_memory_state_event(
            connection,
            memory_id,
            version,
            MemoryStatus::Superseded,
            replacement.recorded_at(),
        )?;
        let rebuilt_version = if memory.basis() == MemoryBasis::DirectEvidence {
            if memory.source_claim_ids() != [superseded_claim_id] {
                return Err(RepositoryError::new(
                    "direct memory has an invalid correction source set",
                ));
            }
            let next_version = version
                .checked_add(1)
                .ok_or_else(|| RepositoryError::new("memory version space exhausted"))?;
            insert_corrected_direct_memory_version(connection, &memory, next_version, replacement)?;
            rebuilt += 1;
            Some(next_version)
        } else {
            None
        };
        connection
            .execute(
                "INSERT INTO claim_correction_memory_work_items
                 (correction_claim_id, memory_id, affected_version, action,
                  rebuilt_version, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    to_sql_id(replacement.id().get())?,
                    to_sql_id(memory_id.get())?,
                    to_sql_id(version)?,
                    i64::from(rebuilt_version.is_none()),
                    rebuilt_version.map(to_sql_id).transpose()?,
                    replacement.recorded_at().as_millis(),
                ],
            )
            .map_err(repository_error)?;
    }
    Ok((affected.len(), rebuilt))
}

fn insert_corrected_direct_memory_version(
    connection: &Connection,
    previous: &MemoryVersion,
    version: u64,
    replacement: &Claim,
) -> Result<(), RepositoryError> {
    let (applicable_kind, applicable_start, applicable_end) =
        encode_applicable_time(replacement.applicable_time());
    connection
        .execute(
            "INSERT INTO long_term_memory_versions
             (memory_id, version, predecessor_version, subject, kind, statement,
              confidence, applicable_kind, applicable_start, applicable_end,
              salience_reason, basis, formed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                to_sql_id(previous.id().get())?,
                to_sql_id(version)?,
                to_sql_id(previous.version())?,
                encode_memory_subject(previous.subject()),
                encode_memory_kind(previous.kind()),
                replacement.statement(),
                encode_memory_confidence(previous.confidence()),
                applicable_kind,
                applicable_start,
                applicable_end,
                previous.salience_reason(),
                encode_memory_basis(previous.basis()),
                replacement.recorded_at().as_millis(),
            ],
        )
        .map_err(repository_error)?;
    connection
        .execute(
            "INSERT INTO long_term_memory_sources
             (memory_id, version, ordinal, claim_id) VALUES (?1, ?2, 0, ?3)",
            params![
                to_sql_id(previous.id().get())?,
                to_sql_id(version)?,
                to_sql_id(replacement.id().get())?,
            ],
        )
        .map_err(repository_error)?;
    let mut terms = BTreeSet::new();
    terms.extend(search_terms(replacement.statement()));
    terms.extend(search_terms(previous.salience_reason()));
    for term in terms {
        connection
            .execute(
                "INSERT INTO long_term_memory_terms (memory_id, version, term)
                 VALUES (?1, ?2, ?3)",
                params![to_sql_id(previous.id().get())?, to_sql_id(version)?, term],
            )
            .map_err(repository_error)?;
    }
    insert_memory_state_event(
        connection,
        previous.id(),
        version,
        MemoryStatus::Active,
        replacement.recorded_at(),
    )
}

fn update_retrieval_projection_for_correction(
    transaction: &rusqlite::Transaction<'_>,
    authority: &RetrievalAuthority,
    superseded_id: ClaimId,
    replacement: &Claim,
) -> Result<(), RepositoryError> {
    let changed = transaction
        .execute(
            "UPDATE retrieval_claim_documents SET claim_status = 1 WHERE claim_id = ?1",
            [to_sql_id(superseded_id.get())?],
        )
        .map_err(repository_error)?;
    if changed != 1 {
        return Err(RepositoryError::new(
            "superseded claim is missing from the current retrieval projection",
        ));
    }
    insert_retrieval_claims(transaction, std::slice::from_ref(replacement))
        .map_err(repository_error)?;
    let index_digest = retrieval_index_digest(transaction).map_err(repository_error)?;
    let metadata_changed = transaction
        .execute(
            "UPDATE retrieval_index_metadata
             SET contract_version = ?1, authority_digest = ?2, index_digest = ?3,
                 built_at = ?4, evidence_block_count = ?5,
                 ledger_claim_count = ?6, relation_count = ?7
             WHERE id = 1",
            params![
                RETRIEVAL_INDEX_VERSION,
                authority.digest.as_slice(),
                index_digest.as_slice(),
                authority.built_at_millis,
                i64::try_from(authority.blocks.len()).map_err(repository_error)?,
                i64::try_from(authority.claims.len()).map_err(repository_error)?,
                i64::try_from(authority.relations.len()).map_err(repository_error)?,
            ],
        )
        .map_err(repository_error)?;
    if metadata_changed != 1 {
        return Err(RepositoryError::new(
            "retrieval metadata disappeared during correction",
        ));
    }
    Ok(())
}

fn validate_persisted_memory_sources(
    connection: &Connection,
    proposal: &ValidatedMemoryProposal,
) -> Result<(), RepositoryError> {
    let expected_owner = match proposal.subject() {
        MemorySubject::Person => ClaimOwner::Person,
        MemorySubject::Counterpart => ClaimOwner::Counterpart,
        MemorySubject::Shared => ClaimOwner::Shared,
    };
    for claim_id in proposal.source_claim_ids() {
        let claim = load_claim(connection, *claim_id)?
            .ok_or_else(|| RepositoryError::new("memory source claim does not exist"))?;
        if claim.status() != ClaimStatus::Current {
            return Err(RepositoryError::new(
                "memory source claim is no longer current",
            ));
        }
        if claim.owner() != expected_owner {
            return Err(RepositoryError::new(
                "memory source claim changed ledger attribution",
            ));
        }
    }
    Ok(())
}

fn validate_persisted_pattern_maturity(
    connection: &Connection,
    current: &MemoryVersion,
    proposal: &ValidatedPatternMaturityProposal,
) -> Result<(), RepositoryError> {
    if proposal.new_support_claim_ids().is_empty()
        || proposal.discussion_evidence_refs().is_empty()
        || proposal.rationale().trim().is_empty()
    {
        return Err(RepositoryError::new(
            "pattern maturity qualification is incomplete",
        ));
    }
    let mut expected_sources = current.source_claim_ids().to_vec();
    for claim_id in proposal.new_support_claim_ids() {
        if !expected_sources.contains(claim_id) {
            expected_sources.push(*claim_id);
        }
    }
    if expected_sources != proposal.all_source_claim_ids() {
        return Err(RepositoryError::new(
            "pattern maturity source set changed after validation",
        ));
    }
    let expected_owner = match current.subject() {
        MemorySubject::Person => ClaimOwner::Person,
        MemorySubject::Counterpart => ClaimOwner::Counterpart,
        MemorySubject::Shared => ClaimOwner::Shared,
    };
    let mut base_evidence = BTreeSet::new();
    for claim_id in current.source_claim_ids() {
        let claim = load_claim(connection, *claim_id)?
            .ok_or_else(|| RepositoryError::new("pattern source claim does not exist"))?;
        for citation in claim.support() {
            base_evidence.insert(citation.evidence_id());
        }
    }
    let mut independent_new_evidence = BTreeSet::new();
    for claim_id in proposal.new_support_claim_ids() {
        let claim = load_claim(connection, *claim_id)?
            .ok_or_else(|| RepositoryError::new("new pattern support does not exist"))?;
        if claim.status() != ClaimStatus::Current || claim.owner() != expected_owner {
            return Err(RepositoryError::new(
                "new pattern support changed attribution or currentness",
            ));
        }
        for citation in claim.support() {
            let evidence = validate_pattern_citation(connection, citation)?;
            if !base_evidence.contains(&citation.evidence_id())
                && evidence.recorded_at().as_millis() > current.formed_at().as_millis()
            {
                independent_new_evidence.insert(citation.evidence_id());
            }
        }
    }
    if independent_new_evidence.is_empty() {
        return Err(RepositoryError::new(
            "pattern maturity has no independent new support",
        ));
    }
    let review = validate_pattern_citation(connection, proposal.counterexample_review_ref())?;
    if review.speaker() != Speaker::Counterpart
        || review.recorded_at().as_millis() <= current.formed_at().as_millis()
    {
        return Err(RepositoryError::new(
            "pattern maturity counterexample review is invalid",
        ));
    }
    validate_pattern_citations(connection, proposal.counter_evidence_refs(), true)?;
    let discussion =
        validate_pattern_citations(connection, proposal.discussion_evidence_refs(), false)?;
    let has_person = discussion
        .iter()
        .any(|evidence| evidence.speaker() == Speaker::Person);
    let has_counterpart = discussion
        .iter()
        .any(|evidence| evidence.speaker() == Speaker::Counterpart);
    if !has_person
        || !has_counterpart
        || discussion
            .iter()
            .any(|evidence| evidence.recorded_at().as_millis() <= current.formed_at().as_millis())
    {
        return Err(RepositoryError::new(
            "pattern maturity discussion is not two-sided and subsequent",
        ));
    }
    Ok(())
}

fn validate_pattern_citations(
    connection: &Connection,
    citations: &[EvidenceCitation],
    allow_empty: bool,
) -> Result<Vec<ConversationEvidence>, RepositoryError> {
    if (!allow_empty && citations.is_empty()) || citations.len() > MAX_DISPUTE_EVIDENCE {
        return Err(RepositoryError::new(
            "invalid pattern maturity evidence count",
        ));
    }
    let mut unique = BTreeSet::new();
    let mut evidence = Vec::with_capacity(citations.len());
    for citation in citations {
        if !unique.insert(citation.evidence_id()) {
            return Err(RepositoryError::new("duplicate pattern maturity evidence"));
        }
        evidence.push(validate_pattern_citation(connection, citation)?);
    }
    Ok(evidence)
}

fn validate_pattern_citation(
    connection: &Connection,
    citation: &EvidenceCitation,
) -> Result<ConversationEvidence, RepositoryError> {
    let evidence = load_conversation_evidence(connection, citation.evidence_id())?
        .ok_or_else(|| RepositoryError::new("pattern maturity evidence does not exist"))?;
    if citation.quote().trim().is_empty() || !evidence.verbatim().contains(citation.quote()) {
        return Err(RepositoryError::new(
            "pattern maturity evidence quote does not match",
        ));
    }
    Ok(evidence)
}

fn insert_memory_version(
    transaction: &rusqlite::Transaction<'_>,
    memory_id: MemoryId,
    version: u64,
    predecessor_version: Option<u64>,
    proposal: &ValidatedMemoryProposal,
    formed_at: Timestamp,
) -> Result<(), RepositoryError> {
    if proposal.initial_status() == MemoryStatus::Superseded {
        return Err(RepositoryError::new(
            "a new memory version cannot start superseded",
        ));
    }
    let (applicable_kind, applicable_start, applicable_end) =
        encode_applicable_time(proposal.applicable_time());
    transaction
        .execute(
            "INSERT INTO long_term_memory_versions
             (memory_id, version, predecessor_version, subject, kind, statement,
              confidence, applicable_kind, applicable_start, applicable_end,
              salience_reason, basis, formed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                to_sql_id(memory_id.get())?,
                to_sql_id(version)?,
                predecessor_version.map(to_sql_id).transpose()?,
                encode_memory_subject(proposal.subject()),
                encode_memory_kind(proposal.kind()),
                proposal.statement(),
                encode_memory_confidence(proposal.confidence()),
                applicable_kind,
                applicable_start,
                applicable_end,
                proposal.salience_reason(),
                encode_memory_basis(proposal.basis()),
                formed_at.as_millis(),
            ],
        )
        .map_err(repository_error)?;
    for (ordinal, claim_id) in proposal.source_claim_ids().iter().enumerate() {
        transaction
            .execute(
                "INSERT INTO long_term_memory_sources
                 (memory_id, version, ordinal, claim_id) VALUES (?1, ?2, ?3, ?4)",
                params![
                    to_sql_id(memory_id.get())?,
                    to_sql_id(version)?,
                    i64::try_from(ordinal).map_err(repository_error)?,
                    to_sql_id(claim_id.get())?,
                ],
            )
            .map_err(repository_error)?;
    }
    let mut terms = BTreeSet::new();
    terms.extend(search_terms(proposal.statement()));
    terms.extend(search_terms(proposal.salience_reason()));
    for term in terms {
        transaction
            .execute(
                "INSERT INTO long_term_memory_terms (memory_id, version, term)
                 VALUES (?1, ?2, ?3)",
                params![to_sql_id(memory_id.get())?, to_sql_id(version)?, term],
            )
            .map_err(repository_error)?;
    }
    insert_memory_state_event(
        transaction,
        memory_id,
        version,
        proposal.initial_status(),
        formed_at,
    )?;
    if let Some(review) = proposal.pattern_counterexample_review() {
        insert_memory_counterexample_review(transaction, memory_id, version, review)?;
    }
    Ok(())
}

fn insert_memory_counterexample_review(
    connection: &Connection,
    memory_id: MemoryId,
    version: u64,
    review: &EvidenceCitation,
) -> Result<(), RepositoryError> {
    connection
        .execute(
            "INSERT INTO long_term_memory_counterexample_reviews
             (memory_id, version, evidence_id, quote) VALUES (?1, ?2, ?3, ?4)",
            params![
                to_sql_id(memory_id.get())?,
                to_sql_id(version)?,
                to_sql_id(review.evidence_id().get())?,
                review.quote(),
            ],
        )
        .map_err(repository_error)?;
    Ok(())
}

fn insert_matured_pattern_version(
    transaction: &rusqlite::Transaction<'_>,
    previous: &MemoryVersion,
    version: u64,
    proposal: &ValidatedPatternMaturityProposal,
    proposed_at: Timestamp,
) -> Result<(), RepositoryError> {
    let (applicable_kind, applicable_start, applicable_end) =
        encode_applicable_time(previous.applicable_time());
    transaction
        .execute(
            "INSERT INTO long_term_memory_versions
             (memory_id, version, predecessor_version, subject, kind, statement,
              confidence, applicable_kind, applicable_start, applicable_end,
              salience_reason, basis, formed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                to_sql_id(previous.id().get())?,
                to_sql_id(version)?,
                to_sql_id(previous.version())?,
                encode_memory_subject(previous.subject()),
                encode_memory_kind(previous.kind()),
                previous.statement(),
                encode_memory_confidence(previous.confidence()),
                applicable_kind,
                applicable_start,
                applicable_end,
                previous.salience_reason(),
                encode_memory_basis(previous.basis()),
                proposed_at.as_millis(),
            ],
        )
        .map_err(repository_error)?;
    for (ordinal, claim_id) in proposal.all_source_claim_ids().iter().enumerate() {
        transaction
            .execute(
                "INSERT INTO long_term_memory_sources
                 (memory_id, version, ordinal, claim_id) VALUES (?1, ?2, ?3, ?4)",
                params![
                    to_sql_id(previous.id().get())?,
                    to_sql_id(version)?,
                    i64::try_from(ordinal).map_err(repository_error)?,
                    to_sql_id(claim_id.get())?,
                ],
            )
            .map_err(repository_error)?;
    }
    let mut terms = BTreeSet::new();
    terms.extend(search_terms(previous.statement()));
    terms.extend(search_terms(previous.salience_reason()));
    for term in terms {
        transaction
            .execute(
                "INSERT INTO long_term_memory_terms (memory_id, version, term)
                 VALUES (?1, ?2, ?3)",
                params![to_sql_id(previous.id().get())?, to_sql_id(version)?, term],
            )
            .map_err(repository_error)?;
    }
    insert_memory_state_event(
        transaction,
        previous.id(),
        version,
        MemoryStatus::SupportedCounterpartView,
        proposed_at,
    )?;
    insert_memory_counterexample_review(
        transaction,
        previous.id(),
        version,
        proposal.counterexample_review_ref(),
    )
}

fn insert_pattern_maturity_record(
    transaction: &rusqlite::Transaction<'_>,
    proposal: &ValidatedPatternMaturityProposal,
    to_version: u64,
    proposed_at: Timestamp,
) -> Result<(), RepositoryError> {
    transaction
        .execute(
            "INSERT INTO pattern_maturity_records
             (memory_id, from_version, to_version, rationale, proposed_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                to_sql_id(proposal.memory_id().get())?,
                to_sql_id(proposal.expected_version())?,
                to_sql_id(to_version)?,
                proposal.rationale(),
                proposed_at.as_millis(),
            ],
        )
        .map_err(repository_error)?;
    for (ordinal, claim_id) in proposal.new_support_claim_ids().iter().enumerate() {
        transaction
            .execute(
                "INSERT INTO pattern_maturity_new_support
                 (memory_id, to_version, ordinal, claim_id) VALUES (?1, ?2, ?3, ?4)",
                params![
                    to_sql_id(proposal.memory_id().get())?,
                    to_sql_id(to_version)?,
                    i64::try_from(ordinal).map_err(repository_error)?,
                    to_sql_id(claim_id.get())?,
                ],
            )
            .map_err(repository_error)?;
    }
    insert_pattern_maturity_evidence(
        transaction,
        proposal.memory_id(),
        to_version,
        0,
        proposal.counter_evidence_refs(),
    )?;
    insert_pattern_maturity_evidence(
        transaction,
        proposal.memory_id(),
        to_version,
        1,
        proposal.discussion_evidence_refs(),
    )
}

fn insert_pattern_maturity_evidence(
    transaction: &rusqlite::Transaction<'_>,
    memory_id: MemoryId,
    to_version: u64,
    role: i64,
    citations: &[EvidenceCitation],
) -> Result<(), RepositoryError> {
    for (ordinal, citation) in citations.iter().enumerate() {
        transaction
            .execute(
                "INSERT INTO pattern_maturity_evidence
                 (memory_id, to_version, role, ordinal, evidence_id, quote)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    to_sql_id(memory_id.get())?,
                    to_sql_id(to_version)?,
                    role,
                    i64::try_from(ordinal).map_err(repository_error)?,
                    to_sql_id(citation.evidence_id().get())?,
                    citation.quote(),
                ],
            )
            .map_err(repository_error)?;
    }
    Ok(())
}

fn insert_memory_state_event(
    connection: &Connection,
    memory_id: MemoryId,
    version: u64,
    status: MemoryStatus,
    occurred_at: Timestamp,
) -> Result<(), RepositoryError> {
    let ordinal: i64 = connection
        .query_row(
            "SELECT COALESCE(MAX(ordinal) + 1, 0)
             FROM long_term_memory_state_events
             WHERE memory_id = ?1 AND version = ?2",
            params![to_sql_id(memory_id.get())?, to_sql_id(version)?],
            |row| row.get(0),
        )
        .map_err(repository_error)?;
    connection
        .execute(
            "INSERT INTO long_term_memory_state_events
             (memory_id, version, ordinal, status, occurred_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                to_sql_id(memory_id.get())?,
                to_sql_id(version)?,
                ordinal,
                encode_memory_status(status),
                occurred_at.as_millis(),
            ],
        )
        .map_err(repository_error)?;
    Ok(())
}

fn insert_claim_state_event(
    connection: &Connection,
    claim_id: ClaimId,
    status: ClaimStatus,
    caused_by_claim_id: Option<ClaimId>,
    occurred_at: Timestamp,
) -> Result<(), RepositoryError> {
    let valid_cause = match status {
        ClaimStatus::Current => caused_by_claim_id.is_none(),
        ClaimStatus::Superseded => caused_by_claim_id.is_some_and(|id| id != claim_id),
    };
    if !valid_cause {
        return Err(RepositoryError::new("invalid claim state transition cause"));
    }
    let ordinal: i64 = connection
        .query_row(
            "SELECT COALESCE(MAX(ordinal) + 1, 0)
             FROM claim_state_events WHERE claim_id = ?1",
            [to_sql_id(claim_id.get())?],
            |row| row.get(0),
        )
        .map_err(repository_error)?;
    connection
        .execute(
            "INSERT INTO claim_state_events
             (claim_id, ordinal, status, caused_by_claim_id, occurred_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                to_sql_id(claim_id.get())?,
                ordinal,
                encode_claim_status(status),
                caused_by_claim_id
                    .map(|id| to_sql_id(id.get()))
                    .transpose()?,
                occurred_at.as_millis(),
            ],
        )
        .map_err(repository_error)?;
    Ok(())
}

fn insert_current_claim_state_event(
    connection: &Connection,
    claim_id: ClaimId,
    recorded_at: Timestamp,
) -> Result<(), RepositoryError> {
    insert_claim_state_event(
        connection,
        claim_id,
        ClaimStatus::Current,
        None,
        recorded_at,
    )
}

fn load_claim(connection: &Connection, id: ClaimId) -> Result<Option<Claim>, RepositoryError> {
    let stored = connection
        .query_row(
            "SELECT c.id, c.owner, c.statement, c.uncertainty, c.applicable_kind,
                    c.applicable_start, c.applicable_end, c.recorded_at,
                    c.supersedes_claim_id,
                    (SELECT e.status FROM claim_state_events e
                     WHERE e.claim_id = c.id ORDER BY e.ordinal DESC LIMIT 1),
                    (SELECT successor.id FROM claims successor
                     WHERE successor.supersedes_claim_id = c.id)
             FROM claims c WHERE c.id = ?1",
            [to_sql_id(id.get())?],
            stored_claim_from_row,
        )
        .optional()
        .map_err(repository_error)?;
    stored.map(|stored| stored.decode(connection)).transpose()
}

fn load_current_memory(
    connection: &Connection,
    id: MemoryId,
) -> Result<Option<MemoryVersion>, RepositoryError> {
    let version = connection
        .query_row(
            "SELECT MAX(version) FROM long_term_memory_versions WHERE memory_id = ?1",
            [to_sql_id(id.get())?],
            |row| row.get::<_, Option<i64>>(0),
        )
        .map_err(repository_error)?;
    let Some(version) = version else {
        return Ok(None);
    };
    let version = u64::try_from(version).map_err(repository_error)?;
    load_memory_version(connection, id, version).map(Some)
}

fn load_memory_versions(
    connection: &Connection,
    only_id: Option<MemoryId>,
) -> Result<Vec<MemoryVersion>, RepositoryError> {
    let mut statement = connection
        .prepare(
            "SELECT memory_id, version FROM long_term_memory_versions
             WHERE (?1 IS NULL OR memory_id = ?1)
             ORDER BY memory_id, version",
        )
        .map_err(repository_error)?;
    let rows = statement
        .query_map(
            [only_id.map(|id| to_sql_id(id.get())).transpose()?],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .map_err(repository_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(repository_error)?;
    drop(statement);
    rows.into_iter()
        .map(|(memory_id, version)| {
            let memory_id = u64::try_from(memory_id).map_err(repository_error)?;
            let memory_id = MemoryId::new(memory_id)
                .ok_or_else(|| RepositoryError::new("invalid persisted memory id"))?;
            let version = u64::try_from(version).map_err(repository_error)?;
            load_memory_version(connection, memory_id, version)
        })
        .collect()
}

fn load_memory_version(
    connection: &Connection,
    memory_id: MemoryId,
    version: u64,
) -> Result<MemoryVersion, RepositoryError> {
    let stored = connection
        .query_row(
            "SELECT predecessor_version, statement, subject, kind, confidence,
                    applicable_kind, applicable_start, applicable_end,
                    salience_reason, basis, formed_at,
                    (SELECT status FROM long_term_memory_state_events e
                     WHERE e.memory_id = v.memory_id AND e.version = v.version
                     ORDER BY ordinal DESC LIMIT 1)
             FROM long_term_memory_versions v
             WHERE memory_id = ?1 AND version = ?2",
            params![to_sql_id(memory_id.get())?, to_sql_id(version)?],
            |row| {
                Ok((
                    row.get::<_, Option<i64>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, i64>(10)?,
                    row.get::<_, Option<i64>>(11)?,
                ))
            },
        )
        .optional()
        .map_err(repository_error)?
        .ok_or_else(|| RepositoryError::new("memory version does not exist"))?;
    let (
        predecessor_version,
        statement,
        subject,
        kind,
        confidence,
        applicable_kind,
        applicable_start,
        applicable_end,
        salience_reason,
        basis,
        formed_at,
        status,
    ) = stored;
    let predecessor_version = predecessor_version
        .map(u64::try_from)
        .transpose()
        .map_err(repository_error)?;
    if (version == 1 && predecessor_version.is_some())
        || (version > 1 && predecessor_version != Some(version - 1))
    {
        return Err(RepositoryError::new(
            "invalid persisted memory version chain",
        ));
    }
    let status = status.ok_or_else(|| RepositoryError::new("memory version has no state event"))?;
    Ok(MemoryVersion::restore(
        memory_id,
        version,
        predecessor_version,
        statement,
        decode_memory_subject(subject)?,
        decode_memory_kind(kind)?,
        load_memory_sources(connection, memory_id, version)?,
        decode_applicable_time(applicable_kind, applicable_start, applicable_end)?,
        decode_memory_confidence(confidence)?,
        salience_reason,
        decode_memory_basis(basis)?,
        decode_memory_status(status)?,
        Timestamp::from_millis(formed_at),
        load_memory_counterexample_review(connection, memory_id, version)?,
    ))
}

fn load_memory_counterexample_review(
    connection: &Connection,
    memory_id: MemoryId,
    version: u64,
) -> Result<Option<EvidenceCitation>, RepositoryError> {
    let stored = connection
        .query_row(
            "SELECT evidence_id, quote
             FROM long_term_memory_counterexample_reviews
             WHERE memory_id = ?1 AND version = ?2",
            params![to_sql_id(memory_id.get())?, to_sql_id(version)?],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(repository_error)?;
    stored
        .map(|(evidence_id, quote)| {
            let evidence_id = u64::try_from(evidence_id).map_err(repository_error)?;
            Ok(EvidenceCitation::new(
                EvidenceId::from_raw(evidence_id),
                quote,
            ))
        })
        .transpose()
}

fn load_pattern_maturity_records(
    connection: &Connection,
    memory_id: MemoryId,
) -> Result<Vec<PatternMaturityRecord>, RepositoryError> {
    let mut statement = connection
        .prepare(
            "SELECT from_version, to_version, rationale, proposed_at
             FROM pattern_maturity_records
             WHERE memory_id = ?1 ORDER BY to_version",
        )
        .map_err(repository_error)?;
    let rows = statement
        .query_map([to_sql_id(memory_id.get())?], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .map_err(repository_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(repository_error)?;
    drop(statement);
    rows.into_iter()
        .map(|(from_version, to_version, rationale, proposed_at)| {
            let from_version = u64::try_from(from_version).map_err(repository_error)?;
            let to_version = u64::try_from(to_version).map_err(repository_error)?;
            let review = load_memory_counterexample_review(connection, memory_id, to_version)?
                .ok_or_else(|| {
                    RepositoryError::new("pattern maturity record has no counterexample review")
                })?;
            Ok(PatternMaturityRecord::restore(
                memory_id,
                from_version,
                to_version,
                load_pattern_maturity_support(connection, memory_id, to_version)?,
                load_pattern_maturity_evidence(connection, memory_id, to_version, 0)?,
                review,
                load_pattern_maturity_evidence(connection, memory_id, to_version, 1)?,
                rationale,
                Timestamp::from_millis(proposed_at),
            ))
        })
        .collect()
}

fn load_pattern_maturity_support(
    connection: &Connection,
    memory_id: MemoryId,
    to_version: u64,
) -> Result<Vec<ClaimId>, RepositoryError> {
    let mut statement = connection
        .prepare(
            "SELECT claim_id FROM pattern_maturity_new_support
             WHERE memory_id = ?1 AND to_version = ?2 ORDER BY ordinal",
        )
        .map_err(repository_error)?;
    statement
        .query_map(
            params![to_sql_id(memory_id.get())?, to_sql_id(to_version)?],
            |row| row.get::<_, i64>(0),
        )
        .map_err(repository_error)?
        .map(|claim_id| {
            let claim_id =
                u64::try_from(claim_id.map_err(repository_error)?).map_err(repository_error)?;
            Ok(ClaimId::from_raw(claim_id))
        })
        .collect()
}

fn load_pattern_maturity_evidence(
    connection: &Connection,
    memory_id: MemoryId,
    to_version: u64,
    role: i64,
) -> Result<Vec<EvidenceCitation>, RepositoryError> {
    let mut statement = connection
        .prepare(
            "SELECT evidence_id, quote FROM pattern_maturity_evidence
             WHERE memory_id = ?1 AND to_version = ?2 AND role = ?3
             ORDER BY ordinal",
        )
        .map_err(repository_error)?;
    statement
        .query_map(
            params![to_sql_id(memory_id.get())?, to_sql_id(to_version)?, role],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .map_err(repository_error)?
        .map(|stored| {
            let (evidence_id, quote) = stored.map_err(repository_error)?;
            let evidence_id = u64::try_from(evidence_id).map_err(repository_error)?;
            Ok(EvidenceCitation::new(
                EvidenceId::from_raw(evidence_id),
                quote,
            ))
        })
        .collect()
}

fn load_memory_sources(
    connection: &Connection,
    memory_id: MemoryId,
    version: u64,
) -> Result<Vec<ClaimId>, RepositoryError> {
    let mut statement = connection
        .prepare(
            "SELECT ordinal, claim_id FROM long_term_memory_sources
             WHERE memory_id = ?1 AND version = ?2 ORDER BY ordinal",
        )
        .map_err(repository_error)?;
    let rows = statement
        .query_map(
            params![to_sql_id(memory_id.get())?, to_sql_id(version)?],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .map_err(repository_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(repository_error)?;
    if rows.is_empty() || rows.len() > MAX_MEMORY_SOURCES {
        return Err(RepositoryError::new(
            "invalid persisted memory source count",
        ));
    }
    rows.into_iter()
        .enumerate()
        .map(|(expected_ordinal, (ordinal, claim_id))| {
            if usize::try_from(ordinal).ok() != Some(expected_ordinal) {
                return Err(RepositoryError::new(
                    "invalid persisted memory source order",
                ));
            }
            let claim_id = u64::try_from(claim_id).map_err(repository_error)?;
            Ok(ClaimId::from_raw(claim_id))
        })
        .collect()
}

fn load_conversation_evidence(
    connection: &Connection,
    id: EvidenceId,
) -> Result<Option<ConversationEvidence>, RepositoryError> {
    let stored = connection
        .query_row(
            "SELECT id, session_id, speaker, verbatim, recorded_at,
                    counterpart_identity_version
             FROM conversation_evidence WHERE id = ?1",
            [to_sql_id(id.get())?],
            stored_evidence_from_row,
        )
        .optional()
        .map_err(repository_error)?;
    stored.map(StoredEvidence::decode).transpose()
}

fn validate_persisted_evidence_citations(
    connection: &Connection,
    citations: &[EvidenceCitation],
) -> Result<(), RepositoryError> {
    if citations.is_empty() || citations.len() > MAX_DISPUTE_EVIDENCE {
        return Err(RepositoryError::new(
            "invalid memory dispute evidence count",
        ));
    }
    let mut unique = BTreeSet::new();
    for citation in citations {
        if !unique.insert(citation.evidence_id()) {
            return Err(RepositoryError::new("duplicate memory dispute evidence"));
        }
        let evidence = load_conversation_evidence(connection, citation.evidence_id())?
            .ok_or_else(|| RepositoryError::new("memory dispute evidence does not exist"))?;
        if citation.quote().trim().is_empty() || !evidence.verbatim().contains(citation.quote()) {
            return Err(RepositoryError::new(
                "memory dispute evidence quote does not match",
            ));
        }
    }
    Ok(())
}

fn insert_dispute_evidence(
    connection: &Connection,
    table: &str,
    dispute_id: u64,
    citations: &[EvidenceCitation],
) -> Result<(), RepositoryError> {
    let sql = match table {
        "memory_dispute_counter_evidence" => {
            "INSERT INTO memory_dispute_counter_evidence
             (dispute_id, ordinal, evidence_id, quote) VALUES (?1, ?2, ?3, ?4)"
        }
        "memory_dispute_review_evidence" => {
            "INSERT INTO memory_dispute_review_evidence
             (dispute_id, ordinal, evidence_id, quote) VALUES (?1, ?2, ?3, ?4)"
        }
        _ => {
            return Err(RepositoryError::new(
                "invalid memory dispute evidence table",
            ));
        }
    };
    for (ordinal, citation) in citations.iter().enumerate() {
        connection
            .execute(
                sql,
                params![
                    to_sql_id(dispute_id)?,
                    i64::try_from(ordinal).map_err(repository_error)?,
                    to_sql_id(citation.evidence_id().get())?,
                    citation.quote(),
                ],
            )
            .map_err(repository_error)?;
    }
    Ok(())
}

fn insert_dispute_terms<'a>(
    connection: &Connection,
    dispute_id: u64,
    values: impl IntoIterator<Item = &'a str>,
) -> Result<(), RepositoryError> {
    let mut terms = BTreeSet::new();
    for value in values {
        terms.extend(search_terms(value));
    }
    for term in terms {
        connection
            .execute(
                "INSERT OR IGNORE INTO memory_dispute_terms (dispute_id, term)
                 VALUES (?1, ?2)",
                params![to_sql_id(dispute_id)?, term],
            )
            .map_err(repository_error)?;
    }
    Ok(())
}

fn persist_dispute_review(
    transaction: &rusqlite::Transaction<'_>,
    review: &ValidatedMemoryDisputeReview,
    reviewed_at: Timestamp,
    revised_version: Option<u64>,
) -> Result<(), RepositoryError> {
    transaction
        .execute(
            "UPDATE memory_disputes
             SET outcome = ?1, reviewed_at = ?2, review_rationale = ?3,
                 revised_version = ?4
             WHERE id = ?5 AND outcome = 0",
            params![
                encode_dispute_outcome(review.outcome()),
                reviewed_at.as_millis(),
                review.rationale(),
                revised_version.map(to_sql_id).transpose()?,
                to_sql_id(review.dispute_id().get())?,
            ],
        )
        .map_err(repository_error)?;
    insert_dispute_evidence(
        transaction,
        "memory_dispute_review_evidence",
        review.dispute_id().get(),
        review.evidence(),
    )?;
    insert_dispute_terms(
        transaction,
        review.dispute_id().get(),
        std::iter::once(review.rationale())
            .chain(review.evidence().iter().map(EvidenceCitation::quote)),
    )
}

fn load_dispute_evidence(
    connection: &Connection,
    table: &str,
    dispute_id: MemoryDisputeId,
    required: bool,
) -> Result<Vec<EvidenceCitation>, RepositoryError> {
    let sql = match table {
        "memory_dispute_counter_evidence" => {
            "SELECT ordinal, evidence_id, quote FROM memory_dispute_counter_evidence
             WHERE dispute_id = ?1 ORDER BY ordinal"
        }
        "memory_dispute_review_evidence" => {
            "SELECT ordinal, evidence_id, quote FROM memory_dispute_review_evidence
             WHERE dispute_id = ?1 ORDER BY ordinal"
        }
        _ => {
            return Err(RepositoryError::new(
                "invalid memory dispute evidence table",
            ));
        }
    };
    let mut statement = connection.prepare(sql).map_err(repository_error)?;
    let rows = statement
        .query_map([to_sql_id(dispute_id.get())?], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(repository_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(repository_error)?;
    if (required && rows.is_empty()) || rows.len() > MAX_DISPUTE_EVIDENCE {
        return Err(RepositoryError::new(
            "invalid persisted memory dispute evidence count",
        ));
    }
    rows.into_iter()
        .enumerate()
        .map(|(expected, (ordinal, evidence_id, quote))| {
            if usize::try_from(ordinal).ok() != Some(expected) || quote.trim().is_empty() {
                return Err(RepositoryError::new(
                    "invalid persisted memory dispute evidence order",
                ));
            }
            let evidence_id = u64::try_from(evidence_id).map_err(repository_error)?;
            let evidence_id = EvidenceId::from_raw(evidence_id);
            let evidence = load_conversation_evidence(connection, evidence_id)?
                .ok_or_else(|| RepositoryError::new("memory dispute evidence is missing"))?;
            if !evidence.verbatim().contains(&quote) {
                return Err(RepositoryError::new(
                    "persisted memory dispute quote does not match",
                ));
            }
            Ok(EvidenceCitation::new(evidence_id, quote))
        })
        .collect()
}

fn load_memory_dispute(
    connection: &Connection,
    id: MemoryDisputeId,
) -> Result<Option<MemoryDispute>, RepositoryError> {
    let stored = connection
        .query_row(
            "SELECT memory_id, memory_version, reason, raised_at, outcome,
                    reviewed_at, review_rationale, revised_version
             FROM memory_disputes WHERE id = ?1",
            [to_sql_id(id.get())?],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                ))
            },
        )
        .optional()
        .map_err(repository_error)?;
    let Some((
        memory_id,
        memory_version,
        reason,
        raised_at,
        outcome,
        reviewed_at,
        rationale,
        revised_version,
    )) = stored
    else {
        return Ok(None);
    };
    let memory_id = u64::try_from(memory_id).map_err(repository_error)?;
    let memory_id = MemoryId::new(memory_id)
        .ok_or_else(|| RepositoryError::new("invalid persisted memory id"))?;
    let memory_version = u64::try_from(memory_version).map_err(repository_error)?;
    let outcome = decode_dispute_outcome(outcome)?;
    let revised_version = revised_version
        .map(u64::try_from)
        .transpose()
        .map_err(repository_error)?;
    let review = match outcome {
        MemoryDisputeOutcome::Open => {
            if reviewed_at.is_some() || rationale.is_some() || revised_version.is_some() {
                return Err(RepositoryError::new("open memory dispute has a review"));
            }
            None
        }
        MemoryDisputeOutcome::Retracted
        | MemoryDisputeOutcome::Revised
        | MemoryDisputeOutcome::Maintained
        | MemoryDisputeOutcome::Weakened => {
            validate_dispute_revision_link(
                connection,
                memory_id,
                memory_version,
                outcome,
                revised_version,
            )?;
            let reviewed_at = reviewed_at
                .ok_or_else(|| RepositoryError::new("resolved dispute has no timestamp"))?;
            let rationale = rationale
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| RepositoryError::new("resolved dispute has no rationale"))?;
            Some(MemoryDisputeReviewRecord::restore(
                outcome,
                rationale,
                load_dispute_evidence(connection, "memory_dispute_review_evidence", id, true)?,
                Timestamp::from_millis(reviewed_at),
            ))
        }
    };
    Ok(Some(MemoryDispute::restore(
        id,
        memory_id,
        memory_version,
        reason,
        load_dispute_evidence(connection, "memory_dispute_counter_evidence", id, true)?,
        Timestamp::from_millis(raised_at),
        outcome,
        review,
        revised_version,
    )))
}

fn validate_dispute_revision_link(
    connection: &Connection,
    memory_id: MemoryId,
    memory_version: u64,
    outcome: MemoryDisputeOutcome,
    revised_version: Option<u64>,
) -> Result<(), RepositoryError> {
    match outcome {
        MemoryDisputeOutcome::Revised => {
            let successor = revised_version
                .ok_or_else(|| RepositoryError::new("revised dispute has no successor version"))?;
            let successor = load_memory_version(connection, memory_id, successor)?;
            if successor.predecessor_version() != Some(memory_version) {
                return Err(RepositoryError::new(
                    "revised dispute successor does not follow the disputed version",
                ));
            }
        }
        MemoryDisputeOutcome::Retracted
        | MemoryDisputeOutcome::Maintained
        | MemoryDisputeOutcome::Weakened => {
            if revised_version.is_some() {
                return Err(RepositoryError::new(
                    "non-revised dispute has a successor version",
                ));
            }
        }
        MemoryDisputeOutcome::Open => {
            return Err(RepositoryError::new(
                "open memory dispute cannot carry a resolved revision link",
            ));
        }
    }
    Ok(())
}

fn load_memory_disputes(
    connection: &Connection,
    memory_id: MemoryId,
) -> Result<Vec<MemoryDispute>, RepositoryError> {
    let mut statement = connection
        .prepare("SELECT id FROM memory_disputes WHERE memory_id = ?1 ORDER BY id")
        .map_err(repository_error)?;
    let ids = statement
        .query_map([to_sql_id(memory_id.get())?], |row| row.get::<_, i64>(0))
        .map_err(repository_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(repository_error)?;
    ids.into_iter()
        .map(|id| {
            let id = u64::try_from(id).map_err(repository_error)?;
            let id = MemoryDisputeId::new(id)
                .ok_or_else(|| RepositoryError::new("invalid persisted dispute id"))?;
            load_memory_dispute(connection, id)?
                .ok_or_else(|| RepositoryError::new("persisted memory dispute is missing"))
        })
        .collect()
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
        insert_conversation_evidence(self.connection(), &evidence)
    }

    fn append_claim(&mut self, claim: Claim) -> Result<(), RepositoryError> {
        if claim.status() != ClaimStatus::Current
            || claim.supersedes().is_some()
            || claim.superseded_by().is_some()
        {
            return Err(RepositoryError::new(
                "versioned claim changes require the correction repository",
            ));
        }
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
        insert_current_claim_state_event(&transaction, claim.id(), claim.recorded_at())?;
        transaction.commit().map_err(repository_error)?;
        Ok(())
    }

    fn evidence(&self, id: EvidenceId) -> Result<Option<ConversationEvidence>, RepositoryError> {
        let stored = self
            .connection()
            .query_row(
                "SELECT id, session_id, speaker, verbatim, recorded_at,
                        counterpart_identity_version
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
                "SELECT id, session_id, speaker, verbatim, recorded_at,
                        counterpart_identity_version
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
                    "SELECT c.id, c.owner, c.statement, c.uncertainty, c.applicable_kind,
                            c.applicable_start, c.applicable_end, c.recorded_at,
                            c.supersedes_claim_id,
                            (SELECT e.status FROM claim_state_events e
                             WHERE e.claim_id = c.id ORDER BY e.ordinal DESC LIMIT 1),
                            (SELECT successor.id FROM claims successor
                             WHERE successor.supersedes_claim_id = c.id)
                     FROM claims c ORDER BY c.id",
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

    fn commit_pattern_maturity(
        &mut self,
        proposal: &PatternMaturityProposal,
        proposed_at: Timestamp,
    ) -> Result<PatternMaturityCommitOutcome, RepositoryError> {
        match commit_pattern_maturity_domain(self, proposal, proposed_at) {
            Ok(memory) => Ok(PatternMaturityCommitOutcome::Accepted(
                PatternMaturityReceipt::new(memory.id().get(), memory.version()),
            )),
            Err(MemoryError::InvalidPatternMaturity(_)) => {
                Ok(PatternMaturityCommitOutcome::QualificationRejected)
            }
            Err(MemoryError::Repository(error)) => Err(error),
            Err(error) => Err(RepositoryError::new(format!(
                "unexpected pattern maturity domain error: {error}"
            ))),
        }
    }
}

impl SharedExperienceRepository for VaultRepository {
    fn next_shared_agreement_candidate_id(&mut self) -> SharedAgreementCandidateId {
        let id = SharedAgreementCandidateId::from_raw(self.next_shared_agreement_candidate_id);
        self.next_shared_agreement_candidate_id = self
            .next_shared_agreement_candidate_id
            .checked_add(1)
            .expect("shared agreement candidate identifier space exhausted");
        id
    }

    fn stage_shared_agreement_candidate(
        &mut self,
        candidate: SharedAgreementCandidate,
    ) -> Result<(), RepositoryError> {
        if candidate.status() != SharedAgreementCandidateStatus::AwaitingPerson
            || candidate.version() == 0
            || candidate.predecessor_candidate_id().is_some()
            || !has_valid_candidate_boundaries(&candidate)
            || candidate.counterpart_assented_at().is_none()
            || candidate.decided_at().is_some()
            || candidate.claim_id().is_some()
            || candidate.statement().trim().is_empty()
        {
            return Err(RepositoryError::new(
                "new shared agreement candidate must await person confirmation",
            ));
        }
        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()
            .map_err(repository_error)?;
        validate_candidate_support(&transaction, candidate.support(), true, true)?;
        validate_candidate_supersession_targets(&transaction, &candidate)?;
        transaction
            .execute(
                "INSERT INTO shared_agreement_candidates
                 (id, statement, occurred_at, recorded_at, status, decided_at,
                  confirmed_claim_id, version, predecessor_candidate_id, scope,
                  effective_from, effective_until, end_condition,
                  awaiting_counterpart, counterpart_assented_at,
                  person_confirmed_at)
                 VALUES (?1, ?2, ?3, ?4, 0, NULL, NULL, ?5, NULL, ?6,
                         ?7, ?8, ?9, 0, ?10, NULL)",
                params![
                    to_sql_id(candidate.id().get())?,
                    candidate.statement(),
                    candidate.occurred_at().as_millis(),
                    candidate.recorded_at().as_millis(),
                    to_sql_id(candidate.version())?,
                    candidate.scope(),
                    candidate.effective_from().map(Timestamp::as_millis),
                    candidate.effective_until().map(Timestamp::as_millis),
                    candidate.end_condition(),
                    candidate
                        .counterpart_assented_at()
                        .map(Timestamp::as_millis),
                ],
            )
            .map_err(repository_error)?;
        insert_shared_candidate_supersessions(&transaction, &candidate)?;
        insert_shared_candidate_support(&transaction, &candidate)?;
        transaction.commit().map_err(repository_error)?;
        Ok(())
    }

    fn commit_shared_agreement_revision(
        &mut self,
        previous_id: SharedAgreementCandidateId,
        person_evidence: ConversationEvidence,
        revised: SharedAgreementCandidate,
        revised_at: Timestamp,
    ) -> Result<(), RepositoryError> {
        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()
            .map_err(repository_error)?;
        let previous = load_shared_agreement_candidate(&transaction, previous_id)?
            .ok_or_else(|| RepositoryError::new("shared agreement candidate does not exist"))?;
        if previous.status() != SharedAgreementCandidateStatus::AwaitingPerson
            || revised.status() != SharedAgreementCandidateStatus::AwaitingCounterpart
            || revised.version() != previous.version().saturating_add(1)
            || revised.predecessor_candidate_id() != Some(previous_id)
            || !has_valid_candidate_boundaries(&revised)
            || revised.counterpart_assented_at().is_some()
            || revised.decided_at().is_some()
            || revised.claim_id().is_some()
            || person_evidence.speaker() != Speaker::Person
        {
            return Err(RepositoryError::new("invalid shared agreement revision"));
        }
        insert_conversation_evidence(&transaction, &person_evidence)?;
        validate_candidate_support(&transaction, revised.support(), true, false)?;
        validate_candidate_supersession_targets(&transaction, &revised)?;
        let retired = transaction
            .execute(
                "UPDATE shared_agreement_candidates
                 SET status = 1, decided_at = ?1
                 WHERE id = ?2 AND status = 0 AND awaiting_counterpart = 0",
                params![revised_at.as_millis(), to_sql_id(previous_id.get())?],
            )
            .map_err(repository_error)?;
        if retired != 1 {
            return Err(RepositoryError::new(
                "shared agreement candidate is no longer signable",
            ));
        }
        transaction
            .execute(
                "INSERT INTO shared_agreement_candidates
                 (id, statement, occurred_at, recorded_at, status, decided_at,
                  confirmed_claim_id, version, predecessor_candidate_id, scope,
                  effective_from, effective_until, end_condition,
                  awaiting_counterpart, counterpart_assented_at,
                  person_confirmed_at)
                 VALUES (?1, ?2, ?3, ?4, 0, NULL, NULL, ?5, ?6, ?7,
                         ?8, ?9, ?10, 1, NULL, NULL)",
                params![
                    to_sql_id(revised.id().get())?,
                    revised.statement(),
                    revised.occurred_at().as_millis(),
                    revised.recorded_at().as_millis(),
                    to_sql_id(revised.version())?,
                    to_sql_id(previous_id.get())?,
                    revised.scope(),
                    revised.effective_from().map(Timestamp::as_millis),
                    revised.effective_until().map(Timestamp::as_millis),
                    revised.end_condition(),
                ],
            )
            .map_err(repository_error)?;
        insert_shared_candidate_supersessions(&transaction, &revised)?;
        insert_shared_candidate_support(&transaction, &revised)?;
        transaction.commit().map_err(repository_error)?;
        Ok(())
    }

    fn commit_counterpart_agreement_assent(
        &mut self,
        id: SharedAgreementCandidateId,
        version: u64,
        citation: EvidenceCitation,
        assented_at: Timestamp,
    ) -> Result<SharedAgreementCandidate, RepositoryError> {
        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()
            .map_err(repository_error)?;
        let candidate = load_shared_agreement_candidate(&transaction, id)?
            .ok_or_else(|| RepositoryError::new("shared agreement candidate does not exist"))?;
        if candidate.status() != SharedAgreementCandidateStatus::AwaitingCounterpart
            || candidate.version() != version
        {
            return Err(RepositoryError::new(
                "shared agreement candidate is not awaiting this counterpart assent",
            ));
        }
        validate_candidate_support(&transaction, std::slice::from_ref(&citation), false, true)?;
        let ordinal = i64::try_from(candidate.support().len()).map_err(repository_error)?;
        transaction
            .execute(
                "INSERT INTO shared_agreement_candidate_support
                 (candidate_id, ordinal, evidence_id, quote)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    to_sql_id(id.get())?,
                    ordinal,
                    to_sql_id(citation.evidence_id().get())?,
                    citation.quote(),
                ],
            )
            .map_err(repository_error)?;
        let affected = transaction
            .execute(
                "UPDATE shared_agreement_candidates
                 SET awaiting_counterpart = 0, counterpart_assented_at = ?1
                 WHERE id = ?2 AND version = ?3 AND status = 0
                       AND awaiting_counterpart = 1",
                params![
                    assented_at.as_millis(),
                    to_sql_id(id.get())?,
                    to_sql_id(version)?,
                ],
            )
            .map_err(repository_error)?;
        if affected != 1 {
            return Err(RepositoryError::new(
                "shared agreement candidate assent raced with another decision",
            ));
        }
        transaction.commit().map_err(repository_error)?;
        load_shared_agreement_candidate(self.connection(), id)?
            .ok_or_else(|| RepositoryError::new("persisted shared agreement candidate is missing"))
    }

    fn shared_agreement_candidate(
        &self,
        id: SharedAgreementCandidateId,
    ) -> Result<Option<SharedAgreementCandidate>, RepositoryError> {
        load_shared_agreement_candidate(self.connection(), id)
    }

    fn commit_shared_agreement_decision(
        &mut self,
        id: SharedAgreementCandidateId,
        decision: SharedAgreementDecision,
        confirmed: Option<SharedExperience>,
        decided_at: Timestamp,
    ) -> Result<SharedAgreementResolution, RepositoryError> {
        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()
            .map_err(repository_error)?;
        let candidate = load_shared_agreement_candidate(&transaction, id)?
            .ok_or_else(|| RepositoryError::new("shared agreement candidate does not exist"))?;
        if candidate.status() != SharedAgreementCandidateStatus::AwaitingPerson {
            return Err(RepositoryError::new(
                "shared agreement candidate is not awaiting person confirmation",
            ));
        }

        let (status, claim_id) = match decision {
            SharedAgreementDecision::Confirm => {
                let experience = confirmed.ok_or_else(|| {
                    RepositoryError::new("confirmed agreement requires a shared claim")
                })?;
                if experience.kind() != SharedExperienceKind::Agreement
                    || experience.claim().statement() != candidate.statement()
                    || experience.claim().support() != candidate.support()
                    || candidate.effective_from().is_none()
                    || experience.claim().applicable_time() != agreement_applicable_time(&candidate)
                {
                    return Err(RepositoryError::new(
                        "confirmed agreement does not match its immutable candidate",
                    ));
                }
                validate_candidate_supersession_targets(&transaction, &candidate)?;
                insert_shared_claim(&transaction, experience.claim())?;
                transaction
                    .execute(
                        "INSERT INTO shared_experiences
                         (claim_id, kind, candidate_id, ceremony_dismissed)
                         VALUES (?1, ?2, ?3, 1)",
                        params![
                            to_sql_id(experience.claim().id().get())?,
                            encode_shared_experience_kind(experience.kind()),
                            to_sql_id(id.get())?,
                        ],
                    )
                    .map_err(repository_error)?;
                (
                    SharedAgreementCandidateStatus::Confirmed,
                    Some(experience.claim().id()),
                )
            }
            SharedAgreementDecision::Defer => {
                if confirmed.is_some() {
                    return Err(RepositoryError::new(
                        "deferred agreement cannot append a shared claim",
                    ));
                }
                (SharedAgreementCandidateStatus::Deferred, None)
            }
        };
        transaction
            .execute(
                "UPDATE shared_agreement_candidates
                 SET status = ?1, decided_at = ?2, confirmed_claim_id = ?3,
                     person_confirmed_at = CASE WHEN ?1 = 2 THEN ?2 ELSE NULL END
                 WHERE id = ?4 AND status = 0 AND awaiting_counterpart = 0",
                params![
                    encode_shared_agreement_status(status),
                    decided_at.as_millis(),
                    claim_id.map(ClaimId::get).map(to_sql_id).transpose()?,
                    to_sql_id(id.get())?,
                ],
            )
            .map_err(repository_error)?;
        transaction.commit().map_err(repository_error)?;
        Ok(SharedAgreementResolution::new(id, status, claim_id))
    }

    fn commit_shared_experience(
        &mut self,
        experience: SharedExperience,
    ) -> Result<(), RepositoryError> {
        if matches!(
            experience.kind(),
            SharedExperienceKind::Agreement
                | SharedExperienceKind::AgreementBreach
                | SharedExperienceKind::AgreementWithdrawal
        ) {
            return Err(RepositoryError::new(
                "agreements, breaches, and withdrawals require their typed commit path",
            ));
        }
        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()
            .map_err(repository_error)?;
        insert_shared_claim(&transaction, experience.claim())?;
        transaction
            .execute(
                "INSERT INTO shared_experiences
                 (claim_id, kind, candidate_id, ceremony_dismissed)
                 VALUES (?1, ?2, NULL, ?3)",
                params![
                    to_sql_id(experience.claim().id().get())?,
                    encode_shared_experience_kind(experience.kind()),
                    i64::from(experience.ceremony_dismissed()),
                ],
            )
            .map_err(repository_error)?;
        transaction.commit().map_err(repository_error)?;
        Ok(())
    }

    fn commit_relational_constraint_departure(
        &mut self,
        experience: SharedExperience,
    ) -> Result<(), RepositoryError> {
        let departure = experience.constraint_departure().ok_or_else(|| {
            RepositoryError::new("agreement breach requires a constraint departure")
        })?;
        if experience.kind() != SharedExperienceKind::AgreementBreach
            || departure.reason().trim().is_empty()
        {
            return Err(RepositoryError::new(
                "invalid relational constraint departure",
            ));
        }
        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()
            .map_err(repository_error)?;
        let agreement_exists = transaction
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM shared_experiences
                    WHERE claim_id = ?1 AND kind = 0
                 )",
                [to_sql_id(departure.agreement_claim_id().get())?],
                |row| row.get::<_, bool>(0),
            )
            .map_err(repository_error)?;
        if !agreement_exists {
            return Err(RepositoryError::new("departed agreement does not exist"));
        }
        let agreement = load_claim(&transaction, departure.agreement_claim_id())?
            .ok_or_else(|| RepositoryError::new("departed agreement claim is missing"))?;
        if !agreement
            .support()
            .iter()
            .all(|citation| experience.claim().support().contains(citation))
            || !has_exact_counterpart_reason(
                &transaction,
                experience.claim().support(),
                departure.reason(),
            )?
        {
            return Err(RepositoryError::new(
                "agreement breach must preserve agreement support and exact reason evidence",
            ));
        }
        insert_shared_claim(&transaction, experience.claim())?;
        transaction
            .execute(
                "INSERT INTO shared_experiences
                 (claim_id, kind, candidate_id, ceremony_dismissed,
                  departed_agreement_claim_id, departure_reason)
                 VALUES (?1, ?2, NULL, 0, ?3, ?4)",
                params![
                    to_sql_id(experience.claim().id().get())?,
                    encode_shared_experience_kind(experience.kind()),
                    to_sql_id(departure.agreement_claim_id().get())?,
                    departure.reason(),
                ],
            )
            .map_err(repository_error)?;
        transaction.commit().map_err(repository_error)?;
        Ok(())
    }

    fn commit_agreement_withdrawal(
        &mut self,
        person_confirmation: Option<ConversationEvidence>,
        experience: SharedExperience,
    ) -> Result<(), RepositoryError> {
        let withdrawal = experience
            .agreement_withdrawal()
            .cloned()
            .ok_or_else(|| RepositoryError::new("agreement withdrawal metadata is missing"))?;
        if experience.kind() != SharedExperienceKind::AgreementWithdrawal
            || withdrawal.id() != experience.claim().id()
            || withdrawal.evidence_refs() != experience.claim().support()
            || withdrawal
                .reason()
                .is_some_and(|reason| reason.trim().is_empty())
            || (withdrawal.actor() == AgreementWithdrawalActor::Counterpart
                && withdrawal.reason().is_none())
        {
            return Err(RepositoryError::new("invalid agreement withdrawal"));
        }
        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()
            .map_err(repository_error)?;
        match (withdrawal.actor(), person_confirmation.as_ref()) {
            (AgreementWithdrawalActor::Person, Some(confirmation))
                if confirmation.speaker() == Speaker::Person
                    && confirmation.recorded_at() == withdrawal.effective_at() =>
            {
                insert_conversation_evidence(&transaction, confirmation)?;
            }
            (AgreementWithdrawalActor::Counterpart, None) => {}
            _ => return Err(RepositoryError::new("withdrawal actor evidence is invalid")),
        }
        if !stored_agreement_is_active_at(
            &transaction,
            withdrawal.agreement_claim_id(),
            withdrawal.effective_at(),
        )? {
            return Err(RepositoryError::new(
                "withdrawn shared agreement is not active",
            ));
        }
        let agreement = load_claim(&transaction, withdrawal.agreement_claim_id())?
            .ok_or_else(|| RepositoryError::new("withdrawn agreement claim is missing"))?;
        if !agreement
            .support()
            .iter()
            .all(|citation| experience.claim().support().contains(citation))
            || !has_exact_withdrawal_actor_evidence(
                &transaction,
                experience.claim().support(),
                &withdrawal,
            )?
        {
            return Err(RepositoryError::new(
                "agreement withdrawal must preserve agreement support and exact actor evidence",
            ));
        }
        insert_shared_claim(&transaction, experience.claim())?;
        transaction
            .execute(
                "INSERT INTO shared_experiences
                 (claim_id, kind, candidate_id, ceremony_dismissed,
                  departed_agreement_claim_id, departure_reason)
                 VALUES (?1, ?2, NULL, 0, NULL, NULL)",
                params![
                    to_sql_id(experience.claim().id().get())?,
                    encode_shared_experience_kind(experience.kind()),
                ],
            )
            .map_err(repository_error)?;
        transaction
            .execute(
                "INSERT INTO agreement_withdrawals
                 (claim_id, agreement_claim_id, actor, effective_at, reason)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    to_sql_id(withdrawal.id().get())?,
                    to_sql_id(withdrawal.agreement_claim_id().get())?,
                    encode_agreement_withdrawal_actor(withdrawal.actor()),
                    withdrawal.effective_at().as_millis(),
                    withdrawal.reason(),
                ],
            )
            .map_err(repository_error)?;
        transaction.commit().map_err(repository_error)?;
        Ok(())
    }

    fn all_shared_agreement_candidates(
        &self,
    ) -> Result<Vec<SharedAgreementCandidate>, RepositoryError> {
        let ids = self
            .connection()
            .prepare("SELECT id FROM shared_agreement_candidates ORDER BY id")
            .map_err(repository_error)?
            .query_map([], |row| row.get::<_, i64>(0))
            .map_err(repository_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(repository_error)?;
        ids.into_iter()
            .map(|id| {
                let id = u64::try_from(id).map_err(repository_error)?;
                load_shared_agreement_candidate(
                    self.connection(),
                    SharedAgreementCandidateId::from_raw(id),
                )?
                .ok_or_else(|| RepositoryError::new("persisted candidate is missing"))
            })
            .collect()
    }

    fn all_shared_experiences(&self) -> Result<Vec<SharedExperience>, RepositoryError> {
        let rows = self
            .connection()
            .prepare(
                "SELECT claim_id, kind, ceremony_dismissed,
                        departed_agreement_claim_id, departure_reason
                 FROM shared_experiences ORDER BY claim_id",
            )
            .map_err(repository_error)?
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            })
            .map_err(repository_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(repository_error)?;
        rows.into_iter()
            .map(
                |(claim_id, kind, dismissed, departed_claim_id, departure_reason)| {
                    let claim_id =
                        ClaimId::from_raw(u64::try_from(claim_id).map_err(repository_error)?);
                    let claim = load_claim(self.connection(), claim_id)?
                        .ok_or_else(|| RepositoryError::new("shared claim is missing"))?;
                    let kind = decode_shared_experience_kind(kind)?;
                    if kind == SharedExperienceKind::AgreementBreach {
                        let agreement_claim_id = departed_claim_id
                            .ok_or_else(|| RepositoryError::new("breach agreement is missing"))?;
                        let agreement_claim_id = ClaimId::from_raw(
                            u64::try_from(agreement_claim_id).map_err(repository_error)?,
                        );
                        let reason = departure_reason
                            .ok_or_else(|| RepositoryError::new("breach reason is missing"))?;
                        Ok(SharedExperience::restore_agreement_breach(
                            claim,
                            decode_bool(dismissed)?,
                            RelationalConstraintDeparture::new(agreement_claim_id, reason),
                        ))
                    } else if kind == SharedExperienceKind::AgreementWithdrawal {
                        let withdrawal = load_agreement_withdrawal(
                            self.connection(),
                            claim_id,
                            claim.support().to_vec(),
                        )?;
                        Ok(SharedExperience::restore_agreement_withdrawal(
                            claim,
                            decode_bool(dismissed)?,
                            withdrawal,
                        ))
                    } else {
                        Ok(SharedExperience::restore(
                            kind,
                            claim,
                            decode_bool(dismissed)?,
                        ))
                    }
                },
            )
            .collect()
    }

    fn dismiss_shared_experience_ceremony(
        &mut self,
        claim_id: ClaimId,
    ) -> Result<bool, RepositoryError> {
        let affected = self
            .connection()
            .execute(
                "UPDATE shared_experiences SET ceremony_dismissed = 1
                 WHERE claim_id = ?1",
                [to_sql_id(claim_id.get())?],
            )
            .map_err(repository_error)?;
        Ok(affected == 1)
    }
}

fn load_shared_agreement_candidate(
    connection: &Connection,
    id: SharedAgreementCandidateId,
) -> Result<Option<SharedAgreementCandidate>, RepositoryError> {
    let stored = connection
        .query_row(
            "SELECT statement, occurred_at, recorded_at, status, decided_at,
                    confirmed_claim_id, version, predecessor_candidate_id,
                    scope, effective_from, effective_until, end_condition,
                    awaiting_counterpart, counterpart_assented_at
             FROM shared_agreement_candidates WHERE id = ?1",
            [to_sql_id(id.get())?],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<i64>>(9)?,
                    row.get::<_, Option<i64>>(10)?,
                    row.get::<_, Option<String>>(11)?,
                    row.get::<_, i64>(12)?,
                    row.get::<_, Option<i64>>(13)?,
                ))
            },
        )
        .optional()
        .map_err(repository_error)?;
    let Some((
        statement,
        occurred_at,
        recorded_at,
        status,
        decided_at,
        claim_id,
        version,
        predecessor_candidate_id,
        scope,
        effective_from,
        effective_until,
        end_condition,
        awaiting_counterpart,
        counterpart_assented_at,
    )) = stored
    else {
        return Ok(None);
    };
    let support = load_shared_candidate_support(connection, id)?;
    let supersedes_agreement_ids = load_shared_candidate_supersessions(connection, id)?;
    let claim_id = claim_id
        .map(|value| {
            u64::try_from(value)
                .map(ClaimId::from_raw)
                .map_err(repository_error)
        })
        .transpose()?;
    Ok(Some(SharedAgreementCandidate::restore(
        id,
        u64::try_from(version).map_err(repository_error)?,
        predecessor_candidate_id
            .map(|value| {
                u64::try_from(value)
                    .map(SharedAgreementCandidateId::from_raw)
                    .map_err(repository_error)
            })
            .transpose()?,
        statement,
        scope,
        effective_from.map(Timestamp::from_millis),
        effective_until.map(Timestamp::from_millis),
        end_condition,
        supersedes_agreement_ids,
        support,
        Timestamp::from_millis(occurred_at),
        Timestamp::from_millis(recorded_at),
        decode_shared_agreement_status(status, awaiting_counterpart)?,
        counterpart_assented_at.map(Timestamp::from_millis),
        decided_at.map(Timestamp::from_millis),
        claim_id,
    )))
}

fn insert_conversation_evidence(
    connection: &Connection,
    evidence: &ConversationEvidence,
) -> Result<(), RepositoryError> {
    connection
        .execute(
            "INSERT INTO conversation_evidence
             (id, session_id, speaker, verbatim, recorded_at, counterpart_identity_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                to_sql_id(evidence.id().get())?,
                evidence.session_id().as_str(),
                encode_speaker(evidence.speaker()),
                evidence.verbatim(),
                evidence.recorded_at().as_millis(),
                evidence
                    .counterpart_reply_attribution()
                    .and_then(CounterpartReplyAttribution::identity_version)
                    .map(to_sql_id)
                    .transpose()?,
            ],
        )
        .map_err(repository_error)?;
    Ok(())
}

fn load_shared_candidate_support(
    connection: &Connection,
    id: SharedAgreementCandidateId,
) -> Result<Vec<EvidenceCitation>, RepositoryError> {
    connection
        .prepare(
            "SELECT evidence_id, quote
             FROM shared_agreement_candidate_support
             WHERE candidate_id = ?1 ORDER BY ordinal",
        )
        .map_err(repository_error)?
        .query_map([to_sql_id(id.get())?], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(repository_error)?
        .map(|stored| {
            let (evidence_id, quote) = stored.map_err(repository_error)?;
            let evidence_id = u64::try_from(evidence_id).map_err(repository_error)?;
            Ok(EvidenceCitation::new(
                EvidenceId::from_raw(evidence_id),
                quote,
            ))
        })
        .collect()
}

fn insert_shared_candidate_support(
    transaction: &rusqlite::Transaction<'_>,
    candidate: &SharedAgreementCandidate,
) -> Result<(), RepositoryError> {
    for (ordinal, citation) in candidate.support().iter().enumerate() {
        transaction
            .execute(
                "INSERT INTO shared_agreement_candidate_support
                 (candidate_id, ordinal, evidence_id, quote)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    to_sql_id(candidate.id().get())?,
                    i64::try_from(ordinal).map_err(repository_error)?,
                    to_sql_id(citation.evidence_id().get())?,
                    citation.quote(),
                ],
            )
            .map_err(repository_error)?;
    }
    Ok(())
}

fn load_shared_candidate_supersessions(
    connection: &Connection,
    id: SharedAgreementCandidateId,
) -> Result<Vec<ClaimId>, RepositoryError> {
    connection
        .prepare(
            "SELECT superseded_agreement_claim_id
             FROM shared_agreement_candidate_supersessions
             WHERE candidate_id = ?1 ORDER BY ordinal",
        )
        .map_err(repository_error)?
        .query_map([to_sql_id(id.get())?], |row| row.get::<_, i64>(0))
        .map_err(repository_error)?
        .map(|stored| {
            let claim_id =
                u64::try_from(stored.map_err(repository_error)?).map_err(repository_error)?;
            Ok(ClaimId::from_raw(claim_id))
        })
        .collect()
}

fn insert_shared_candidate_supersessions(
    transaction: &rusqlite::Transaction<'_>,
    candidate: &SharedAgreementCandidate,
) -> Result<(), RepositoryError> {
    for (ordinal, claim_id) in candidate.supersedes_agreement_ids().iter().enumerate() {
        let is_agreement = transaction
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM shared_experiences
                    WHERE claim_id = ?1 AND kind = 0
                 )",
                [to_sql_id(claim_id.get())?],
                |row| row.get::<_, bool>(0),
            )
            .map_err(repository_error)?;
        if !is_agreement {
            return Err(RepositoryError::new(
                "supersession target is not a shared agreement",
            ));
        }
        transaction
            .execute(
                "INSERT INTO shared_agreement_candidate_supersessions
                 (candidate_id, ordinal, superseded_agreement_claim_id)
                 VALUES (?1, ?2, ?3)",
                params![
                    to_sql_id(candidate.id().get())?,
                    i64::try_from(ordinal).map_err(repository_error)?,
                    to_sql_id(claim_id.get())?,
                ],
            )
            .map_err(repository_error)?;
    }
    Ok(())
}

fn validate_candidate_supersession_targets(
    transaction: &rusqlite::Transaction<'_>,
    candidate: &SharedAgreementCandidate,
) -> Result<(), RepositoryError> {
    let effective_from = candidate
        .effective_from()
        .ok_or_else(|| RepositoryError::new("agreement effective time is missing"))?;
    for claim_id in candidate.supersedes_agreement_ids() {
        let is_active = transaction
            .query_row(
                "SELECT EXISTS(
                    SELECT 1
                    FROM shared_agreement_candidates original
                    JOIN shared_experiences original_experience
                      ON original_experience.candidate_id = original.id
                     AND original_experience.kind = 0
                    WHERE original.confirmed_claim_id = ?1
                      AND original.status = 2
                      AND original.effective_from <= ?2
                      AND (original.effective_until IS NULL
                           OR original.effective_until >= ?2)
                      AND NOT EXISTS (
                          SELECT 1
                          FROM shared_agreement_candidate_supersessions edge
                          JOIN shared_agreement_candidates replacement
                            ON replacement.id = edge.candidate_id
                          JOIN shared_experiences replacement_experience
                            ON replacement_experience.candidate_id = replacement.id
                           AND replacement_experience.kind = 0
                          WHERE edge.superseded_agreement_claim_id = ?1
                            AND replacement.status = 2
                            AND replacement.effective_from <= ?2
                      )
                      AND NOT EXISTS (
                          SELECT 1 FROM agreement_withdrawals withdrawal
                          WHERE withdrawal.agreement_claim_id = ?1
                            AND withdrawal.effective_at <= ?2
                      )
                 )",
                params![to_sql_id(claim_id.get())?, effective_from.as_millis()],
                |row| row.get::<_, bool>(0),
            )
            .map_err(repository_error)?;
        if !is_active {
            return Err(RepositoryError::new(
                "superseded shared agreement is no longer active",
            ));
        }
    }
    Ok(())
}

fn validate_shared_support(
    connection: &Connection,
    support: &[EvidenceCitation],
) -> Result<(), RepositoryError> {
    let (has_person, has_counterpart) = validate_exact_support(connection, support)?;
    if !has_person || !has_counterpart {
        return Err(RepositoryError::new(
            "shared history requires evidence from both participants",
        ));
    }
    Ok(())
}

fn validate_candidate_support(
    connection: &Connection,
    support: &[EvidenceCitation],
    require_person: bool,
    require_counterpart: bool,
) -> Result<(), RepositoryError> {
    let (has_person, has_counterpart) = validate_exact_support(connection, support)?;
    if (require_person && !has_person) || (require_counterpart && !has_counterpart) {
        return Err(RepositoryError::new(
            "candidate signature evidence does not match its signing state",
        ));
    }
    Ok(())
}

fn validate_exact_support(
    connection: &Connection,
    support: &[EvidenceCitation],
) -> Result<(bool, bool), RepositoryError> {
    let mut has_person = false;
    let mut has_counterpart = false;
    for citation in support {
        let source = connection
            .query_row(
                "SELECT speaker, verbatim, counterpart_identity_version
                 FROM conversation_evidence WHERE id = ?1",
                [to_sql_id(citation.evidence_id().get())?],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(repository_error)?
            .ok_or_else(|| RepositoryError::new("shared support evidence does not exist"))?;
        if citation.quote().is_empty() || !source.1.contains(citation.quote()) {
            return Err(RepositoryError::new(
                "shared support is not an exact evidence quote",
            ));
        }
        match decode_speaker(source.0)? {
            Speaker::Person => has_person = true,
            Speaker::Counterpart if source.2.is_some() => has_counterpart = true,
            Speaker::Counterpart => {
                return Err(RepositoryError::new(
                    "shared support counterpart evidence is not identity-bound",
                ));
            }
        }
    }
    Ok((has_person, has_counterpart))
}

fn has_exact_counterpart_reason(
    connection: &Connection,
    support: &[EvidenceCitation],
    reason: &str,
) -> Result<bool, RepositoryError> {
    for citation in support {
        if citation.quote() != reason {
            continue;
        }
        let speaker_and_identity = connection
            .query_row(
                "SELECT speaker, counterpart_identity_version
                 FROM conversation_evidence WHERE id = ?1",
                [to_sql_id(citation.evidence_id().get())?],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?)),
            )
            .optional()
            .map_err(repository_error)?;
        if speaker_and_identity.is_some_and(|(speaker, identity_version)| {
            identity_version.is_some()
                && matches!(decode_speaker(speaker), Ok(Speaker::Counterpart))
        }) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn has_exact_withdrawal_actor_evidence(
    connection: &Connection,
    support: &[EvidenceCitation],
    withdrawal: &AgreementWithdrawal,
) -> Result<bool, RepositoryError> {
    for citation in support {
        let source = connection
            .query_row(
                "SELECT speaker, verbatim, recorded_at, counterpart_identity_version
                 FROM conversation_evidence WHERE id = ?1",
                [to_sql_id(citation.evidence_id().get())?],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(repository_error)?;
        let Some((speaker, verbatim, recorded_at, counterpart_identity_version)) = source else {
            continue;
        };
        if recorded_at != withdrawal.effective_at().as_millis()
            || citation.quote().is_empty()
            || !verbatim.contains(citation.quote())
        {
            continue;
        }
        let exact = match withdrawal.actor() {
            AgreementWithdrawalActor::Person => {
                decode_speaker(speaker)? == Speaker::Person
                    && citation.quote().contains("确认退出共同约定 Claim")
            }
            AgreementWithdrawalActor::Counterpart => {
                decode_speaker(speaker)? == Speaker::Counterpart
                    && counterpart_identity_version.is_some()
                    && withdrawal.reason() == Some(citation.quote())
            }
        };
        if exact {
            return Ok(true);
        }
    }
    Ok(false)
}

fn stored_agreement_is_active_at(
    connection: &Connection,
    agreement_claim_id: ClaimId,
    at: Timestamp,
) -> Result<bool, RepositoryError> {
    connection
        .query_row(
            "SELECT EXISTS(
                SELECT 1
                FROM shared_agreement_candidates original
                JOIN shared_experiences original_experience
                  ON original_experience.candidate_id = original.id
                 AND original_experience.kind = 0
                WHERE original.confirmed_claim_id = ?1
                  AND original.status = 2
                  AND original.effective_from <= ?2
                  AND (original.effective_until IS NULL
                       OR original.effective_until >= ?2)
                  AND NOT EXISTS (
                      SELECT 1
                      FROM shared_agreement_candidate_supersessions edge
                      JOIN shared_agreement_candidates replacement
                        ON replacement.id = edge.candidate_id
                      JOIN shared_experiences replacement_experience
                        ON replacement_experience.candidate_id = replacement.id
                       AND replacement_experience.kind = 0
                      WHERE edge.superseded_agreement_claim_id = ?1
                        AND replacement.status = 2
                        AND replacement.effective_from <= ?2
                  )
                  AND NOT EXISTS (
                      SELECT 1 FROM agreement_withdrawals withdrawal
                      WHERE withdrawal.agreement_claim_id = ?1
                        AND withdrawal.effective_at <= ?2
                  )
             )",
            params![to_sql_id(agreement_claim_id.get())?, at.as_millis()],
            |row| row.get::<_, bool>(0),
        )
        .map_err(repository_error)
}

fn load_agreement_withdrawal(
    connection: &Connection,
    claim_id: ClaimId,
    evidence_refs: Vec<EvidenceCitation>,
) -> Result<AgreementWithdrawal, RepositoryError> {
    connection
        .query_row(
            "SELECT agreement_claim_id, actor, effective_at, reason
             FROM agreement_withdrawals WHERE claim_id = ?1",
            [to_sql_id(claim_id.get())?],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()
        .map_err(repository_error)?
        .ok_or_else(|| RepositoryError::new("agreement withdrawal record is missing"))
        .and_then(|(agreement_claim_id, actor, effective_at, reason)| {
            Ok(AgreementWithdrawal::restore(
                claim_id,
                ClaimId::from_raw(u64::try_from(agreement_claim_id).map_err(repository_error)?),
                decode_agreement_withdrawal_actor(actor)?,
                Timestamp::from_millis(effective_at),
                reason,
                evidence_refs,
            ))
        })
}

fn agreement_applicable_time(candidate: &SharedAgreementCandidate) -> ApplicableTime {
    let start = candidate
        .effective_from()
        .expect("signable candidates have an effective time");
    candidate
        .effective_until()
        .map_or(ApplicableTime::Since(start), |end| {
            ApplicableTime::Between { start, end }
        })
}

fn has_valid_candidate_boundaries(candidate: &SharedAgreementCandidate) -> bool {
    let Some(scope) = candidate.scope() else {
        return false;
    };
    let Some(effective_from) = candidate.effective_from() else {
        return false;
    };
    !scope.trim().is_empty()
        && candidate
            .effective_until()
            .is_none_or(|until| until.as_millis() >= effective_from.as_millis())
        && candidate
            .end_condition()
            .is_none_or(|condition| !condition.trim().is_empty())
}

fn insert_shared_claim(
    transaction: &rusqlite::Transaction<'_>,
    claim: &Claim,
) -> Result<(), RepositoryError> {
    if claim.owner() != ClaimOwner::Shared
        || claim.status() != ClaimStatus::Current
        || claim.supersedes().is_some()
        || claim.superseded_by().is_some()
        || claim.statement().trim().is_empty()
    {
        return Err(RepositoryError::new("invalid shared experience claim"));
    }
    validate_shared_support(transaction, claim.support())?;
    let (applicable_kind, applicable_start, applicable_end) =
        encode_applicable_time(claim.applicable_time());
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
    insert_current_claim_state_event(transaction, claim.id(), claim.recorded_at())?;
    Ok(())
}

const fn encode_shared_experience_kind(kind: SharedExperienceKind) -> i64 {
    match kind {
        SharedExperienceKind::Agreement => 0,
        SharedExperienceKind::SubstantiveDisagreement => 1,
        SharedExperienceKind::RelationshipChange => 2,
        SharedExperienceKind::SharedAchievement => 3,
        SharedExperienceKind::AgreementBreach => 4,
        SharedExperienceKind::AgreementWithdrawal => 5,
    }
}

fn decode_shared_experience_kind(value: i64) -> Result<SharedExperienceKind, RepositoryError> {
    match value {
        0 => Ok(SharedExperienceKind::Agreement),
        1 => Ok(SharedExperienceKind::SubstantiveDisagreement),
        2 => Ok(SharedExperienceKind::RelationshipChange),
        3 => Ok(SharedExperienceKind::SharedAchievement),
        4 => Ok(SharedExperienceKind::AgreementBreach),
        5 => Ok(SharedExperienceKind::AgreementWithdrawal),
        _ => Err(RepositoryError::new(
            "invalid persisted shared experience kind",
        )),
    }
}

const fn encode_agreement_withdrawal_actor(actor: AgreementWithdrawalActor) -> i64 {
    match actor {
        AgreementWithdrawalActor::Person => 0,
        AgreementWithdrawalActor::Counterpart => 1,
    }
}

fn decode_agreement_withdrawal_actor(
    value: i64,
) -> Result<AgreementWithdrawalActor, RepositoryError> {
    match value {
        0 => Ok(AgreementWithdrawalActor::Person),
        1 => Ok(AgreementWithdrawalActor::Counterpart),
        _ => Err(RepositoryError::new(
            "invalid persisted agreement withdrawal actor",
        )),
    }
}

const fn encode_shared_agreement_status(status: SharedAgreementCandidateStatus) -> i64 {
    match status {
        SharedAgreementCandidateStatus::AwaitingCounterpart
        | SharedAgreementCandidateStatus::AwaitingPerson => 0,
        SharedAgreementCandidateStatus::Deferred => 1,
        SharedAgreementCandidateStatus::Confirmed => 2,
    }
}

fn decode_shared_agreement_status(
    value: i64,
    awaiting_counterpart: i64,
) -> Result<SharedAgreementCandidateStatus, RepositoryError> {
    match (value, awaiting_counterpart) {
        (0, 0) => Ok(SharedAgreementCandidateStatus::AwaitingPerson),
        (0, 1) => Ok(SharedAgreementCandidateStatus::AwaitingCounterpart),
        (1, 0) => Ok(SharedAgreementCandidateStatus::Deferred),
        (2, 0) => Ok(SharedAgreementCandidateStatus::Confirmed),
        _ => Err(RepositoryError::new(
            "invalid persisted shared agreement candidate status",
        )),
    }
}

fn decode_bool(value: i64) -> Result<bool, RepositoryError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(RepositoryError::new("invalid persisted boolean")),
    }
}

impl ClaimCorrectionRepository for VaultRepository {
    fn claim(&self, id: ClaimId) -> Result<Option<Claim>, RepositoryError> {
        load_claim(self.connection(), id)
    }

    fn commit_person_fact_correction(
        &mut self,
        evidence: ConversationEvidence,
        replacement: Claim,
    ) -> Result<ClaimCorrectionReceipt, RepositoryError> {
        RetrievalRepository::ensure_retrieval_index(self).map_err(repository_error)?;
        let mut authority = load_retrieval_authority(self).map_err(repository_error)?;
        let superseded_id = replacement
            .supersedes()
            .ok_or_else(|| RepositoryError::new("correction claim has no predecessor"))?;
        let previous = load_claim(self.connection(), superseded_id)?
            .ok_or_else(|| RepositoryError::new("claim does not exist"))?;
        validate_claim_correction(&previous, &evidence, &replacement)?;

        let authority_previous = authority
            .claims
            .iter_mut()
            .find(|claim| claim.id() == superseded_id)
            .ok_or_else(|| RepositoryError::new("claim is missing from retrieval authority"))?;
        *authority_previous = Claim::restore_versioned(
            previous.id(),
            previous.owner(),
            previous.statement().to_owned(),
            previous.support().to_vec(),
            previous.uncertainty(),
            previous.applicable_time(),
            previous.recorded_at(),
            ClaimStatus::Superseded,
            previous.supersedes(),
            Some(replacement.id()),
        );
        authority.claims.push(replacement.clone());
        authority.claims.sort_by_key(Claim::id);
        authority.built_at_millis = authority
            .built_at_millis
            .max(replacement.recorded_at().as_millis());
        authority.digest = retrieval_authority_digest(
            &authority.blocks,
            &authority.claims,
            &authority.entities,
            &authority.relations,
        );

        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()
            .map_err(repository_error)?;
        let current = load_claim(&transaction, superseded_id)?
            .ok_or_else(|| RepositoryError::new("claim does not exist"))?;
        validate_claim_correction(&current, &evidence, &replacement)?;
        insert_correction_evidence(&transaction, &evidence)?;
        insert_correction_claim(&transaction, &replacement)?;
        insert_current_claim_state_event(
            &transaction,
            replacement.id(),
            replacement.recorded_at(),
        )?;
        insert_claim_state_event(
            &transaction,
            superseded_id,
            ClaimStatus::Superseded,
            Some(replacement.id()),
            replacement.recorded_at(),
        )?;
        let (invalidated_memories, rebuilt_memories) =
            propagate_claim_correction_to_memories(&transaction, superseded_id, &replacement)?;

        update_retrieval_projection_for_correction(
            &transaction,
            &authority,
            superseded_id,
            &replacement,
        )?;
        transaction.commit().map_err(repository_error)?;
        Ok(ClaimCorrectionReceipt::new(
            evidence.id(),
            superseded_id,
            replacement.id(),
            invalidated_memories,
            rebuilt_memories,
            0,
            2,
        ))
    }
}

impl ForgetRepository for VaultRepository {
    fn commit_forget(
        &mut self,
        target: ForgetTarget,
        requested_at: Timestamp,
    ) -> Result<Option<ForgetReceipt>, RepositoryError> {
        self.forget_with_hook(target, requested_at, |_| Ok(()))
            .map_err(repository_error)
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

impl ActivityTimelineRepository for VaultRepository {
    fn recover_capture_timeline(
        &mut self,
        host_session_id: HostSessionId,
        started_at: Timestamp,
        recovered_host_gap: Option<HostGapReason>,
    ) -> Result<CaptureRecovery, RepositoryError> {
        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()
            .map_err(repository_error)?;
        require_current_open_host_session(&transaction, host_session_id)?;
        let Some(open) = current_capture_span(&transaction)? else {
            transaction.commit().map_err(repository_error)?;
            return Ok(CaptureRecovery::new(CaptureMode::Collecting, None));
        };

        let recovery = match open.kind() {
            CaptureSpanKind::Activity(_) => {
                transaction
                    .execute(
                        "UPDATE capture_spans SET ended_at = observed_until WHERE id = ?1",
                        [to_sql_id(open.id().get())?],
                    )
                    .map_err(repository_error)?;
                let reason =
                    recovered_host_gap.map_or(CaptureGapReason::Crash, CaptureGapReason::from);
                let gap_started_at = std::cmp::min(open.observed_until(), started_at);
                let gap = insert_capture_span(
                    &transaction,
                    host_session_id,
                    CaptureSpanKind::Gap(reason),
                    gap_started_at,
                    started_at,
                )?;
                CaptureRecovery::new(CaptureMode::Collecting, Some(gap.kind().clone()))
            }
            CaptureSpanKind::Gap(reason) => {
                let observed_until = std::cmp::max(open.observed_until(), started_at);
                transaction
                    .execute(
                        "UPDATE capture_spans SET observed_until = ?1 WHERE id = ?2",
                        params![observed_until.as_millis(), to_sql_id(open.id().get())?],
                    )
                    .map_err(repository_error)?;
                let mode = match reason {
                    CaptureGapReason::Paused => CaptureMode::Paused,
                    CaptureGapReason::SessionLocked => CaptureMode::Locked,
                    CaptureGapReason::ExplicitExit
                    | CaptureGapReason::Update
                    | CaptureGapReason::Crash
                    | CaptureGapReason::SourceUnavailable => CaptureMode::Collecting,
                };
                CaptureRecovery::new(mode, Some(open.kind().clone()))
            }
        };
        transaction.commit().map_err(repository_error)?;
        Ok(recovery)
    }

    fn record_capture_checkpoint(
        &mut self,
        host_session_id: HostSessionId,
        checkpoint: &CaptureCheckpoint,
    ) -> Result<CaptureSpan, RepositoryError> {
        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()
            .map_err(repository_error)?;
        require_current_open_host_session(&transaction, host_session_id)?;
        let persisted = match current_capture_span(&transaction)? {
            None => insert_capture_span(
                &transaction,
                host_session_id,
                checkpoint.kind().clone(),
                checkpoint.observed_at(),
                checkpoint.observed_at(),
            )?,
            Some(current) if current.kind() == checkpoint.kind() => {
                let observed_until =
                    std::cmp::max(current.observed_until(), checkpoint.observed_at());
                transaction
                    .execute(
                        "UPDATE capture_spans SET observed_until = ?1 WHERE id = ?2",
                        params![observed_until.as_millis(), to_sql_id(current.id().get())?,],
                    )
                    .map_err(repository_error)?;
                CaptureSpan::restore(
                    current.id(),
                    current.started_in_host_session(),
                    current.kind().clone(),
                    current.started_at(),
                    observed_until,
                    None,
                )
            }
            Some(current) => {
                let transition_at =
                    std::cmp::max(current.observed_until(), checkpoint.observed_at());
                transaction
                    .execute(
                        "UPDATE capture_spans
                         SET observed_until = ?1, ended_at = ?1 WHERE id = ?2",
                        params![transition_at.as_millis(), to_sql_id(current.id().get())?],
                    )
                    .map_err(repository_error)?;
                insert_capture_span(
                    &transaction,
                    host_session_id,
                    checkpoint.kind().clone(),
                    transition_at,
                    transition_at,
                )?
            }
        };
        transaction.commit().map_err(repository_error)?;
        Ok(persisted)
    }

    fn all_capture_spans(&self) -> Result<Vec<CaptureSpan>, RepositoryError> {
        let mut statement = self
            .connection()
            .prepare(
                "SELECT id, started_in_host_session_id, kind, application,
                        window_title, idle_state, gap_reason, started_at,
                        observed_until, ended_at
                 FROM capture_spans ORDER BY id",
            )
            .map_err(repository_error)?;
        statement
            .query_map([], stored_capture_span_from_row)
            .map_err(repository_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(repository_error)?
            .into_iter()
            .map(StoredCaptureSpan::decode)
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
            insert_current_claim_state_event(&transaction, item.claim_id(), item.recorded_at())?;
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
        validate_identity_chain(self.connection(), &identity)?;
        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()
            .map_err(repository_error)?;
        insert_identity_state(&transaction, &identity)?;
        transaction.commit().map_err(repository_error)?;
        Ok(())
    }

    fn current_identity_state(&self) -> Result<Option<IdentityStateVersion>, RepositoryError> {
        Ok(load_identity_states(self.connection())?.pop())
    }

    fn all_identity_states(&self) -> Result<Vec<IdentityStateVersion>, RepositoryError> {
        load_identity_states(self.connection())
    }
}

impl CounterpartRepository for VaultRepository {
    fn commit_initial_counterpart(
        &mut self,
        identity: IdentityStateVersion,
        bundle: SelfBundleVersion,
    ) -> Result<(), RepositoryError> {
        if !valid_initial_counterpart_pair(&identity, &bundle) {
            return Err(RepositoryError::new(
                "initial identity and Self Bundle versions do not form one valid pair",
            ));
        }

        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()
            .map_err(repository_error)?;
        let introduction_count = transaction
            .query_row(
                "SELECT count(*) FROM initial_self_introduction",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(repository_error)?;
        if introduction_count
            != i64::try_from(SelfIntroductionCategory::ALL.len()).map_err(repository_error)?
        {
            return Err(RepositoryError::new(
                "complete initial self introduction does not exist",
            ));
        }
        let identity_count = transaction
            .query_row("SELECT count(*) FROM identity_state_versions", [], |row| {
                row.get::<_, i64>(0)
            })
            .map_err(repository_error)?;
        let bundle_count = transaction
            .query_row("SELECT count(*) FROM self_bundle_versions", [], |row| {
                row.get::<_, i64>(0)
            })
            .map_err(repository_error)?;
        if identity_count != 0 || bundle_count != 0 {
            return Err(RepositoryError::new(
                "initial counterpart state already exists",
            ));
        }

        validate_identity_chain(&transaction, &identity)?;
        validate_self_bundle_chain(&transaction, &bundle)?;
        insert_identity_state(&transaction, &identity)?;
        insert_self_bundle(&transaction, &bundle)?;
        transaction.commit().map_err(repository_error)?;
        Ok(())
    }
}

impl IdentityEvolutionRepository for VaultRepository {
    fn conversation_readiness(&self) -> Result<CounterpartReadiness, RepositoryError> {
        CounterpartRepository::counterpart_readiness(self)
    }

    fn current_identity_context(&self) -> Result<Option<IdentityRuntimeContext>, RepositoryError> {
        let identity = self.current_identity_state()?;
        let bundle = self.current_self_bundle()?;
        match (identity, bundle) {
            (None, None) => Ok(None),
            (Some(_), None) => Err(RepositoryError::new(
                "identity exists without an initialized Self Bundle",
            )),
            (None, Some(_)) => Err(RepositoryError::new(
                "Self Bundle exists without an identity state",
            )),
            (Some(identity), Some(bundle)) => {
                if bundle.state().identity_state_version() != identity.version() {
                    return Err(RepositoryError::new(
                        "current Self Bundle does not reference the current identity",
                    ));
                }
                Ok(Some(identity_runtime_context(&identity, &bundle)))
            }
        }
    }

    fn current_self_bundle_snapshot(&self) -> Result<Option<SelfBundleSnapshot>, RepositoryError> {
        self.current_self_bundle().map(|bundle| {
            bundle.map(|bundle| {
                let state = bundle.state();
                SelfBundleSnapshot::restore(
                    bundle.version(),
                    state.constitution_version(),
                    state.identity_state_version(),
                    state.counterpart_experience_refs().to_vec(),
                    state.belief_refs().to_vec(),
                    state.relationship_state().to_owned(),
                    state.pending_intentions().to_vec(),
                )
            })
        })
    }

    fn counterpart_belief(&self, id: ClaimId) -> Result<Option<Claim>, RepositoryError> {
        load_claim(self.connection(), id)
    }

    fn commit_identity_revision(
        &mut self,
        revision: IdentityRevisionCommit,
    ) -> Result<IdentityRevisionReceipt, RepositoryError> {
        let current_identity = self
            .current_identity_state()?
            .ok_or_else(|| RepositoryError::new("identity is not initialized"))?;
        let current_bundle = self
            .current_self_bundle()?
            .ok_or_else(|| RepositoryError::new("Self Bundle is not initialized"))?;
        validate_identity_revision_commit(&revision, &current_identity, &current_bundle)?;

        let identity = identity_version_from_commit(&revision);
        let bundle_version = current_bundle
            .version()
            .checked_add(1)
            .ok_or_else(|| RepositoryError::new("Self Bundle version space exhausted"))?;
        let next_bundle_state = SelfBundleState::new(
            current_bundle.state().constitution_version(),
            identity.version(),
            current_bundle
                .state()
                .counterpart_experience_refs()
                .to_vec(),
            current_bundle.state().belief_refs().to_vec(),
            current_bundle.state().relationship_state(),
            current_bundle.state().pending_intentions().to_vec(),
        )
        .map_err(repository_error)?;
        let next_bundle = SelfBundleVersion::restore(
            bundle_version,
            Some(current_bundle.version()),
            next_bundle_state,
            Some(WakeCommit::new(
                WakeTrigger::ConversationStarted,
                WakeExit::Completed,
            )),
            identity.formed_at(),
        );

        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()
            .map_err(repository_error)?;
        recheck_identity_revision_versions(&transaction, &revision)?;
        insert_identity_state(&transaction, &identity)?;
        insert_self_bundle(&transaction, &next_bundle)?;
        transaction.commit().map_err(repository_error)?;
        Ok(IdentityRevisionReceipt::new(
            identity.version(),
            bundle_version,
        ))
    }

    fn identity_history(&self) -> Result<Vec<IdentityStateSnapshot>, RepositoryError> {
        self.all_identity_states().map(|states| {
            states
                .iter()
                .map(identity_state_snapshot)
                .collect::<Vec<_>>()
        })
    }
}

impl ReflectionInvitationRepository for VaultRepository {
    fn next_reflection_invitation_id(&mut self) -> ReflectionInvitationId {
        let id = ReflectionInvitationId::from_raw(self.next_reflection_invitation_id);
        self.next_reflection_invitation_id = self
            .next_reflection_invitation_id
            .checked_add(1)
            .expect("reflection invitation identifier space exhausted");
        id
    }

    fn commit_reflection_invitation(
        &mut self,
        invitation: ReflectionInvitation,
    ) -> Result<ReflectionInvitationReceipt, RepositoryError> {
        if invitation.id().get() == 0 || invitation.state() != ReflectionInvitationState::Pending {
            return Err(RepositoryError::new("invalid new reflection invitation"));
        }
        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()
            .map_err(repository_error)?;
        let open_count = transaction
            .query_row(
                "SELECT count(*) FROM reflection_invitations WHERE state != 4",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(repository_error)?;
        if usize::try_from(open_count).map_err(repository_error)? >= MAX_OPEN_REFLECTION_INVITATIONS
        {
            return Err(RepositoryError::new(
                "open reflection invitation budget exceeded",
            ));
        }
        insert_reflection_invitation(&transaction, &invitation)?;
        transaction.commit().map_err(repository_error)?;
        Ok(ReflectionInvitationReceipt::new(
            invitation.id(),
            invitation.state(),
        ))
    }

    fn transition_reflection_invitation(
        &mut self,
        expected_state: ReflectionInvitationState,
        invitation: ReflectionInvitation,
    ) -> Result<ReflectionInvitationReceipt, RepositoryError> {
        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()
            .map_err(repository_error)?;
        let current = load_reflection_invitation(&transaction, invitation.id())?
            .ok_or_else(|| RepositoryError::new("reflection invitation does not exist"))?;
        if current.state() != expected_state
            || !reflection_immutable_fields_match(&current, &invitation)
            || !valid_reflection_transition(&current, &invitation)
        {
            return Err(RepositoryError::new(
                "reflection invitation compare-and-swap failed",
            ));
        }
        let changed = transaction
            .execute(
                "UPDATE reflection_invitations
                 SET state = ?1, updated_at = ?2, next_eligible_at = ?3,
                     last_offered_at = ?4, defer_count = ?5, mute_prompted = ?6
                 WHERE id = ?7 AND state = ?8",
                params![
                    encode_reflection_state(invitation.state()),
                    invitation.updated_at().as_millis(),
                    invitation.next_eligible_at().map(Timestamp::as_millis),
                    invitation.last_offered_at().map(Timestamp::as_millis),
                    i64::from(invitation.defer_count()),
                    i64::from(invitation.mute_prompted()),
                    to_sql_id(invitation.id().get())?,
                    encode_reflection_state(expected_state),
                ],
            )
            .map_err(repository_error)?;
        if changed != 1 {
            return Err(RepositoryError::new(
                "reflection invitation compare-and-swap failed",
            ));
        }
        transaction.commit().map_err(repository_error)?;
        Ok(ReflectionInvitationReceipt::new(
            invitation.id(),
            invitation.state(),
        ))
    }

    fn reflection_invitation(
        &self,
        id: ReflectionInvitationId,
    ) -> Result<Option<ReflectionInvitation>, RepositoryError> {
        load_reflection_invitation(self.connection(), id)
    }

    fn all_reflection_invitations(&self) -> Result<Vec<ReflectionInvitation>, RepositoryError> {
        load_reflection_invitations(self.connection())
    }
}

impl SelfBundleRepository for VaultRepository {
    fn append_self_bundle(&mut self, bundle: SelfBundleVersion) -> Result<(), RepositoryError> {
        validate_self_bundle_chain(self.connection(), &bundle)?;
        let transaction = self
            .connection
            .as_mut()
            .expect("an open vault always owns a database connection")
            .transaction()
            .map_err(repository_error)?;
        insert_self_bundle(&transaction, &bundle)?;
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

fn validate_identity_chain(
    connection: &Connection,
    identity: &IdentityStateVersion,
) -> Result<(), RepositoryError> {
    let current = connection
        .query_row(
            "SELECT MAX(version) FROM identity_state_versions",
            [],
            |row| row.get::<_, Option<i64>>(0),
        )
        .map_err(repository_error)?
        .map(u64::try_from)
        .transpose()
        .map_err(repository_error)?;
    match current {
        None if identity.version() == 1 && identity.predecessor_version().is_none() => Ok(()),
        Some(version)
            if identity.version() == version.saturating_add(1)
                && identity.predecessor_version() == Some(version) =>
        {
            Ok(())
        }
        _ => Err(RepositoryError::new(
            "identity version does not continue the current immutable chain",
        )),
    }
}

fn valid_initial_counterpart_pair(
    identity: &IdentityStateVersion,
    bundle: &SelfBundleVersion,
) -> bool {
    identity.version() == 1
        && identity.predecessor_version().is_none()
        && bundle.version() == 1
        && bundle.predecessor_version().is_none()
        && bundle.wake_commit().is_none()
        && bundle.state().constitution_version() == 1
        && bundle.state().identity_state_version() == identity.version()
        && bundle.state().relationship_state() == identity.profile().relationship_posture()
        && bundle.state().counterpart_experience_refs().is_empty()
        && bundle.state().belief_refs().is_empty()
        && bundle.state().pending_intentions().is_empty()
        && bundle.committed_at() == identity.formed_at()
}

fn insert_identity_state(
    transaction: &rusqlite::Transaction<'_>,
    identity: &IdentityStateVersion,
) -> Result<(), RepositoryError> {
    let version = to_sql_id(identity.version())?;
    let profile = identity.profile();
    transaction
        .execute(
            "INSERT INTO identity_state_versions
             (version, predecessor_version, name, expression_traits, viewpoints,
              value_priorities, relationship_posture, own_goals, change_reason, formed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                version,
                identity.predecessor_version().map(to_sql_id).transpose()?,
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
    Ok(())
}

fn load_identity_states(
    connection: &Connection,
) -> Result<Vec<IdentityStateVersion>, RepositoryError> {
    let mut statement = connection
        .prepare(
            "SELECT version, predecessor_version, name, expression_traits, viewpoints,
                    value_priorities, relationship_posture, own_goals, change_reason, formed_at
             FROM identity_state_versions ORDER BY version",
        )
        .map_err(repository_error)?;
    let rows = statement
        .query_map([], |row| {
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
        })
        .map_err(repository_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(repository_error)?;
    let mut identities = Vec::with_capacity(rows.len());
    for (
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
    ) in rows
    {
        let version = u64::try_from(version).map_err(repository_error)?;
        identities.push(IdentityStateVersion::restore(
            version,
            predecessor_version
                .map(u64::try_from)
                .transpose()
                .map_err(repository_error)?,
            IdentityProfile::new(
                name,
                expression_traits,
                viewpoints,
                value_priorities,
                relationship_posture,
                own_goals,
            ),
            change_reason,
            load_identity_evidence(connection, version)?,
            Timestamp::from_millis(formed_at),
        ));
    }
    for (index, identity) in identities.iter().enumerate() {
        let expected_version = u64::try_from(index)
            .map_err(repository_error)?
            .saturating_add(1);
        let expected_predecessor = (index > 0).then_some(expected_version.saturating_sub(1));
        if identity.version() != expected_version
            || identity.predecessor_version() != expected_predecessor
        {
            return Err(RepositoryError::new(
                "persisted identity chain is not contiguous and immutable",
            ));
        }
    }
    Ok(identities)
}

fn identity_state_snapshot(identity: &IdentityStateVersion) -> IdentityStateSnapshot {
    IdentityStateSnapshot::restore(
        identity.version(),
        identity.predecessor_version(),
        IdentityProfileSnapshot::new(
            identity.profile().name(),
            identity.profile().expression_traits(),
            identity.profile().viewpoints(),
            identity.profile().value_priorities(),
            identity.profile().relationship_posture(),
            identity.profile().own_goals(),
        ),
        identity.change_reason(),
        identity.evidence_refs().to_vec(),
        identity.formed_at(),
    )
}

fn identity_runtime_context(
    identity: &IdentityStateVersion,
    bundle: &SelfBundleVersion,
) -> IdentityRuntimeContext {
    IdentityRuntimeContext::new(
        bundle.state().constitution_version(),
        bundle.version(),
        identity_state_snapshot(identity),
    )
}

const fn encode_reflection_importance(value: ReflectionImportance) -> i64 {
    match value {
        ReflectionImportance::Ordinary => 0,
        ReflectionImportance::Important => 1,
        ReflectionImportance::ImmediateSafetyRisk => 2,
    }
}

fn decode_reflection_importance(value: i64) -> Result<ReflectionImportance, RepositoryError> {
    match value {
        0 => Ok(ReflectionImportance::Ordinary),
        1 => Ok(ReflectionImportance::Important),
        2 => Ok(ReflectionImportance::ImmediateSafetyRisk),
        _ => Err(RepositoryError::new("invalid reflection importance")),
    }
}

const fn encode_reflection_basis(value: ReflectionInvitationBasis) -> i64 {
    match value {
        ReflectionInvitationBasis::ImportantSingleChange => 0,
        ReflectionInvitationBasis::RepeatedPattern => 1,
    }
}

fn decode_reflection_basis(value: i64) -> Result<ReflectionInvitationBasis, RepositoryError> {
    match value {
        0 => Ok(ReflectionInvitationBasis::ImportantSingleChange),
        1 => Ok(ReflectionInvitationBasis::RepeatedPattern),
        _ => Err(RepositoryError::new("invalid reflection basis")),
    }
}

const fn encode_reflection_state(value: ReflectionInvitationState) -> i64 {
    match value {
        ReflectionInvitationState::Pending => 0,
        ReflectionInvitationState::Offered => 1,
        ReflectionInvitationState::Deferred => 2,
        ReflectionInvitationState::MutedByPerson => 3,
        ReflectionInvitationState::Resolved => 4,
    }
}

fn decode_reflection_state(value: i64) -> Result<ReflectionInvitationState, RepositoryError> {
    match value {
        0 => Ok(ReflectionInvitationState::Pending),
        1 => Ok(ReflectionInvitationState::Offered),
        2 => Ok(ReflectionInvitationState::Deferred),
        3 => Ok(ReflectionInvitationState::MutedByPerson),
        4 => Ok(ReflectionInvitationState::Resolved),
        _ => Err(RepositoryError::new("invalid reflection invitation state")),
    }
}

fn reflection_immutable_fields_match(
    current: &ReflectionInvitation,
    updated: &ReflectionInvitation,
) -> bool {
    current.topic_key() == updated.topic_key()
        && current.observation() == updated.observation()
        && current.evidence_refs() == updated.evidence_refs()
        && current.why_now() == updated.why_now()
        && current.importance() == updated.importance()
        && current.basis() == updated.basis()
        && current.created_at() == updated.created_at()
}

fn valid_reflection_transition(
    current: &ReflectionInvitation,
    updated: &ReflectionInvitation,
) -> bool {
    if updated.updated_at().as_millis() < current.updated_at().as_millis() {
        return false;
    }
    match (current.state(), updated.state()) {
        (
            ReflectionInvitationState::Pending
            | ReflectionInvitationState::Deferred
            | ReflectionInvitationState::MutedByPerson,
            ReflectionInvitationState::Offered,
        ) if current.state() != ReflectionInvitationState::MutedByPerson
            || current.importance() == ReflectionImportance::ImmediateSafetyRisk =>
        {
            updated.next_eligible_at().is_none()
                && updated.last_offered_at() == Some(updated.updated_at())
                && updated.defer_count() == current.defer_count()
                && updated.mute_prompted() == (current.mute_prompted() || current.defer_count() > 0)
        }
        (ReflectionInvitationState::Offered, ReflectionInvitationState::Deferred) => {
            updated.next_eligible_at().is_some()
                && updated.last_offered_at() == current.last_offered_at()
                && updated.defer_count() == current.defer_count().saturating_add(1)
                && updated.mute_prompted() == current.mute_prompted()
        }
        (
            ReflectionInvitationState::Offered,
            ReflectionInvitationState::MutedByPerson | ReflectionInvitationState::Resolved,
        ) => {
            updated.next_eligible_at().is_none()
                && updated.last_offered_at() == current.last_offered_at()
                && updated.defer_count() == current.defer_count()
                && updated.mute_prompted() == current.mute_prompted()
        }
        _ => false,
    }
}

fn insert_reflection_invitation(
    transaction: &rusqlite::Transaction<'_>,
    invitation: &ReflectionInvitation,
) -> Result<(), RepositoryError> {
    transaction
        .execute(
            "INSERT INTO reflection_invitations
         (id, topic_key, observation, why_now, importance, basis, state,
          created_at, updated_at, next_eligible_at, last_offered_at,
          defer_count, mute_prompted)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                to_sql_id(invitation.id().get())?,
                invitation.topic_key(),
                invitation.observation(),
                invitation.why_now(),
                encode_reflection_importance(invitation.importance()),
                encode_reflection_basis(invitation.basis()),
                encode_reflection_state(invitation.state()),
                invitation.created_at().as_millis(),
                invitation.updated_at().as_millis(),
                invitation.next_eligible_at().map(Timestamp::as_millis),
                invitation.last_offered_at().map(Timestamp::as_millis),
                i64::from(invitation.defer_count()),
                i64::from(invitation.mute_prompted()),
            ],
        )
        .map_err(repository_error)?;
    for (ordinal, citation) in invitation.evidence_refs().iter().enumerate() {
        transaction
            .execute(
                "INSERT INTO reflection_invitation_evidence
             (invitation_id, ordinal, evidence_id, quote) VALUES (?1, ?2, ?3, ?4)",
                params![
                    to_sql_id(invitation.id().get())?,
                    i64::try_from(ordinal).map_err(repository_error)?,
                    to_sql_id(citation.evidence_id().get())?,
                    citation.quote(),
                ],
            )
            .map_err(repository_error)?;
    }
    Ok(())
}

fn load_reflection_invitations(
    connection: &Connection,
) -> Result<Vec<ReflectionInvitation>, RepositoryError> {
    let ids = {
        let mut statement = connection
            .prepare("SELECT id FROM reflection_invitations ORDER BY id")
            .map_err(repository_error)?;
        statement
            .query_map([], |row| row.get::<_, i64>(0))
            .map_err(repository_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(repository_error)?
    };
    ids.into_iter()
        .map(|id| {
            let id = ReflectionInvitationId::from_raw(u64::try_from(id).map_err(repository_error)?);
            load_reflection_invitation(connection, id)?
                .ok_or_else(|| RepositoryError::new("persisted reflection invitation is missing"))
        })
        .collect()
}

fn load_reflection_invitation(
    connection: &Connection,
    id: ReflectionInvitationId,
) -> Result<Option<ReflectionInvitation>, RepositoryError> {
    let stored = connection
        .query_row(
            "SELECT topic_key, observation, why_now, importance, basis, state,
                    created_at, updated_at, next_eligible_at, last_offered_at,
                    defer_count, mute_prompted
             FROM reflection_invitations WHERE id = ?1",
            [to_sql_id(id.get())?],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, Option<i64>>(8)?,
                    row.get::<_, Option<i64>>(9)?,
                    row.get::<_, i64>(10)?,
                    row.get::<_, i64>(11)?,
                ))
            },
        )
        .optional()
        .map_err(repository_error)?;
    let Some((
        topic_key,
        observation,
        why_now,
        importance,
        basis,
        state,
        created_at,
        updated_at,
        next_eligible_at,
        last_offered_at,
        defer_count,
        mute_prompted,
    )) = stored
    else {
        return Ok(None);
    };
    Ok(Some(ReflectionInvitation::restore(
        id,
        topic_key,
        observation,
        load_reflection_invitation_evidence(connection, id)?,
        why_now,
        decode_reflection_importance(importance)?,
        decode_reflection_basis(basis)?,
        decode_reflection_state(state)?,
        Timestamp::from_millis(created_at),
        Timestamp::from_millis(updated_at),
        next_eligible_at.map(Timestamp::from_millis),
        last_offered_at.map(Timestamp::from_millis),
        u32::try_from(defer_count).map_err(repository_error)?,
        match mute_prompted {
            0 => false,
            1 => true,
            _ => return Err(RepositoryError::new("invalid reflection mute prompt flag")),
        },
    )))
}

fn load_reflection_invitation_evidence(
    connection: &Connection,
    id: ReflectionInvitationId,
) -> Result<Vec<EvidenceCitation>, RepositoryError> {
    let mut statement = connection
        .prepare(
            "SELECT evidence_id, quote FROM reflection_invitation_evidence
             WHERE invitation_id = ?1 ORDER BY ordinal",
        )
        .map_err(repository_error)?;
    statement
        .query_map([to_sql_id(id.get())?], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(repository_error)?
        .map(|stored| {
            let (evidence_id, quote) = stored.map_err(repository_error)?;
            Ok(EvidenceCitation::new(
                EvidenceId::from_raw(u64::try_from(evidence_id).map_err(repository_error)?),
                quote,
            ))
        })
        .collect()
}

fn validate_identity_revision_commit(
    revision: &IdentityRevisionCommit,
    identity: &IdentityStateVersion,
    bundle: &SelfBundleVersion,
) -> Result<(), RepositoryError> {
    let expected_next = identity
        .version()
        .checked_add(1)
        .ok_or_else(|| RepositoryError::new("identity version space exhausted"))?;
    if revision.expected_identity_version() != identity.version()
        || revision.expected_self_bundle_version() != bundle.version()
        || revision.constitution_version() != bundle.state().constitution_version()
        || bundle.state().identity_state_version() != identity.version()
        || revision.state().version() != expected_next
        || revision.state().predecessor_version() != Some(identity.version())
    {
        return Err(RepositoryError::new(
            "identity revision does not continue the current identity and Self Bundle",
        ));
    }
    Ok(())
}

fn identity_version_from_commit(revision: &IdentityRevisionCommit) -> IdentityStateVersion {
    let state = revision.state();
    IdentityStateVersion::restore(
        state.version(),
        state.predecessor_version(),
        IdentityProfile::new(
            state.profile().name(),
            state.profile().expression_traits(),
            state.profile().viewpoints(),
            state.profile().value_priorities(),
            state.profile().relationship_posture(),
            state.profile().own_goals(),
        ),
        state.change_reason(),
        state.evidence_refs().to_vec(),
        state.formed_at(),
    )
}

fn recheck_identity_revision_versions(
    transaction: &rusqlite::Transaction<'_>,
    revision: &IdentityRevisionCommit,
) -> Result<(), RepositoryError> {
    let current_identity = transaction
        .query_row(
            "SELECT MAX(version) FROM identity_state_versions",
            [],
            |row| row.get::<_, Option<i64>>(0),
        )
        .map_err(repository_error)?
        .and_then(|value| u64::try_from(value).ok());
    let current_bundle = transaction
        .query_row(
            "SELECT version, constitution_version, identity_state_version
             FROM self_bundle_versions ORDER BY version DESC LIMIT 1",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()
        .map_err(repository_error)?;
    let expected_bundle = current_bundle
        .map(|(bundle, constitution, identity)| {
            Ok((
                u64::try_from(bundle).map_err(repository_error)?,
                u64::try_from(constitution).map_err(repository_error)?,
                u64::try_from(identity).map_err(repository_error)?,
            ))
        })
        .transpose()?;
    if current_identity != Some(revision.expected_identity_version())
        || expected_bundle
            != Some((
                revision.expected_self_bundle_version(),
                revision.constitution_version(),
                revision.expected_identity_version(),
            ))
    {
        return Err(RepositoryError::new(
            "identity or Self Bundle changed before revision commit",
        ));
    }
    Ok(())
}

fn insert_self_bundle(
    transaction: &rusqlite::Transaction<'_>,
    bundle: &SelfBundleVersion,
) -> Result<(), RepositoryError> {
    let (wake_trigger, wake_exit) = encode_wake_commit(bundle.wake_commit())?;
    let version = to_sql_id(bundle.version())?;
    let state = bundle.state();
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
    insert_self_bundle_children(transaction, version, state)
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

fn load_runtime_profile(connection: &Connection) -> Result<RuntimeProfile, VaultError> {
    let (base_url, model, bearer_key) = connection
        .query_row(
            "SELECT base_url, model, bearer_key FROM runtime_profiles
             WHERE singleton_id = ?1",
            [RUNTIME_PROFILE_SINGLETON_ID],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?
        .ok_or(VaultError::InvalidKeyOrCorrupt)?;
    let target =
        RuntimeTarget::new(&base_url, &model).map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
    validate_responses_bearer_token(bearer_key.as_deref())
        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
    if target.base_url() != base_url || target.model() != model {
        return Err(VaultError::InvalidKeyOrCorrupt);
    }
    Ok(RuntimeProfile {
        base_url,
        model,
        bearer_key: bearer_key.map(Zeroizing::new),
    })
}

fn runtime_profile_view(profile: &RuntimeProfile) -> RuntimeProfileView {
    RuntimeProfileView {
        base_url: profile.base_url.clone(),
        model: profile.model.clone(),
        api_key_configured: profile.bearer_key.is_some(),
        api_key_last_four: profile.bearer_key().and_then(redacted_last_four_chars),
    }
}

fn redacted_last_four_chars(value: &str) -> Option<String> {
    let mut suffix = value.chars().rev().take(5).collect::<Vec<_>>();
    if suffix.len() <= 4 {
        return None;
    }
    suffix.truncate(4);
    suffix.reverse();
    Some(suffix.into_iter().collect())
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

fn insert_understanding_projection(
    transaction: &rusqlite::Transaction<'_>,
    projection_id: u64,
    build: &ProjectionBuild,
) -> Result<(), VaultError> {
    let recipe = build.recipe();
    transaction.execute(
        "INSERT INTO understanding_projections
         (id, contract_version, trigger_kind, trigger_detail, recall_count,
          projection_kind, subject, requested_at, generation, status, material_digest)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, 0, ?9)",
        params![
            to_vault_sql_id(projection_id)?,
            UNDERSTANDING_CONTRACT_VERSION,
            encode_projection_trigger_kind(recipe.trigger().kind()),
            recipe.trigger().detail(),
            recipe.trigger().recall_count().map(i64::from),
            encode_projection_kind(recipe.content().kind()),
            recipe.subject(),
            recipe.requested_at_millis(),
            build.material_digest().as_slice(),
        ],
    )?;
    let source_ordinals = recipe
        .sources()
        .into_iter()
        .enumerate()
        .map(|(ordinal, reference)| (reference, ordinal))
        .collect::<HashMap<_, _>>();
    for (reference, ordinal) in &source_ordinals {
        transaction.execute(
            "INSERT INTO understanding_projection_sources
             (projection_id, ordinal, evidence_id, block_id)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                to_vault_sql_id(projection_id)?,
                i64::try_from(*ordinal).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                to_vault_sql_id(reference.evidence_id())?,
                to_vault_sql_id(reference.block_id().get())?,
            ],
        )?;
    }
    for (statement_ordinal, statement) in recipe.content().statements().iter().enumerate() {
        transaction.execute(
            "INSERT INTO understanding_projection_statements
             (projection_id, ordinal, statement) VALUES (?1, ?2, ?3)",
            params![
                to_vault_sql_id(projection_id)?,
                i64::try_from(statement_ordinal).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                statement.text(),
            ],
        )?;
        for reference in statement.sources() {
            let source_ordinal = source_ordinals
                .get(reference)
                .ok_or(VaultError::InvalidKeyOrCorrupt)?;
            transaction.execute(
                "INSERT INTO understanding_projection_statement_sources
                 (projection_id, statement_ordinal, source_ordinal)
                 VALUES (?1, ?2, ?3)",
                params![
                    to_vault_sql_id(projection_id)?,
                    i64::try_from(statement_ordinal)
                        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                    i64::try_from(*source_ordinal).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                ],
            )?;
        }
    }
    replace_understanding_artifact(transaction, projection_id, build)?;
    insert_understanding_event(
        transaction,
        projection_id,
        ProjectionStatus::Active,
        None,
        recipe.requested_at_millis(),
    )
}

fn replace_understanding_artifact(
    transaction: &rusqlite::Transaction<'_>,
    projection_id: u64,
    build: &ProjectionBuild,
) -> Result<(), VaultError> {
    transaction.execute(
        "DELETE FROM understanding_projection_artifacts WHERE projection_id = ?1",
        [to_vault_sql_id(projection_id)?],
    )?;
    transaction.execute(
        "INSERT INTO understanding_projection_artifacts
         (projection_id, contract_version, material_digest, built_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            to_vault_sql_id(projection_id)?,
            UNDERSTANDING_CONTRACT_VERSION,
            build.material_digest().as_slice(),
            build.recipe().requested_at_millis(),
        ],
    )?;
    let mut terms = BTreeSet::new();
    terms.extend(search_terms(build.recipe().subject()));
    terms.extend(search_terms(build.recipe().trigger().detail()));
    for statement in build.recipe().content().statements() {
        terms.extend(search_terms(statement.text()));
    }
    for term in terms {
        transaction.execute(
            "INSERT INTO understanding_projection_terms (projection_id, term) VALUES (?1, ?2)",
            params![to_vault_sql_id(projection_id)?, term],
        )?;
    }
    Ok(())
}

fn insert_understanding_event(
    transaction: &rusqlite::Transaction<'_>,
    projection_id: u64,
    status: ProjectionStatus,
    reason: Option<EvidenceBlockRef>,
    occurred_at_millis: i64,
) -> Result<(), VaultError> {
    let ordinal: i64 = transaction.query_row(
        "SELECT COALESCE(MAX(ordinal) + 1, 0)
         FROM understanding_projection_events WHERE projection_id = ?1",
        [to_vault_sql_id(projection_id)?],
        |row| row.get(0),
    )?;
    transaction.execute(
        "INSERT INTO understanding_projection_events
         (projection_id, ordinal, status, reason_evidence_id, reason_block_id, occurred_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            to_vault_sql_id(projection_id)?,
            ordinal,
            encode_projection_status(status),
            reason
                .map(|reference| to_vault_sql_id(reference.evidence_id()))
                .transpose()?,
            reason
                .map(|reference| to_vault_sql_id(reference.block_id().get()))
                .transpose()?,
            occurred_at_millis,
        ],
    )?;
    Ok(())
}

fn load_understanding_projection(
    connection: &Connection,
    id: ProjectionId,
) -> Result<Option<StoredProjectionRecipe>, VaultError> {
    let stored = connection
        .query_row(
            "SELECT contract_version, trigger_kind, trigger_detail, recall_count,
                    projection_kind, subject, requested_at, generation, status, material_digest
             FROM understanding_projections WHERE id = ?1",
            [to_vault_sql_id(id.get())?],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, Vec<u8>>(9)?,
                ))
            },
        )
        .optional()?;
    let Some((
        contract,
        trigger_kind,
        trigger_detail,
        recall_count,
        projection_kind,
        subject,
        requested_at_millis,
        generation,
        status,
        digest,
    )) = stored
    else {
        return Ok(None);
    };
    if contract != UNDERSTANDING_CONTRACT_VERSION {
        return Err(VaultError::InvalidKeyOrCorrupt);
    }
    let trigger = decode_projection_trigger(trigger_kind, trigger_detail, recall_count)?;
    let statements = load_understanding_statements(connection, id.get())?;
    let content = match decode_projection_kind(projection_kind)? {
        ProjectionKind::EventChain => ProjectionContent::EventChain(statements),
        ProjectionKind::PersonTopicRelations => ProjectionContent::PersonTopicRelations(statements),
        ProjectionKind::PhaseSummary => {
            let [statement] = <[SourcedStatement; 1]>::try_from(statements)
                .map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
            ProjectionContent::PhaseSummary(statement)
        }
    };
    let recipe = ProjectionRecipe::new(trigger, subject, content, requested_at_millis)
        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
    let source_count: i64 = connection.query_row(
        "SELECT count(*) FROM understanding_projection_sources WHERE projection_id = ?1",
        [to_vault_sql_id(id.get())?],
        |row| row.get(0),
    )?;
    if usize::try_from(source_count).ok() != Some(recipe.sources().len()) {
        return Err(VaultError::InvalidKeyOrCorrupt);
    }
    let digest: [u8; 32] = digest
        .try_into()
        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
    let projection = StoredProjection::new(
        id,
        u64::try_from(generation).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
        decode_projection_status(status)?,
        digest,
    );
    Ok(Some(StoredProjectionRecipe::new(projection, recipe)))
}

fn load_understanding_statements(
    connection: &Connection,
    projection_id: u64,
) -> Result<Vec<SourcedStatement>, VaultError> {
    let projection_id_sql = to_vault_sql_id(projection_id)?;
    let mut statement = connection.prepare(
        "SELECT ordinal, statement FROM understanding_projection_statements
         WHERE projection_id = ?1 ORDER BY ordinal",
    )?;
    let rows = statement
        .query_map([projection_id_sql], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    rows.into_iter()
        .enumerate()
        .map(|(expected_ordinal, (ordinal, text))| {
            if usize::try_from(ordinal).ok() != Some(expected_ordinal) {
                return Err(VaultError::InvalidKeyOrCorrupt);
            }
            let mut source_statement = connection.prepare(
                "SELECT s.evidence_id, s.block_id
                 FROM understanding_projection_statement_sources x
                 JOIN understanding_projection_sources s
                   ON s.projection_id = x.projection_id AND s.ordinal = x.source_ordinal
                 WHERE x.projection_id = ?1 AND x.statement_ordinal = ?2
                 ORDER BY x.source_ordinal",
            )?;
            let sources = source_statement
                .query_map(params![projection_id_sql, ordinal], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .map(|(evidence_id, block_id)| decode_evidence_block_ref(evidence_id, block_id))
                .collect::<Result<Vec<_>, _>>()?;
            SourcedStatement::new(text, sources).map_err(|_| VaultError::InvalidKeyOrCorrupt)
        })
        .collect()
}

fn resolve_understanding_source(
    connection: &Connection,
    object_store: &ObjectStore,
    reference: EvidenceBlockRef,
) -> Result<Option<ProjectionSource>, VaultError> {
    let stored = connection
        .query_row(
            "SELECT b.start_byte, b.end_byte, r.canonical_digest,
                    v.source_record_id,
                    CASE WHEN s.origin_kind = 1 THEN s.current_locator
                         ELSE s.source_locator END,
                    a.archived_at, a.object_id
             FROM evidence_blocks b
             JOIN extraction_revisions r ON r.id = b.extraction_revision_id
             JOIN source_record_versions v ON v.evidence_id = b.evidence_id
             JOIN source_records s ON s.id = v.source_record_id
             JOIN archived_evidence a ON a.id = b.evidence_id
             WHERE b.evidence_id = ?1 AND b.id = ?2",
            params![
                to_vault_sql_id(reference.evidence_id())?,
                to_vault_sql_id(reference.block_id().get())?,
            ],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                ))
            },
        )
        .optional()?;
    let Some((start, end, canonical_digest, source_record_id, locator, recorded_at, object_id)) =
        stored
    else {
        return Ok(None);
    };
    let canonical = object_store.read(&object_id)?;
    let actual_digest: [u8; 32] = Sha256::digest(&canonical).into();
    if canonical_digest.as_slice() != actual_digest {
        return Err(VaultError::InvalidKeyOrCorrupt);
    }
    let canonical = std::str::from_utf8(&canonical).map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
    let start = usize::try_from(start).map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
    let end = usize::try_from(end).map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
    let verbatim = canonical
        .get(start..end)
        .ok_or(VaultError::InvalidKeyOrCorrupt)?
        .to_owned();
    Ok(Some(ProjectionSource::new(
        reference,
        verbatim,
        u64::try_from(source_record_id).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
        locator.ok_or(VaultError::InvalidKeyOrCorrupt)?,
        recorded_at,
    )))
}

fn stored_projection(
    projection_id: u64,
    generation: u64,
    status: ProjectionStatus,
    material_digest: &[u8; 32],
) -> Result<StoredProjection, VaultError> {
    Ok(StoredProjection::new(
        ProjectionId::new(projection_id).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
        generation,
        status,
        *material_digest,
    ))
}

const fn encode_projection_trigger_kind(kind: ProjectionTriggerKind) -> i64 {
    match kind {
        ProjectionTriggerKind::PersonDesignated => 0,
        ProjectionTriggerKind::RepeatedRecall => 1,
        ProjectionTriggerKind::ImportantChange => 2,
        ProjectionTriggerKind::CurrentTask => 3,
    }
}

fn decode_projection_trigger(
    kind: i64,
    detail: String,
    recall_count: Option<i64>,
) -> Result<ProjectionTrigger, VaultError> {
    match (kind, recall_count) {
        (0, None) => Ok(ProjectionTrigger::PersonDesignated { reason: detail }),
        (1, Some(count)) => Ok(ProjectionTrigger::RepeatedRecall {
            query: detail,
            recall_count: u32::try_from(count).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
        }),
        (2, None) => Ok(ProjectionTrigger::ImportantChange {
            description: detail,
        }),
        (3, None) => Ok(ProjectionTrigger::CurrentTask { task: detail }),
        _ => Err(VaultError::InvalidKeyOrCorrupt),
    }
}

const fn encode_projection_kind(kind: ProjectionKind) -> i64 {
    match kind {
        ProjectionKind::EventChain => 0,
        ProjectionKind::PersonTopicRelations => 1,
        ProjectionKind::PhaseSummary => 2,
    }
}

const fn decode_projection_kind(value: i64) -> Result<ProjectionKind, VaultError> {
    match value {
        0 => Ok(ProjectionKind::EventChain),
        1 => Ok(ProjectionKind::PersonTopicRelations),
        2 => Ok(ProjectionKind::PhaseSummary),
        _ => Err(VaultError::InvalidKeyOrCorrupt),
    }
}

const fn encode_projection_status(status: ProjectionStatus) -> i64 {
    match status {
        ProjectionStatus::Active => 0,
        ProjectionStatus::Invalidated => 1,
    }
}

const fn decode_projection_status(value: i64) -> Result<ProjectionStatus, VaultError> {
    match value {
        0 => Ok(ProjectionStatus::Active),
        1 => Ok(ProjectionStatus::Invalidated),
        _ => Err(VaultError::InvalidKeyOrCorrupt),
    }
}

fn reconcile_understanding_projections(
    transaction: &rusqlite::Transaction<'_>,
    object_store: &ObjectStore,
    batch: &LineageBatch,
) -> Result<(), VaultError> {
    let mut affected = HashMap::<u64, Vec<&BlockLineage>>::new();
    for lineage in batch.lineages() {
        let mut statement = transaction.prepare(
            "SELECT projection_id FROM understanding_projection_sources s
             JOIN understanding_projections p ON p.id = s.projection_id
             WHERE s.evidence_id = ?1 AND s.block_id = ?2 AND p.status = 0
             ORDER BY projection_id",
        )?;
        for projection_id in statement
            .query_map(
                params![
                    to_vault_sql_id(lineage.from_ref().evidence_id())?,
                    to_vault_sql_id(lineage.from_ref().block_id().get())?,
                ],
                |row| row.get::<_, i64>(0),
            )?
            .collect::<Result<Vec<_>, _>>()?
        {
            affected
                .entry(u64::try_from(projection_id).map_err(|_| VaultError::InvalidKeyOrCorrupt)?)
                .or_default()
                .push(lineage);
        }
    }

    let projection_ids = affected.keys().copied().collect::<BTreeSet<_>>();
    for projection_id in projection_ids {
        let lineages = affected
            .get(&projection_id)
            .ok_or(VaultError::InvalidKeyOrCorrupt)?;
        reconcile_one_understanding_projection(
            transaction,
            object_store,
            projection_id,
            lineages,
            batch.decided_at_millis(),
        )?;
    }
    Ok(())
}

fn reconcile_one_understanding_projection(
    transaction: &rusqlite::Transaction<'_>,
    object_store: &ObjectStore,
    projection_id: u64,
    lineages: &[&BlockLineage],
    decided_at_millis: i64,
) -> Result<(), VaultError> {
    let first_unsafe = lineages.iter().find(|lineage| {
        !matches!(
            lineage.status(),
            BlockLineageStatus::Unchanged | BlockLineageStatus::Moved
        )
    });
    if let Some(lineage) = first_unsafe {
        transaction.execute(
            "UPDATE understanding_projections
             SET generation = generation + 1, status = 1 WHERE id = ?1 AND status = 0",
            [to_vault_sql_id(projection_id)?],
        )?;
        transaction.execute(
            "DELETE FROM understanding_projection_artifacts WHERE projection_id = ?1",
            [to_vault_sql_id(projection_id)?],
        )?;
        return insert_understanding_event(
            transaction,
            projection_id,
            ProjectionStatus::Invalidated,
            Some(lineage.from_ref()),
            decided_at_millis,
        );
    }

    for lineage in lineages {
        let to_ref = lineage.to_ref().ok_or(VaultError::InvalidKeyOrCorrupt)?;
        let changed = transaction.execute(
            "UPDATE understanding_projection_sources
             SET evidence_id = ?1, block_id = ?2
             WHERE projection_id = ?3 AND evidence_id = ?4 AND block_id = ?5",
            params![
                to_vault_sql_id(to_ref.evidence_id())?,
                to_vault_sql_id(to_ref.block_id().get())?,
                to_vault_sql_id(projection_id)?,
                to_vault_sql_id(lineage.from_ref().evidence_id())?,
                to_vault_sql_id(lineage.from_ref().block_id().get())?,
            ],
        )?;
        if changed != 1 {
            return Err(VaultError::InvalidKeyOrCorrupt);
        }
    }
    let id = ProjectionId::new(projection_id).map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
    let recipe = load_understanding_projection(transaction, id)?
        .ok_or(VaultError::InvalidKeyOrCorrupt)?
        .recipe()
        .clone();
    let mut sources = Vec::with_capacity(recipe.sources().len());
    for reference in recipe.sources() {
        sources.push(
            resolve_understanding_source(transaction, object_store, reference)?
                .ok_or(VaultError::InvalidKeyOrCorrupt)?,
        );
    }
    let build = ProjectionBuild::from_resolved_sources(recipe, sources)
        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
    transaction.execute(
        "UPDATE understanding_projections
         SET generation = generation + 1, status = 0, material_digest = ?1
         WHERE id = ?2 AND status = 0",
        params![
            build.material_digest().as_slice(),
            to_vault_sql_id(projection_id)?
        ],
    )?;
    replace_understanding_artifact(transaction, projection_id, &build)?;
    insert_understanding_event(
        transaction,
        projection_id,
        ProjectionStatus::Active,
        lineages.first().map(|lineage| lineage.from_ref()),
        decided_at_millis,
    )
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
        hash_i64(&mut hasher, encode_claim_status(claim.status()));
        hash_optional_i64(
            &mut hasher,
            claim
                .supersedes()
                .map(|id| i64::try_from(id.get()).unwrap_or(i64::MAX)),
        );
        hash_optional_i64(
            &mut hasher,
            claim
                .superseded_by()
                .map(|id| i64::try_from(id.get()).unwrap_or(i64::MAX)),
        );
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
    clear_retrieval_index_counted(transaction).map(|_| ())
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
              recorded_at, statement_digest, claim_status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                to_vault_sql_id(claim.id().get())?,
                start,
                end,
                i64::from(unknown),
                claim.recorded_at().as_millis(),
                statement_digest.as_slice(),
                encode_claim_status(claim.status()),
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
                recorded_at, statement_digest, claim_status
         FROM retrieval_claim_documents ORDER BY claim_id",
    )?;
    for (claim_id, start, end, unknown, recorded_at, digest, status) in statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, Vec<u8>>(5)?,
                row.get::<_, i64>(6)?,
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
        hash_i64(hasher, status);
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

fn recall_long_term_memory_candidates(
    connection: &Connection,
    query: &RetrievalQuery,
) -> Result<Vec<RecallHit>, VaultError> {
    let mut terms = BTreeSet::new();
    if let Some(text) = query.text() {
        terms.extend(search_terms(text));
    }
    for entity in query.entities() {
        terms.extend(search_terms(entity));
    }
    let start = query.time().map(eam_retrieval::TimeRange::start_millis);
    let end = query.time().map(eam_retrieval::TimeRange::end_millis);
    let mut claim_ids = BTreeSet::new();
    if terms.is_empty() {
        let mut statement = connection.prepare(
            "SELECT s.claim_id
             FROM long_term_memory_versions v
             JOIN long_term_memory_sources s
               ON s.memory_id = v.memory_id AND s.version = v.version
             WHERE v.version = (
                       SELECT MAX(latest.version) FROM long_term_memory_versions latest
                       WHERE latest.memory_id = v.memory_id
                   )
               AND (SELECT e.status FROM long_term_memory_state_events e
                    WHERE e.memory_id = v.memory_id AND e.version = v.version
                    ORDER BY e.ordinal DESC LIMIT 1) IN (0, 1, 2, 6, 7)
               AND (?1 IS NULL OR (
                    v.applicable_start IS NOT NULL
                    AND v.applicable_start <= ?2
                    AND (v.applicable_kind = 1
                         OR COALESCE(v.applicable_end, v.applicable_start) >= ?1)
               ))
             ORDER BY v.memory_id, s.ordinal",
        )?;
        for claim_id in statement
            .query_map(params![start, end], |row| row.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?
        {
            claim_ids.insert(u64::try_from(claim_id).map_err(|_| VaultError::InvalidKeyOrCorrupt)?);
            if claim_ids.len() == MAX_LONG_TERM_MEMORY_CANDIDATES {
                break;
            }
        }
    } else {
        for term in terms {
            let mut statement = connection.prepare(
                "SELECT s.claim_id
                 FROM long_term_memory_terms t
                 JOIN long_term_memory_versions v
                   ON v.memory_id = t.memory_id AND v.version = t.version
                 JOIN long_term_memory_sources s
                   ON s.memory_id = v.memory_id AND s.version = v.version
                 WHERE t.term = ?1
                   AND v.version = (
                       SELECT MAX(latest.version) FROM long_term_memory_versions latest
                       WHERE latest.memory_id = v.memory_id
                   )
                   AND (SELECT e.status FROM long_term_memory_state_events e
                        WHERE e.memory_id = v.memory_id AND e.version = v.version
                        ORDER BY e.ordinal DESC LIMIT 1) IN (0, 1, 2, 6, 7)
                   AND (?2 IS NULL OR (
                        v.applicable_start IS NOT NULL
                        AND v.applicable_start <= ?3
                        AND (v.applicable_kind = 1
                             OR COALESCE(v.applicable_end, v.applicable_start) >= ?2)
                   ))
                 ORDER BY v.memory_id, s.ordinal",
            )?;
            for claim_id in statement
                .query_map(params![term, start, end], |row| row.get::<_, i64>(0))?
                .collect::<Result<Vec<_>, _>>()?
            {
                claim_ids
                    .insert(u64::try_from(claim_id).map_err(|_| VaultError::InvalidKeyOrCorrupt)?);
                if claim_ids.len() == MAX_LONG_TERM_MEMORY_CANDIDATES {
                    break;
                }
            }
            if claim_ids.len() == MAX_LONG_TERM_MEMORY_CANDIDATES {
                break;
            }
        }
    }
    Ok(claim_ids
        .into_iter()
        .map(|claim_id| {
            RecallHit::new(
                CandidateRef::ledger(ClaimId::from_raw(claim_id)),
                RecallChannels::long_term_memory(),
                0,
            )
        })
        .collect())
}

fn recall_disputed_memories(
    connection: &Connection,
    query: &RetrievalQuery,
) -> Result<Vec<DisputedMemoryRecall>, VaultError> {
    let mut terms = BTreeSet::new();
    if let Some(text) = query.text() {
        terms.extend(search_terms(text));
    }
    for entity in query.entities() {
        terms.extend(search_terms(entity));
    }
    if terms.is_empty() {
        return Ok(Vec::new());
    }
    let start = query.time().map(eam_retrieval::TimeRange::start_millis);
    let end = query.time().map(eam_retrieval::TimeRange::end_millis);
    let mut dispute_ids = BTreeSet::new();
    for term in terms {
        let mut statement = connection.prepare(
            "SELECT DISTINCT d.id
             FROM memory_disputes d
             JOIN long_term_memory_versions v
               ON v.memory_id = d.memory_id AND v.version = d.memory_version
             LEFT JOIN memory_dispute_terms dt ON dt.dispute_id = d.id
             LEFT JOIN long_term_memory_terms mt
               ON mt.memory_id = v.memory_id AND mt.version = v.version
             WHERE (dt.term = ?1 OR mt.term = ?1)
               AND d.outcome IN (0, 3)
               AND d.id = (
                   SELECT MAX(latest_d.id) FROM memory_disputes latest_d
                   WHERE latest_d.memory_id = d.memory_id
                     AND latest_d.outcome IN (0, 3)
               )
               AND v.version = (
                   SELECT MAX(latest_v.version) FROM long_term_memory_versions latest_v
                   WHERE latest_v.memory_id = v.memory_id
               )
               AND (SELECT e.status FROM long_term_memory_state_events e
                    WHERE e.memory_id = v.memory_id AND e.version = v.version
                    ORDER BY e.ordinal DESC LIMIT 1) = 4
               AND (?2 IS NULL OR (
                    v.applicable_start IS NOT NULL
                    AND v.applicable_start <= ?3
                    AND (v.applicable_kind = 1
                         OR COALESCE(v.applicable_end, v.applicable_start) >= ?2)
               ))
             ORDER BY d.id",
        )?;
        for id in statement
            .query_map(params![term, start, end], |row| row.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?
        {
            dispute_ids.insert(u64::try_from(id).map_err(|_| VaultError::InvalidKeyOrCorrupt)?);
            if dispute_ids.len() == MAX_LONG_TERM_MEMORY_CANDIDATES {
                break;
            }
        }
        if dispute_ids.len() == MAX_LONG_TERM_MEMORY_CANDIDATES {
            break;
        }
    }

    dispute_ids
        .into_iter()
        .map(|id| load_disputed_memory_recall(connection, id))
        .collect()
}

fn load_disputed_memory_recall(
    connection: &Connection,
    id: u64,
) -> Result<DisputedMemoryRecall, VaultError> {
    let dispute_id = MemoryDisputeId::new(id).ok_or(VaultError::InvalidKeyOrCorrupt)?;
    let dispute = load_memory_dispute(connection, dispute_id)
        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?
        .ok_or(VaultError::InvalidKeyOrCorrupt)?;
    let memory = load_memory_version(connection, dispute.memory_id(), dispute.memory_version())
        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
    if memory.status() != MemoryStatus::Disputed {
        return Err(VaultError::InvalidKeyOrCorrupt);
    }
    let counterpart_sources = memory
        .source_claim_ids()
        .iter()
        .map(|id| {
            load_claim(connection, *id)
                .map_err(|_| VaultError::InvalidKeyOrCorrupt)?
                .ok_or(VaultError::InvalidKeyOrCorrupt)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let (review_rationale, review_evidence, state) = match dispute.outcome() {
        MemoryDisputeOutcome::Open => (None, Vec::new(), DisputeState::Open),
        MemoryDisputeOutcome::Maintained => {
            let review = dispute.review().ok_or(VaultError::InvalidKeyOrCorrupt)?;
            (
                Some(review.rationale().to_owned()),
                review.evidence().to_vec(),
                DisputeState::Maintained,
            )
        }
        MemoryDisputeOutcome::Retracted
        | MemoryDisputeOutcome::Revised
        | MemoryDisputeOutcome::Weakened => {
            return Err(VaultError::InvalidKeyOrCorrupt);
        }
    };
    Ok(DisputedMemoryRecall::new(
        dispute.id().get(),
        memory.id().get(),
        memory.version(),
        memory.statement().to_owned(),
        counterpart_sources,
        dispute.reason().to_owned(),
        dispute.counter_evidence().to_vec(),
        review_rationale,
        review_evidence,
        state,
    ))
}

fn recall_understanding_candidates(
    connection: &Connection,
    query: &RetrievalQuery,
) -> Result<Vec<RecallHit>, VaultError> {
    let mut terms = BTreeSet::new();
    if let Some(text) = query.text() {
        terms.extend(search_terms(text));
    }
    for entity in query.entities() {
        terms.extend(search_terms(entity));
    }
    let mut references = BTreeSet::new();
    for term in terms {
        let mut statement = connection.prepare(
            "SELECT s.evidence_id, s.block_id
             FROM understanding_projection_terms t
             JOIN understanding_projection_artifacts a
               ON a.projection_id = t.projection_id
             JOIN understanding_projections p
               ON p.id = a.projection_id
              AND p.status = 0
              AND p.contract_version = a.contract_version
              AND p.material_digest = a.material_digest
             JOIN understanding_projection_sources s
               ON s.projection_id = p.id
             JOIN retrieval_block_documents d
               ON d.evidence_id = s.evidence_id AND d.block_id = s.block_id
             WHERE t.term = ?1
               AND (?2 IS NULL OR d.recorded_at BETWEEN ?2 AND ?3)
             ORDER BY p.id, s.ordinal",
        )?;
        let start = query.time().map(eam_retrieval::TimeRange::start_millis);
        let end = query.time().map(eam_retrieval::TimeRange::end_millis);
        for (evidence_id, block_id) in statement
            .query_map(params![term, start, end], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?
        {
            references.insert(CandidateRef::Evidence {
                evidence_id: u64::try_from(evidence_id)
                    .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                block_id: u64::try_from(block_id).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
            });
            if references.len() == MAX_UNDERSTANDING_CANDIDATES {
                break;
            }
        }
        if references.len() == MAX_UNDERSTANDING_CANDIDATES {
            break;
        }
    }
    Ok(references
        .into_iter()
        .map(|reference| RecallHit::new(reference, RecallChannels::understanding(), 0))
        .collect())
}

fn recall_retrieval_candidates(
    connection: &Connection,
    query: &RetrievalQuery,
) -> Result<Vec<RecallHit>, VaultError> {
    let mut hits = Vec::new();
    append_lexical_hits(connection, query.text(), query.source_scope(), &mut hits)?;
    append_vector_hits(connection, query.text(), &mut hits)?;
    append_temporal_hits(connection, query.time(), query.source_scope(), &mut hits)?;
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
    scope: SourceScope,
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
            let sql = match scope {
                SourceScope::Current => {
                    "SELECT t.claim_id FROM retrieval_claim_terms t
                     JOIN retrieval_claim_documents d ON d.claim_id = t.claim_id
                     WHERE t.term = ?1 AND d.claim_status = 0 ORDER BY t.claim_id"
                }
                SourceScope::Historical => {
                    "SELECT claim_id FROM retrieval_claim_terms
                     WHERE term = ?1 ORDER BY claim_id"
                }
            };
            let mut statement = connection.prepare(sql)?;
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
    scope: SourceScope,
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
        let claim_sql = match scope {
            SourceScope::Current => {
                "SELECT claim_id FROM retrieval_claim_documents
                 WHERE claim_status = 0
                   AND applicable_unknown = 0
                   AND applicable_start <= ?2
                   AND (applicable_end IS NULL OR applicable_end >= ?1)
                 ORDER BY claim_id"
            }
            SourceScope::Historical => {
                "SELECT claim_id FROM retrieval_claim_documents
                 WHERE applicable_unknown = 0
                   AND applicable_start <= ?2
                   AND (applicable_end IS NULL OR applicable_end >= ?1)
                 ORDER BY claim_id"
            }
        };
        let mut statement = connection.prepare(claim_sql)?;
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
        CandidateRef::Ledger { claim_id } => resolve_retrieval_claim(repository, claim_id, scope),
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
                    ),
                    s.origin_kind, roots.lifecycle_state
             FROM source_record_versions v
             JOIN source_records s ON s.id = v.source_record_id
             LEFT JOIN source_roots roots ON roots.id = s.root_id
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
                    row.get::<_, i64>(5)?,
                    row.get::<_, Option<i64>>(6)?,
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
    let lifecycle = match (source.5, source.6) {
        (0, None) => None,
        (1, Some(value)) => Some(decode_source_root_lifecycle(value)?),
        _ => return Err(VaultError::InvalidKeyOrCorrupt),
    };
    let lifecycle_eligible = match lifecycle {
        None | Some(SourceRootLifecycle::Active) => true,
        Some(SourceRootLifecycle::Detached) => scope == SourceScope::Historical,
        Some(SourceRootLifecycle::Staged) => false,
    };
    if !lifecycle_eligible {
        return Ok(None);
    }
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
    scope: SourceScope,
) -> Result<Option<AuthoritativeCandidate>, VaultError> {
    let claim = load_claim(repository.connection(), ClaimId::from_raw(claim_id))
        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?
        .ok_or(VaultError::InvalidKeyOrCorrupt)?;
    if scope == SourceScope::Current && claim.status() != ClaimStatus::Current {
        return Ok(None);
    }
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
    let (indexed_digest, indexed_status) = repository
        .connection()
        .query_row(
            "SELECT statement_digest, claim_status
             FROM retrieval_claim_documents WHERE claim_id = ?1",
            [to_vault_sql_id(claim_id)?],
            |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()?
        .ok_or(VaultError::InvalidKeyOrCorrupt)?;
    let authority_digest: [u8; 32] = Sha256::digest(claim.statement().as_bytes()).into();
    if indexed_digest.as_slice() != authority_digest
        || indexed_status != encode_claim_status(claim.status())
    {
        return Err(VaultError::InvalidKeyOrCorrupt);
    }
    Ok(Some(AuthoritativeCandidate::Ledger(claim)))
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
            "SELECT root_locator, lifecycle_state, availability, first_seen_at,
                    last_reconciled_at
             FROM source_roots WHERE id = ?1",
            [root_id_sql],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                ))
            },
        )
        .optional()?
        .ok_or(VaultError::InvalidKeyOrCorrupt)?;
    let root = SourceRoot::new(
        root_id,
        root.0,
        decode_source_root_lifecycle(root.1)?,
        decode_source_availability(root.2)?,
        root.3,
        root.4,
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

fn load_active_source_root_snapshot(
    connection: &Connection,
) -> Result<Option<SourceRootSnapshot>, VaultError> {
    let root_id = connection
        .query_row(
            "SELECT id FROM source_roots WHERE lifecycle_state = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    root_id
        .map(|root_id| {
            let root_id = u64::try_from(root_id).map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
            load_source_root_snapshot(connection, root_id)
        })
        .transpose()
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

fn insert_source_root_lifecycle_event(
    transaction: &rusqlite::Transaction<'_>,
    root_id: u64,
    lifecycle: SourceRootLifecycle,
    occurred_at_millis: i64,
) -> Result<(), VaultError> {
    let ordinal: i64 = transaction.query_row(
        "SELECT COALESCE(MAX(ordinal), -1) + 1
         FROM source_root_lifecycle_events WHERE root_id = ?1",
        [to_vault_sql_id(root_id)?],
        |row| row.get(0),
    )?;
    transaction.execute(
        "INSERT INTO source_root_lifecycle_events
         (root_id, ordinal, lifecycle_state, occurred_at) VALUES (?1, ?2, ?3, ?4)",
        params![
            to_vault_sql_id(root_id)?,
            ordinal,
            encode_source_root_lifecycle(lifecycle),
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

const fn encode_source_root_lifecycle(value: SourceRootLifecycle) -> i64 {
    match value {
        SourceRootLifecycle::Staged => 0,
        SourceRootLifecycle::Active => 1,
        SourceRootLifecycle::Detached => 2,
    }
}

const fn decode_source_root_lifecycle(value: i64) -> Result<SourceRootLifecycle, VaultError> {
    match value {
        0 => Ok(SourceRootLifecycle::Staged),
        1 => Ok(SourceRootLifecycle::Active),
        2 => Ok(SourceRootLifecycle::Detached),
        _ => Err(VaultError::InvalidKeyOrCorrupt),
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

#[derive(Clone, Copy, Debug, Default)]
struct ForgetClosureCounts {
    authority: usize,
    derived: usize,
    object_references: usize,
}

fn forget_target_code(target: ForgetTarget) -> (i64, u64) {
    match target {
        ForgetTarget::ConversationEvidence(id) => (FORGET_TARGET_CONVERSATION_EVIDENCE, id.get()),
        ForgetTarget::ArchivedEvidence(id) => (FORGET_TARGET_ARCHIVED_EVIDENCE, id),
    }
}

fn decode_forget_target(kind: i64, id: i64) -> Result<ForgetTarget, VaultError> {
    let id = u64::try_from(id).map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
    if id == 0 {
        return Err(VaultError::InvalidKeyOrCorrupt);
    }
    match kind {
        FORGET_TARGET_CONVERSATION_EVIDENCE => {
            Ok(ForgetTarget::ConversationEvidence(EvidenceId::from_raw(id)))
        }
        FORGET_TARGET_ARCHIVED_EVIDENCE => Ok(ForgetTarget::ArchivedEvidence(id)),
        _ => Err(VaultError::InvalidKeyOrCorrupt),
    }
}

fn load_deletion_intents(connection: &Connection) -> Result<Vec<ForgetReceipt>, VaultError> {
    let mut statement = connection.prepare(
        "SELECT id, target_kind, target_id, removed_authority_records,
                removed_derived_records, released_object_references
         FROM deletion_intents ORDER BY id",
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
        .map(
            |(id, kind, target_id, authority, derived, object_references)| {
                Ok(ForgetReceipt::new(
                    u64::try_from(id).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                    decode_forget_target(kind, target_id)?,
                    usize::try_from(authority).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                    usize::try_from(derived).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                    usize::try_from(object_references)
                        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                ))
            },
        )
        .collect()
}

fn load_deletion_intent(
    connection: &Connection,
    target: ForgetTarget,
) -> Result<Option<ForgetReceipt>, VaultError> {
    let (kind, target_id) = forget_target_code(target);
    connection
        .query_row(
            "SELECT id, removed_authority_records, removed_derived_records,
                    released_object_references
             FROM deletion_intents WHERE target_kind = ?1 AND target_id = ?2",
            params![kind, to_vault_sql_id(target_id)?],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .optional()?
        .map(|(id, authority, derived, object_references)| {
            Ok(ForgetReceipt::new(
                u64::try_from(id).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                target,
                usize::try_from(authority).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                usize::try_from(derived).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
                usize::try_from(object_references).map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
            ))
        })
        .transpose()
}

fn insert_deletion_intent(
    transaction: &rusqlite::Transaction<'_>,
    receipt: ForgetReceipt,
    requested_at: Timestamp,
) -> Result<(), VaultError> {
    let (kind, target_id) = forget_target_code(receipt.target());
    transaction.execute(
        "INSERT INTO deletion_intents
         (id, target_kind, target_id, requested_at, removed_authority_records,
          removed_derived_records, released_object_references)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            to_vault_sql_id(receipt.deletion_intent_id())?,
            kind,
            to_vault_sql_id(target_id)?,
            requested_at.as_millis(),
            i64::try_from(receipt.removed_authority_records())
                .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
            i64::try_from(receipt.removed_derived_records())
                .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
            i64::try_from(receipt.released_object_references())
                .map_err(|_| VaultError::InvalidKeyOrCorrupt)?,
        ],
    )?;
    Ok(())
}

fn forget_target_exists(connection: &Connection, target: ForgetTarget) -> Result<bool, VaultError> {
    let (table, id) = match target {
        ForgetTarget::ConversationEvidence(id) => ("conversation_evidence", id.get()),
        ForgetTarget::ArchivedEvidence(id) => ("archived_evidence", id),
    };
    let query = format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE id = ?1)");
    connection
        .query_row(&query, [to_vault_sql_id(id)?], |row| row.get(0))
        .map_err(VaultError::from)
}

struct ConversationForgetPlan {
    evidence_id_sql: i64,
    claim_ids: BTreeSet<u64>,
    shared_agreement_candidate_ids: BTreeSet<u64>,
    memory_ids: BTreeSet<u64>,
    initial_impacted: bool,
    identity_from: Option<i64>,
    bundle_from: Option<i64>,
}

fn delete_conversation_evidence_closure(
    transaction: &rusqlite::Transaction<'_>,
    evidence_id: EvidenceId,
) -> Result<ForgetClosureCounts, VaultError> {
    let plan = plan_conversation_forget(transaction, evidence_id)?;
    let mut counts = ForgetClosureCounts::default();
    counts.derived += clear_retrieval_index_counted(transaction)?;
    delete_bundle_and_identity_closure(transaction, &plan, &mut counts)?;
    delete_memory_closure(transaction, &plan, &mut counts)?;
    delete_reflection_invitation_closure(transaction, &plan, &mut counts)?;
    delete_conversation_claim_closure(transaction, &plan, &mut counts)?;
    counts.authority += transaction.execute(
        "DELETE FROM conversation_evidence WHERE id = ?1",
        [plan.evidence_id_sql],
    )?;
    Ok(counts)
}

fn plan_conversation_forget(
    transaction: &rusqlite::Transaction<'_>,
    evidence_id: EvidenceId,
) -> Result<ConversationForgetPlan, VaultError> {
    let evidence_id_sql = to_vault_sql_id(evidence_id.get())?;
    let mut claim_ids = query_u64_ids(
        transaction,
        "WITH RECURSIVE affected(id) AS (
             SELECT claim_id FROM claim_support WHERE evidence_id = ?1
             UNION
             SELECT child.id FROM claims child
             JOIN affected parent ON child.supersedes_claim_id = parent.id
             UNION
             SELECT parent.supersedes_claim_id FROM claims parent
             JOIN affected child ON parent.id = child.id
             WHERE parent.supersedes_claim_id IS NOT NULL
         ) SELECT id FROM affected ORDER BY id",
        evidence_id_sql,
    )?;
    let shared_agreement_candidate_ids = query_u64_ids(
        transaction,
        &format!(
            "WITH RECURSIVE affected(candidate_id) AS (
               SELECT candidate_id FROM shared_agreement_candidate_support
               WHERE evidence_id = ?1
               UNION SELECT id FROM shared_agreement_candidates
               WHERE {}
               UNION SELECT candidate_id
               FROM shared_agreement_candidate_supersessions
               WHERE {}
               UNION SELECT successor.id FROM shared_agreement_candidates successor
               JOIN affected predecessor
                 ON successor.predecessor_candidate_id = predecessor.candidate_id
               UNION SELECT edge.candidate_id
               FROM shared_agreement_candidate_supersessions edge
               JOIN shared_agreement_candidates original
                 ON original.confirmed_claim_id = edge.superseded_agreement_claim_id
               JOIN affected predecessor ON predecessor.candidate_id = original.id
             ) SELECT candidate_id FROM affected ORDER BY candidate_id",
            id_predicate("confirmed_claim_id", &claim_ids),
            id_predicate("superseded_agreement_claim_id", &claim_ids)
        ),
        evidence_id_sql,
    )?;
    claim_ids.extend(query_u64_ids_without_param(
        transaction,
        &format!(
            "SELECT confirmed_claim_id FROM shared_agreement_candidates
             WHERE {} AND confirmed_claim_id IS NOT NULL",
            id_predicate("id", &shared_agreement_candidate_ids)
        ),
    )?);
    let claim_predicate = id_predicate("claim_id", &claim_ids);
    let initial_impacted = transaction.query_row(
        &format!(
            "SELECT EXISTS(SELECT 1 FROM initial_self_introduction
             WHERE evidence_id = ?1 OR {claim_predicate})"
        ),
        [evidence_id_sql],
        |row| row.get(0),
    )?;
    let identity_from = earliest_identity_version(transaction, evidence_id_sql, initial_impacted)?;
    let memory_ids = query_u64_ids(
        transaction,
        &format!(
            "SELECT memory_id FROM long_term_memory_sources WHERE {claim_predicate}
             UNION SELECT d.memory_id FROM memory_disputes d
               JOIN memory_dispute_counter_evidence e ON e.dispute_id = d.id
               WHERE e.evidence_id = ?1
             UNION SELECT d.memory_id FROM memory_disputes d
               JOIN memory_dispute_review_evidence e ON e.dispute_id = d.id
               WHERE e.evidence_id = ?1
             UNION SELECT memory_id FROM long_term_memory_counterexample_reviews
               WHERE evidence_id = ?1
             UNION SELECT memory_id FROM pattern_maturity_evidence
               WHERE evidence_id = ?1 ORDER BY memory_id"
        ),
        evidence_id_sql,
    )?;
    let bundle_from =
        earliest_bundle_version(transaction, initial_impacted, identity_from, &claim_ids)?;
    Ok(ConversationForgetPlan {
        evidence_id_sql,
        claim_ids,
        shared_agreement_candidate_ids,
        memory_ids,
        initial_impacted,
        identity_from,
        bundle_from,
    })
}

fn earliest_identity_version(
    transaction: &rusqlite::Transaction<'_>,
    evidence_id: i64,
    initial_impacted: bool,
) -> Result<Option<i64>, VaultError> {
    let (query, parameter) = if initial_impacted {
        ("SELECT MIN(version) FROM identity_state_versions", None)
    } else {
        (
            "SELECT MIN(identity_version) FROM identity_state_evidence WHERE evidence_id = ?1",
            Some(evidence_id),
        )
    };
    match parameter {
        Some(value) => transaction.query_row(query, [value], |row| row.get(0)),
        None => transaction.query_row(query, [], |row| row.get(0)),
    }
    .map_err(VaultError::from)
}

fn earliest_bundle_version(
    transaction: &rusqlite::Transaction<'_>,
    initial_impacted: bool,
    identity_from: Option<i64>,
    claim_ids: &BTreeSet<u64>,
) -> Result<Option<i64>, VaultError> {
    let mut earliest = if initial_impacted {
        transaction.query_row("SELECT MIN(version) FROM self_bundle_versions", [], |row| {
            row.get::<_, Option<i64>>(0)
        })?
    } else {
        None
    };
    if let Some(identity_from) = identity_from {
        let candidate = transaction.query_row(
            "SELECT MIN(version) FROM self_bundle_versions WHERE identity_state_version >= ?1",
            [identity_from],
            |row| row.get(0),
        )?;
        earliest = min_option(earliest, candidate);
    }
    if !claim_ids.is_empty() {
        let candidate = transaction.query_row(
            &format!(
                "SELECT MIN(bundle_version) FROM self_bundle_beliefs WHERE {}",
                id_predicate("claim_id", claim_ids)
            ),
            [],
            |row| row.get(0),
        )?;
        earliest = min_option(earliest, candidate);
    }
    Ok(earliest)
}

fn delete_bundle_and_identity_closure(
    transaction: &rusqlite::Transaction<'_>,
    plan: &ConversationForgetPlan,
    counts: &mut ForgetClosureCounts,
) -> Result<(), VaultError> {
    if let Some(from) = plan.bundle_from {
        for table in [
            "self_bundle_experiences",
            "self_bundle_beliefs",
            "self_bundle_pending_intentions",
        ] {
            counts.derived += transaction.execute(
                &format!("DELETE FROM {table} WHERE bundle_version >= ?1"),
                [from],
            )?;
        }
        counts.derived +=
            delete_versions_descending(transaction, "self_bundle_versions", "version", from)?;
    }
    if plan.initial_impacted {
        counts.authority += transaction.execute("DELETE FROM initial_self_introduction", [])?;
    }
    if let Some(from) = plan.identity_from {
        counts.derived += transaction.execute(
            "DELETE FROM identity_state_evidence WHERE identity_version >= ?1",
            [from],
        )?;
        counts.derived +=
            delete_versions_descending(transaction, "identity_state_versions", "version", from)?;
    }
    Ok(())
}

fn delete_memory_closure(
    transaction: &rusqlite::Transaction<'_>,
    plan: &ConversationForgetPlan,
    counts: &mut ForgetClosureCounts,
) -> Result<(), VaultError> {
    let memory_predicate = id_predicate("memory_id", &plan.memory_ids);
    counts.derived += transaction.execute(
        &format!(
            "DELETE FROM claim_correction_memory_work_items WHERE {} OR {memory_predicate}",
            id_predicate("correction_claim_id", &plan.claim_ids)
        ),
        [],
    )?;
    if plan.memory_ids.is_empty() {
        return Ok(());
    }
    let dispute_ids = query_u64_ids_without_param(
        transaction,
        &format!("SELECT id FROM memory_disputes WHERE {memory_predicate}"),
    )?;
    for table in [
        "memory_dispute_terms",
        "memory_dispute_counter_evidence",
        "memory_dispute_review_evidence",
    ] {
        counts.derived += transaction.execute(
            &format!(
                "DELETE FROM {table} WHERE {}",
                id_predicate("dispute_id", &dispute_ids)
            ),
            [],
        )?;
    }
    counts.derived += transaction.execute(
        &format!("DELETE FROM memory_disputes WHERE {memory_predicate}"),
        [],
    )?;
    for table in [
        "pattern_maturity_evidence",
        "pattern_maturity_new_support",
        "pattern_maturity_records",
        "long_term_memory_counterexample_reviews",
    ] {
        counts.derived +=
            transaction.execute(&format!("DELETE FROM {table} WHERE {memory_predicate}"), [])?;
    }
    for table in [
        "long_term_memory_terms",
        "long_term_memory_state_events",
        "long_term_memory_sources",
    ] {
        counts.derived +=
            transaction.execute(&format!("DELETE FROM {table} WHERE {memory_predicate}"), [])?;
    }
    counts.derived += delete_memory_versions_descending(transaction, &plan.memory_ids)?;
    counts.derived += transaction.execute(
        &format!(
            "DELETE FROM long_term_memories WHERE {}",
            id_predicate("id", &plan.memory_ids)
        ),
        [],
    )?;
    Ok(())
}

fn delete_reflection_invitation_closure(
    transaction: &rusqlite::Transaction<'_>,
    plan: &ConversationForgetPlan,
    counts: &mut ForgetClosureCounts,
) -> Result<(), VaultError> {
    let invitation_ids = query_u64_ids(
        transaction,
        "SELECT invitation_id FROM reflection_invitation_evidence
         WHERE evidence_id = ?1 ORDER BY invitation_id",
        plan.evidence_id_sql,
    )?;
    if invitation_ids.is_empty() {
        return Ok(());
    }
    let invitation_predicate = id_predicate("invitation_id", &invitation_ids);
    counts.derived += transaction.execute(
        &format!("DELETE FROM reflection_invitation_evidence WHERE {invitation_predicate}"),
        [],
    )?;
    counts.derived += transaction.execute(
        &format!(
            "DELETE FROM reflection_invitations WHERE {}",
            id_predicate("id", &invitation_ids)
        ),
        [],
    )?;
    Ok(())
}

fn delete_conversation_claim_closure(
    transaction: &rusqlite::Transaction<'_>,
    plan: &ConversationForgetPlan,
    counts: &mut ForgetClosureCounts,
) -> Result<(), VaultError> {
    for table in [
        "memory_dispute_counter_evidence",
        "memory_dispute_review_evidence",
    ] {
        counts.derived += transaction.execute(
            &format!("DELETE FROM {table} WHERE evidence_id = ?1"),
            [plan.evidence_id_sql],
        )?;
    }
    let claim_predicate = id_predicate("claim_id", &plan.claim_ids);
    let candidate_predicate = id_predicate("candidate_id", &plan.shared_agreement_candidate_ids);
    counts.derived += transaction.execute(
        &format!(
            "DELETE FROM shared_agreement_candidate_supersessions
             WHERE {candidate_predicate} OR {}",
            id_predicate("superseded_agreement_claim_id", &plan.claim_ids)
        ),
        [],
    )?;
    counts.derived += transaction.execute(
        &format!(
            "DELETE FROM agreement_withdrawals
             WHERE {claim_predicate} OR {}",
            id_predicate("agreement_claim_id", &plan.claim_ids)
        ),
        [],
    )?;
    counts.derived += transaction.execute(
        &format!(
            "DELETE FROM shared_experiences
             WHERE {claim_predicate} OR {candidate_predicate}"
        ),
        [],
    )?;
    if !plan.shared_agreement_candidate_ids.is_empty() {
        counts.derived += transaction.execute(
            &format!("DELETE FROM shared_agreement_candidate_support WHERE {candidate_predicate}"),
            [],
        )?;
        for candidate_id in plan.shared_agreement_candidate_ids.iter().rev() {
            counts.derived += transaction.execute(
                "DELETE FROM shared_agreement_candidates WHERE id = ?1",
                [to_vault_sql_id(*candidate_id)?],
            )?;
        }
    }
    if plan.claim_ids.is_empty() {
        return Ok(());
    }
    counts.derived += transaction.execute(
        &format!("DELETE FROM claim_state_events WHERE {claim_predicate}"),
        [],
    )?;
    counts.authority += transaction.execute(
        &format!("DELETE FROM claim_support WHERE {claim_predicate}"),
        [],
    )?;
    counts.authority += delete_claims_leaf_first(transaction, &plan.claim_ids)?;
    Ok(())
}

struct ArchivedForgetPlan {
    source_record_id: i64,
    evidence_ids: BTreeSet<u64>,
    projection_ids: BTreeSet<u64>,
    batch_ids: BTreeSet<u64>,
}

fn delete_archived_evidence_closure(
    transaction: &rusqlite::Transaction<'_>,
    archive_id: u64,
) -> Result<ForgetClosureCounts, VaultError> {
    let plan = plan_archived_forget(transaction, archive_id)?;
    let mut counts = ForgetClosureCounts {
        object_references: plan.evidence_ids.len(),
        ..ForgetClosureCounts::default()
    };
    counts.derived += clear_retrieval_index_counted(transaction)?;
    delete_archived_derivatives(transaction, &plan, &mut counts)?;
    delete_archived_authority(transaction, &plan, &mut counts)?;
    Ok(counts)
}

fn plan_archived_forget(
    transaction: &rusqlite::Transaction<'_>,
    archive_id: u64,
) -> Result<ArchivedForgetPlan, VaultError> {
    let source_record_id = transaction
        .query_row(
            "SELECT source_record_id FROM source_record_versions WHERE evidence_id = ?1",
            [to_vault_sql_id(archive_id)?],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(VaultError::InvalidKeyOrCorrupt)?;
    let evidence_ids = query_u64_ids(
        transaction,
        "SELECT evidence_id FROM source_record_versions WHERE source_record_id = ?1",
        source_record_id,
    )?;
    let evidence_predicate = id_predicate("evidence_id", &evidence_ids);
    let revision_ids = query_u64_ids_without_param(
        transaction,
        &format!("SELECT id FROM extraction_revisions WHERE {evidence_predicate}"),
    )?;
    let projection_ids = query_u64_ids_without_param(
        transaction,
        &format!(
            "SELECT projection_id FROM understanding_projection_sources WHERE {evidence_predicate}
             UNION SELECT projection_id FROM understanding_projection_events WHERE {}",
            id_predicate("reason_evidence_id", &evidence_ids)
        ),
    )?;
    let batch_ids = query_u64_ids_without_param(
        transaction,
        &format!(
            "SELECT id FROM block_lineage_batches WHERE source_record_id = {source_record_id}
             OR {} OR {}",
            id_predicate("from_revision_id", &revision_ids),
            id_predicate("to_revision_id", &revision_ids)
        ),
    )?;
    Ok(ArchivedForgetPlan {
        source_record_id,
        evidence_ids,
        projection_ids,
        batch_ids,
    })
}

fn delete_archived_derivatives(
    transaction: &rusqlite::Transaction<'_>,
    plan: &ArchivedForgetPlan,
    counts: &mut ForgetClosureCounts,
) -> Result<(), VaultError> {
    if !plan.projection_ids.is_empty() {
        counts.derived += transaction.execute(
            &format!(
                "DELETE FROM understanding_projections WHERE {}",
                id_predicate("id", &plan.projection_ids)
            ),
            [],
        )?;
    }
    if !plan.batch_ids.is_empty() {
        let predicate = id_predicate("batch_id", &plan.batch_ids);
        for table in [
            "incremental_work_items",
            "block_lineage_candidates",
            "block_lineages",
        ] {
            counts.derived +=
                transaction.execute(&format!("DELETE FROM {table} WHERE {predicate}"), [])?;
        }
        counts.authority += transaction.execute(
            &format!(
                "DELETE FROM block_lineage_batches WHERE {}",
                id_predicate("id", &plan.batch_ids)
            ),
            [],
        )?;
    }
    counts.derived += transaction.execute(
        &format!(
            "DELETE FROM obsidian_relation_resolutions WHERE {} OR resolved_source_record_id = {}",
            id_predicate("evidence_id", &plan.evidence_ids),
            plan.source_record_id
        ),
        [],
    )?;
    Ok(())
}

fn delete_archived_authority(
    transaction: &rusqlite::Transaction<'_>,
    plan: &ArchivedForgetPlan,
    counts: &mut ForgetClosureCounts,
) -> Result<(), VaultError> {
    let evidence_predicate = id_predicate("evidence_id", &plan.evidence_ids);
    for table in [
        "obsidian_properties",
        "obsidian_tags",
        "obsidian_aliases",
        "obsidian_relations",
    ] {
        counts.authority += transaction.execute(
            &format!("DELETE FROM {table} WHERE {evidence_predicate}"),
            [],
        )?;
    }
    counts.authority += delete_evidence_blocks_leaf_first(transaction, &plan.evidence_ids)?;
    counts.authority += transaction.execute(
        &format!("DELETE FROM extraction_revisions WHERE {evidence_predicate}"),
        [],
    )?;
    let evidence_set = id_set(&plan.evidence_ids);
    for table in ["markdown_parse_artifacts", "markdown_parse_attempts"] {
        counts.authority += transaction.execute(
            &format!("DELETE FROM {table} WHERE archive_id IN {evidence_set}"),
            [],
        )?;
    }
    counts.authority += transaction.execute(
        &format!("DELETE FROM source_record_versions WHERE {evidence_predicate}"),
        [],
    )?;
    counts.authority += transaction.execute(
        &format!("DELETE FROM archived_evidence WHERE id IN {evidence_set}"),
        [],
    )?;
    counts.authority += transaction.execute(
        &format!(
            "DELETE FROM source_record_state_events WHERE source_record_id = {}",
            plan.source_record_id
        ),
        [],
    )?;
    counts.authority += transaction.execute(
        &format!(
            "DELETE FROM source_records WHERE id = {}",
            plan.source_record_id
        ),
        [],
    )?;
    Ok(())
}

fn clear_retrieval_index_counted(
    transaction: &rusqlite::Transaction<'_>,
) -> Result<usize, VaultError> {
    let mut count = 0;
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
        count += transaction.execute(&format!("DELETE FROM {table}"), [])?;
    }
    Ok(count)
}

fn query_u64_ids(
    connection: &Connection,
    sql: &str,
    parameter: i64,
) -> Result<BTreeSet<u64>, VaultError> {
    let mut statement = connection.prepare(sql)?;
    statement
        .query_map([parameter], |row| row.get::<_, i64>(0))?
        .map(|value| {
            let value = value?;
            u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, value))
        })
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(VaultError::from)
}

fn query_u64_ids_without_param(
    connection: &Connection,
    sql: &str,
) -> Result<BTreeSet<u64>, VaultError> {
    let mut statement = connection.prepare(sql)?;
    statement
        .query_map([], |row| row.get::<_, i64>(0))?
        .map(|value| {
            let value = value?;
            u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, value))
        })
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(VaultError::from)
}

fn id_set(ids: &BTreeSet<u64>) -> String {
    if ids.is_empty() {
        return "(NULL)".to_owned();
    }
    format!(
        "({})",
        ids.iter().map(u64::to_string).collect::<Vec<_>>().join(",")
    )
}

fn id_predicate(column: &str, ids: &BTreeSet<u64>) -> String {
    if ids.is_empty() {
        "0".to_owned()
    } else {
        format!("{column} IN {}", id_set(ids))
    }
}

const fn min_option(left: Option<i64>, right: Option<i64>) -> Option<i64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(if left < right { left } else { right }),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn delete_versions_descending(
    transaction: &rusqlite::Transaction<'_>,
    table: &str,
    column: &str,
    from: i64,
) -> Result<usize, VaultError> {
    let query = format!("SELECT {column} FROM {table} WHERE {column} >= ?1 ORDER BY {column} DESC");
    let ids = transaction
        .prepare(&query)?
        .query_map([from], |row| row.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut deleted = 0;
    for id in ids {
        deleted +=
            transaction.execute(&format!("DELETE FROM {table} WHERE {column} = ?1"), [id])?;
    }
    Ok(deleted)
}

fn delete_memory_versions_descending(
    transaction: &rusqlite::Transaction<'_>,
    memory_ids: &BTreeSet<u64>,
) -> Result<usize, VaultError> {
    let predicate = id_predicate("memory_id", memory_ids);
    let rows = transaction
        .prepare(&format!(
            "SELECT memory_id, version FROM long_term_memory_versions
             WHERE {predicate} ORDER BY memory_id, version DESC"
        ))?
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut deleted = 0;
    for (memory_id, version) in rows {
        deleted += transaction.execute(
            "DELETE FROM long_term_memory_versions WHERE memory_id = ?1 AND version = ?2",
            params![memory_id, version],
        )?;
    }
    Ok(deleted)
}

fn delete_claims_leaf_first(
    transaction: &rusqlite::Transaction<'_>,
    claim_ids: &BTreeSet<u64>,
) -> Result<usize, VaultError> {
    let mut remaining = claim_ids.clone();
    let mut deleted = 0;
    while !remaining.is_empty() {
        let remaining_predicate = id_predicate("id", &remaining);
        let parents = transaction
            .prepare(&format!(
                "SELECT supersedes_claim_id FROM claims
                 WHERE {remaining_predicate} AND supersedes_claim_id IS NOT NULL"
            ))?
            .query_map([], |row| row.get::<_, i64>(0))?
            .map(|value| {
                let value = value?;
                u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, value))
            })
            .collect::<Result<BTreeSet<_>, _>>()?;
        let leaves = remaining
            .iter()
            .copied()
            .filter(|id| !parents.contains(id))
            .collect::<Vec<_>>();
        if leaves.is_empty() {
            return Err(VaultError::InvalidKeyOrCorrupt);
        }
        for id in leaves {
            deleted +=
                transaction.execute("DELETE FROM claims WHERE id = ?1", [to_vault_sql_id(id)?])?;
            remaining.remove(&id);
        }
    }
    Ok(deleted)
}

fn delete_evidence_blocks_leaf_first(
    transaction: &rusqlite::Transaction<'_>,
    evidence_ids: &BTreeSet<u64>,
) -> Result<usize, VaultError> {
    let predicate = id_predicate("evidence_id", evidence_ids);
    let mut deleted = 0;
    loop {
        let changed = transaction.execute(
            &format!(
                "DELETE FROM evidence_blocks
                 WHERE {predicate}
                   AND NOT EXISTS (
                       SELECT 1 FROM evidence_blocks child
                       WHERE child.parent_id = evidence_blocks.id
                         AND child.extraction_revision_id = evidence_blocks.extraction_revision_id
                   )"
            ),
            [],
        )?;
        deleted += changed;
        let remaining: i64 = transaction.query_row(
            &format!("SELECT COUNT(*) FROM evidence_blocks WHERE {predicate}"),
            [],
            |row| row.get(0),
        )?;
        if remaining == 0 {
            return Ok(deleted);
        }
        if changed == 0 {
            return Err(VaultError::InvalidKeyOrCorrupt);
        }
    }
}

fn next_identifier_with_deletion_watermark(
    connection: &Connection,
    table: &str,
    target_kind: i64,
) -> Result<u64, VaultError> {
    let query = format!(
        "SELECT MAX(value) FROM (
             SELECT COALESCE(MAX(id), 0) AS value FROM {table}
             UNION ALL
             SELECT COALESCE(MAX(target_id), 0) AS value
             FROM deletion_intents WHERE target_kind = ?1
         )"
    );
    let maximum: i64 = connection.query_row(&query, [target_kind], |row| row.get(0))?;
    let maximum = u64::try_from(maximum).map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
    maximum
        .checked_add(1)
        .ok_or(VaultError::InvalidKeyOrCorrupt)
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

struct StoredBrowserVisit {
    id: i64,
    host_session_id: i64,
    submission_id: String,
    url: String,
    title: String,
    visited_at_millis: i64,
    dwell_millis: i64,
    content_archive_id: Option<i64>,
    content_captured_at_millis: Option<i64>,
    content_authorized_origin: Option<String>,
    content_status: Option<i64>,
    content_reason: Option<i64>,
}

fn stored_browser_visit_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredBrowserVisit> {
    Ok(StoredBrowserVisit {
        id: row.get(0)?,
        host_session_id: row.get(1)?,
        submission_id: row.get(2)?,
        url: row.get(3)?,
        title: row.get(4)?,
        visited_at_millis: row.get(5)?,
        dwell_millis: row.get(6)?,
        content_archive_id: row.get(7)?,
        content_captured_at_millis: row.get(8)?,
        content_authorized_origin: row.get(9)?,
        content_status: row.get(10)?,
        content_reason: row.get(11)?,
    })
}

fn load_all_browser_visits(
    repository: &VaultRepository,
) -> Result<Vec<BrowserVisit>, RepositoryError> {
    let mut statement = repository
        .connection()
        .prepare(
            "SELECT v.id, v.host_session_id, v.submission_id, v.url, v.title,
                    v.visited_at, v.dwell_millis, v.content_evidence_id,
                    v.content_captured_at, v.content_authorized_origin,
                    a.status, a.unparsed_reason
             FROM browser_visits v
             LEFT JOIN archived_evidence a ON a.id = v.content_evidence_id
             ORDER BY v.id",
        )
        .map_err(repository_error)?;
    let rows = statement
        .query_map([], stored_browser_visit_from_row)
        .map_err(repository_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(repository_error)?;
    drop(statement);
    rows.into_iter()
        .map(|stored| restore_browser_visit(repository, stored))
        .collect()
}

fn load_browser_visit_by_submission(
    repository: &VaultRepository,
    submission_id: &str,
) -> Result<Option<BrowserVisit>, RepositoryError> {
    let stored = repository
        .connection()
        .query_row(
            "SELECT v.id, v.host_session_id, v.submission_id, v.url, v.title,
                    v.visited_at, v.dwell_millis, v.content_evidence_id,
                    v.content_captured_at, v.content_authorized_origin,
                    a.status, a.unparsed_reason
             FROM browser_visits v
             LEFT JOIN archived_evidence a ON a.id = v.content_evidence_id
             WHERE v.submission_id = ?1",
            [submission_id],
            stored_browser_visit_from_row,
        )
        .optional()
        .map_err(repository_error)?;
    stored
        .map(|stored| restore_browser_visit(repository, stored))
        .transpose()
}

fn restore_browser_visit(
    repository: &VaultRepository,
    stored: StoredBrowserVisit,
) -> Result<BrowserVisit, RepositoryError> {
    let id = u64::try_from(stored.id).map_err(repository_error)?;
    let host_session_id = u64::try_from(stored.host_session_id).map_err(repository_error)?;
    if id == 0 || host_session_id == 0 {
        return Err(RepositoryError::new(
            "persisted browser visit identifiers are invalid",
        ));
    }
    let (page_content, content_archive_id) = match (
        stored.content_archive_id,
        stored.content_captured_at_millis,
        stored.content_authorized_origin,
        stored.content_status,
        stored.content_reason,
    ) {
        (None, None, None, None, None) => (None, None),
        (Some(archive_id), Some(captured_at_millis), Some(origin), Some(status), reason) => {
            if decode_archive_status(status, reason).map_err(repository_error)?
                != ArchiveStatus::ArchivedUnparsed(UnparsedReason::UnsupportedFormat)
            {
                return Err(RepositoryError::new(
                    "browser page content is not stored as untrusted evidence",
                ));
            }
            let archive_id = u64::try_from(archive_id).map_err(repository_error)?;
            let body_text = String::from_utf8(
                repository
                    .read_archived_content(archive_id)
                    .map_err(repository_error)?,
            )
            .map_err(repository_error)?;
            (
                Some(PageContentPayload {
                    body_text,
                    captured_at_millis,
                    authorized_origin: origin,
                }),
                Some(archive_id),
            )
        }
        _ => {
            return Err(RepositoryError::new(
                "persisted browser page content relation is invalid",
            ));
        }
    };
    let submission = BrowserSubmission::from_payload(BrowserSubmissionPayload {
        submission_id: stored.submission_id,
        url: stored.url,
        title: stored.title,
        visited_at_millis: stored.visited_at_millis,
        dwell_millis: stored.dwell_millis,
        page_content,
    })
    .map_err(repository_error)?;
    Ok(BrowserVisit::restore(
        BrowserVisitId::from_raw(id),
        HostSessionId::from_raw(host_session_id),
        submission,
        content_archive_id,
    ))
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

fn require_current_open_host_session(
    connection: &Connection,
    session_id: HostSessionId,
) -> Result<(), RepositoryError> {
    let current = current_host_session(connection)?
        .ok_or_else(|| RepositoryError::new("host session is not initialized"))?;
    if current.id() != session_id || current.ended_at().is_some() {
        return Err(RepositoryError::new(
            "capture checkpoint must target the current open host session",
        ));
    }
    Ok(())
}

fn insert_capture_span(
    transaction: &rusqlite::Transaction<'_>,
    host_session_id: HostSessionId,
    kind: CaptureSpanKind,
    started_at: Timestamp,
    observed_until: Timestamp,
) -> Result<CaptureSpan, RepositoryError> {
    let id = CaptureSpanId::from_raw(next_host_identifier(transaction, "capture_spans")?);
    let (kind_code, application, window_title, idle_state, gap_reason) = match &kind {
        CaptureSpanKind::Activity(snapshot) => (
            0,
            Some(snapshot.application()),
            Some(snapshot.window_title()),
            Some(encode_idle_state(snapshot.idle_state())),
            None,
        ),
        CaptureSpanKind::Gap(reason) => (
            1,
            None,
            None,
            None,
            Some(encode_capture_gap_reason(*reason)),
        ),
    };
    transaction
        .execute(
            "INSERT INTO capture_spans
             (id, started_in_host_session_id, kind, application, window_title,
              idle_state, gap_reason, started_at, observed_until, ended_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL)",
            params![
                to_sql_id(id.get())?,
                to_sql_id(host_session_id.get())?,
                kind_code,
                application,
                window_title,
                idle_state,
                gap_reason,
                started_at.as_millis(),
                observed_until.as_millis(),
            ],
        )
        .map_err(repository_error)?;
    Ok(CaptureSpan::restore(
        id,
        host_session_id,
        kind,
        started_at,
        observed_until,
        None,
    ))
}

fn current_capture_span(connection: &Connection) -> Result<Option<CaptureSpan>, RepositoryError> {
    connection
        .query_row(
            "SELECT id, started_in_host_session_id, kind, application,
                    window_title, idle_state, gap_reason, started_at,
                    observed_until, ended_at
             FROM capture_spans WHERE ended_at IS NULL ORDER BY id DESC LIMIT 1",
            [],
            stored_capture_span_from_row,
        )
        .optional()
        .map_err(repository_error)?
        .map(StoredCaptureSpan::decode)
        .transpose()
}

const fn encode_idle_state(value: IdleState) -> i64 {
    match value {
        IdleState::Active => 0,
        IdleState::Idle => 1,
    }
}

fn decode_idle_state(value: i64) -> Result<IdleState, RepositoryError> {
    match value {
        0 => Ok(IdleState::Active),
        1 => Ok(IdleState::Idle),
        _ => Err(RepositoryError::new("invalid persisted idle state")),
    }
}

const fn encode_capture_gap_reason(value: CaptureGapReason) -> i64 {
    match value {
        CaptureGapReason::Paused => 0,
        CaptureGapReason::SessionLocked => 1,
        CaptureGapReason::ExplicitExit => 2,
        CaptureGapReason::Update => 3,
        CaptureGapReason::Crash => 4,
        CaptureGapReason::SourceUnavailable => 5,
    }
}

fn decode_capture_gap_reason(value: i64) -> Result<CaptureGapReason, RepositoryError> {
    match value {
        0 => Ok(CaptureGapReason::Paused),
        1 => Ok(CaptureGapReason::SessionLocked),
        2 => Ok(CaptureGapReason::ExplicitExit),
        3 => Ok(CaptureGapReason::Update),
        4 => Ok(CaptureGapReason::Crash),
        5 => Ok(CaptureGapReason::SourceUnavailable),
        _ => Err(RepositoryError::new("invalid persisted capture gap reason")),
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

struct StoredCaptureSpan {
    id: i64,
    started_in_host_session: i64,
    kind: i64,
    application: Option<String>,
    window_title: Option<String>,
    idle_state: Option<i64>,
    gap_reason: Option<i64>,
    started_at: i64,
    observed_until: i64,
    ended_at: Option<i64>,
}

impl StoredCaptureSpan {
    fn decode(self) -> Result<CaptureSpan, RepositoryError> {
        if self.id <= 0
            || self.started_in_host_session <= 0
            || self.observed_until < self.started_at
            || self
                .ended_at
                .is_some_and(|ended| ended < self.observed_until)
        {
            return Err(RepositoryError::new("invalid persisted capture span"));
        }
        let kind = match (
            self.kind,
            self.application,
            self.window_title,
            self.idle_state,
            self.gap_reason,
        ) {
            (0, Some(application), Some(window_title), Some(idle_state), None) => {
                CaptureSpanKind::Activity(
                    ActivitySnapshot::new(
                        application,
                        window_title,
                        decode_idle_state(idle_state)?,
                    )
                    .map_err(|error| RepositoryError::new(error.to_string()))?,
                )
            }
            (1, None, None, None, Some(reason)) => {
                CaptureSpanKind::Gap(decode_capture_gap_reason(reason)?)
            }
            _ => return Err(RepositoryError::new("invalid persisted capture span kind")),
        };
        Ok(CaptureSpan::restore(
            CaptureSpanId::from_raw(u64::try_from(self.id).map_err(repository_error)?),
            HostSessionId::from_raw(
                u64::try_from(self.started_in_host_session).map_err(repository_error)?,
            ),
            kind,
            Timestamp::from_millis(self.started_at),
            Timestamp::from_millis(self.observed_until),
            self.ended_at.map(Timestamp::from_millis),
        ))
    }
}

fn stored_capture_span_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredCaptureSpan> {
    Ok(StoredCaptureSpan {
        id: row.get(0)?,
        started_in_host_session: row.get(1)?,
        kind: row.get(2)?,
        application: row.get(3)?,
        window_title: row.get(4)?,
        idle_state: row.get(5)?,
        gap_reason: row.get(6)?,
        started_at: row.get(7)?,
        observed_until: row.get(8)?,
        ended_at: row.get(9)?,
    })
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

const fn encode_claim_status(status: ClaimStatus) -> i64 {
    match status {
        ClaimStatus::Current => 0,
        ClaimStatus::Superseded => 1,
    }
}

fn decode_claim_status(value: i64) -> Result<ClaimStatus, RepositoryError> {
    match value {
        0 => Ok(ClaimStatus::Current),
        1 => Ok(ClaimStatus::Superseded),
        _ => Err(RepositoryError::new("invalid persisted claim status")),
    }
}

const fn encode_memory_subject(value: MemorySubject) -> i64 {
    match value {
        MemorySubject::Person => 0,
        MemorySubject::Counterpart => 1,
        MemorySubject::Shared => 2,
    }
}

fn decode_memory_subject(value: i64) -> Result<MemorySubject, RepositoryError> {
    match value {
        0 => Ok(MemorySubject::Person),
        1 => Ok(MemorySubject::Counterpart),
        2 => Ok(MemorySubject::Shared),
        _ => Err(RepositoryError::new("invalid persisted memory subject")),
    }
}

const fn encode_memory_kind(value: MemoryKind) -> i64 {
    match value {
        MemoryKind::Fact => 0,
        MemoryKind::Preference => 1,
        MemoryKind::Goal => 2,
        MemoryKind::Relationship => 3,
        MemoryKind::Hypothesis => 4,
    }
}

fn decode_memory_kind(value: i64) -> Result<MemoryKind, RepositoryError> {
    match value {
        0 => Ok(MemoryKind::Fact),
        1 => Ok(MemoryKind::Preference),
        2 => Ok(MemoryKind::Goal),
        3 => Ok(MemoryKind::Relationship),
        4 => Ok(MemoryKind::Hypothesis),
        _ => Err(RepositoryError::new("invalid persisted memory kind")),
    }
}

const fn encode_memory_confidence(value: MemoryConfidence) -> i64 {
    match value {
        MemoryConfidence::Low => 0,
        MemoryConfidence::Medium => 1,
        MemoryConfidence::High => 2,
    }
}

fn decode_memory_confidence(value: i64) -> Result<MemoryConfidence, RepositoryError> {
    match value {
        0 => Ok(MemoryConfidence::Low),
        1 => Ok(MemoryConfidence::Medium),
        2 => Ok(MemoryConfidence::High),
        _ => Err(RepositoryError::new("invalid persisted memory confidence")),
    }
}

const fn encode_memory_basis(value: MemoryBasis) -> i64 {
    match value {
        MemoryBasis::DirectEvidence => 0,
        MemoryBasis::InterpretiveInference => 1,
        MemoryBasis::PatternCandidate => 2,
    }
}

fn decode_memory_basis(value: i64) -> Result<MemoryBasis, RepositoryError> {
    match value {
        0 => Ok(MemoryBasis::DirectEvidence),
        1 => Ok(MemoryBasis::InterpretiveInference),
        2 => Ok(MemoryBasis::PatternCandidate),
        _ => Err(RepositoryError::new("invalid persisted memory basis")),
    }
}

const fn encode_memory_status(value: MemoryStatus) -> i64 {
    match value {
        MemoryStatus::Active => 0,
        MemoryStatus::Provisional => 1,
        MemoryStatus::ProvisionalPattern => 2,
        MemoryStatus::Superseded => 3,
        MemoryStatus::Disputed => 4,
        MemoryStatus::Retracted => 5,
        MemoryStatus::SupportedCounterpartView => 6,
        MemoryStatus::Weakened => 7,
    }
}

fn decode_memory_status(value: i64) -> Result<MemoryStatus, RepositoryError> {
    match value {
        0 => Ok(MemoryStatus::Active),
        1 => Ok(MemoryStatus::Provisional),
        2 => Ok(MemoryStatus::ProvisionalPattern),
        3 => Ok(MemoryStatus::Superseded),
        4 => Ok(MemoryStatus::Disputed),
        5 => Ok(MemoryStatus::Retracted),
        6 => Ok(MemoryStatus::SupportedCounterpartView),
        7 => Ok(MemoryStatus::Weakened),
        _ => Err(RepositoryError::new("invalid persisted memory status")),
    }
}

const fn encode_dispute_outcome(value: MemoryDisputeOutcome) -> i64 {
    match value {
        MemoryDisputeOutcome::Open => 0,
        MemoryDisputeOutcome::Retracted => 1,
        MemoryDisputeOutcome::Revised => 2,
        MemoryDisputeOutcome::Maintained => 3,
        MemoryDisputeOutcome::Weakened => 4,
    }
}

fn decode_dispute_outcome(value: i64) -> Result<MemoryDisputeOutcome, RepositoryError> {
    match value {
        0 => Ok(MemoryDisputeOutcome::Open),
        1 => Ok(MemoryDisputeOutcome::Retracted),
        2 => Ok(MemoryDisputeOutcome::Revised),
        3 => Ok(MemoryDisputeOutcome::Maintained),
        4 => Ok(MemoryDisputeOutcome::Weakened),
        _ => Err(RepositoryError::new(
            "invalid persisted memory dispute outcome",
        )),
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
    counterpart_identity_version: Option<i64>,
}

impl StoredEvidence {
    fn decode(self) -> Result<ConversationEvidence, RepositoryError> {
        let id = u64::try_from(self.id).map_err(repository_error)?;
        let speaker = decode_speaker(self.speaker)?;
        let identity_version = self
            .counterpart_identity_version
            .map(u64::try_from)
            .transpose()
            .map_err(repository_error)?;
        match (speaker, identity_version) {
            (Speaker::Person, None) => Ok(ConversationEvidence::restore(
                EvidenceId::from_raw(id),
                SessionId::new(self.session_id),
                Speaker::Person,
                self.verbatim,
                Timestamp::from_millis(self.recorded_at),
            )),
            (Speaker::Person, Some(_)) => Err(RepositoryError::new(
                "person evidence cannot carry counterpart identity attribution",
            )),
            (Speaker::Counterpart, None) => Ok(ConversationEvidence::restore_counterpart(
                EvidenceId::from_raw(id),
                SessionId::new(self.session_id),
                self.verbatim,
                Timestamp::from_millis(self.recorded_at),
                CounterpartReplyAttribution::PreIdentityUnbound,
            )),
            (Speaker::Counterpart, Some(version)) => Ok(ConversationEvidence::restore_counterpart(
                EvidenceId::from_raw(id),
                SessionId::new(self.session_id),
                self.verbatim,
                Timestamp::from_millis(self.recorded_at),
                CounterpartReplyAttribution::IdentityBound(version),
            )),
        }
    }
}

fn stored_evidence_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredEvidence> {
    Ok(StoredEvidence {
        id: row.get(0)?,
        session_id: row.get(1)?,
        speaker: row.get(2)?,
        verbatim: row.get(3)?,
        recorded_at: row.get(4)?,
        counterpart_identity_version: row.get(5)?,
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
    supersedes_claim_id: Option<i64>,
    status: i64,
    superseded_by_claim_id: Option<i64>,
}

impl StoredClaim {
    fn decode(self, connection: &Connection) -> Result<Claim, RepositoryError> {
        let id = u64::try_from(self.id).map_err(repository_error)?;
        let claim_id = ClaimId::from_raw(id);
        let support = load_support(connection, claim_id)?;
        Ok(Claim::restore_versioned(
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
            decode_claim_status(self.status)?,
            self.supersedes_claim_id
                .map(|id| u64::try_from(id).map(ClaimId::from_raw))
                .transpose()
                .map_err(repository_error)?,
            self.superseded_by_claim_id
                .map(|id| u64::try_from(id).map(ClaimId::from_raw))
                .transpose()
                .map_err(repository_error)?,
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
        supersedes_claim_id: row.get(8)?,
        status: row.get(9)?,
        superseded_by_claim_id: row.get(10)?,
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
    use eam_core::{IncrementingClock, PatternMaturityProposal};
    use eam_memory::{MemoryError, MemoryMaintenance, MemoryProposal};
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn pre_identity_reply_cannot_authorize_a_counterpart_agreement_withdrawal() {
        let _guard = crate::test_support::sqlcipher_test_lock();
        let directory = tempdir().unwrap();
        let mut repository =
            VaultRepository::open(directory.path(), VaultKey::new([0x58; 32])).unwrap();
        let evidence_id = repository.next_evidence_id();
        let reason = "这条创建前回复不能代表当前第二自我";
        let effective_at = Timestamp::from_millis(500);
        repository
            .append_evidence(ConversationEvidence::restore(
                evidence_id,
                SessionId::new("legacy-withdrawal"),
                Speaker::Counterpart,
                reason.to_owned(),
                effective_at,
            ))
            .unwrap();
        let withdrawal = AgreementWithdrawal::restore(
            ClaimId::from_raw(2),
            ClaimId::from_raw(1),
            AgreementWithdrawalActor::Counterpart,
            effective_at,
            Some(reason.to_owned()),
            vec![EvidenceCitation::new(evidence_id, reason)],
        );

        assert!(
            !has_exact_withdrawal_actor_evidence(
                repository.connection(),
                withdrawal.evidence_refs(),
                &withdrawal,
            )
            .unwrap()
        );
    }

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
    fn interrupted_runtime_profile_update_rolls_back_every_field_across_reopen() {
        let _guard = crate::test_support::sqlcipher_test_lock();
        let directory = tempdir().unwrap();
        let key = [0x57; 32];
        let mut repository = VaultRepository::open(directory.path(), VaultKey::new(key)).unwrap();
        repository
            .update_runtime_profile(
                "https://runtime.example.test/v1",
                "owner/model-v1",
                RuntimeProfileKeyAction::Replace("synthetic-original-key"),
            )
            .unwrap();

        let result = repository.update_runtime_profile_with_hook(
            "http://127.0.0.1:11434/next/",
            "owner/model-v2",
            RuntimeProfileKeyAction::Replace("synthetic-interrupted-key"),
            |_| Err(VaultError::RuntimeProfileUpdateInterrupted),
        );

        assert!(matches!(
            result,
            Err(VaultError::RuntimeProfileUpdateInterrupted)
        ));
        let profile = repository.runtime_profile().unwrap();
        assert_eq!(profile.base_url(), "https://runtime.example.test/v1");
        assert_eq!(profile.model(), "owner/model-v1");
        assert_eq!(profile.bearer_key(), Some("synthetic-original-key"));
        drop(profile);
        repository.close().unwrap();

        let repository = VaultRepository::open(directory.path(), VaultKey::new(key)).unwrap();
        let profile = repository.runtime_profile().unwrap();
        assert_eq!(profile.base_url(), "https://runtime.example.test/v1");
        assert_eq!(profile.model(), "owner/model-v1");
        assert_eq!(profile.bearer_key(), Some("synthetic-original-key"));
    }

    #[test]
    fn interrupted_source_root_activation_rolls_back_both_lifecycles_across_reopen() {
        let _guard = crate::test_support::sqlcipher_test_lock();
        let directory = tempdir().unwrap();
        let key = [0x59; 32];
        let mut repository = VaultRepository::open(directory.path(), VaultKey::new(key)).unwrap();
        let first = repository
            .register_source_root("C:/notes/atomic-first", 10)
            .unwrap();
        let second = repository
            .register_source_root("C:/notes/atomic-second", 11)
            .unwrap();
        repository
            .finish_source_reconciliation(first.id(), &[], 20)
            .unwrap();
        repository
            .finish_source_reconciliation(second.id(), &[], 21)
            .unwrap();
        repository.activate_source_root(first.id(), 30).unwrap();

        let result = repository.activate_source_root_with_hook(second.id(), 40, |transaction| {
            let states = transaction
                .prepare("SELECT id, lifecycle_state FROM source_roots ORDER BY id")?
                .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            assert_eq!(states, vec![(1, 2), (2, 1)]);
            Err(VaultError::InvalidKeyOrCorrupt)
        });

        assert!(matches!(result, Err(VaultError::InvalidKeyOrCorrupt)));
        assert_eq!(
            repository
                .load_active_source_root()
                .unwrap()
                .unwrap()
                .root()
                .id(),
            first.id()
        );
        assert_eq!(
            repository
                .load_source_root(first.id())
                .unwrap()
                .root()
                .lifecycle(),
            SourceRootLifecycle::Active
        );
        assert_eq!(
            repository
                .load_source_root(second.id())
                .unwrap()
                .root()
                .lifecycle(),
            SourceRootLifecycle::Staged
        );
        repository.close().unwrap();

        let repository = VaultRepository::open(directory.path(), VaultKey::new(key)).unwrap();
        assert_eq!(
            repository
                .load_active_source_root()
                .unwrap()
                .unwrap()
                .root()
                .id(),
            first.id()
        );
        assert_eq!(
            repository
                .load_source_root(second.id())
                .unwrap()
                .root()
                .lifecycle(),
            SourceRootLifecycle::Staged
        );
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
    fn browser_capture_failure_rolls_back_visit_and_content_reference_across_reopen() {
        let _guard = crate::test_support::sqlcipher_test_lock();
        let directory = tempdir().unwrap();
        let key = [0x2a; 32];
        let mut repository = VaultRepository::open(directory.path(), VaultKey::new(key)).unwrap();
        let session = repository
            .begin_host_session(Timestamp::from_millis(1), LaunchMode::Foreground)
            .unwrap()
            .session()
            .id();
        let submission = BrowserSubmission::from_payload(BrowserSubmissionPayload {
            submission_id: "browser-interrupted".to_owned(),
            url: "https://example.test/interrupted".to_owned(),
            title: "Interrupted".to_owned(),
            visited_at_millis: 10,
            dwell_millis: 5,
            page_content: Some(PageContentPayload {
                body_text: "untrusted interrupted content".to_owned(),
                captured_at_millis: 15,
                authorized_origin: "https://example.test".to_owned(),
            }),
        })
        .unwrap();

        let result = repository.record_browser_submission_with_hook(session, &submission, |_| {
            Err(RepositoryError::new("browser capture interrupted"))
        });

        assert!(result.is_err());
        assert!(repository.all_browser_visits().unwrap().is_empty());
        assert!(repository.archived_evidence().unwrap().is_empty());
        assert_eq!(repository.object_store.object_file_count().unwrap(), 1);
        repository.close().unwrap();

        let repository = VaultRepository::open(directory.path(), VaultKey::new(key)).unwrap();
        assert!(repository.all_browser_visits().unwrap().is_empty());
        assert!(repository.archived_evidence().unwrap().is_empty());
        assert_eq!(repository.object_store.object_file_count().unwrap(), 0);
    }

    #[test]
    fn forget_failure_rolls_back_intent_authority_and_retrieval_across_reopen() {
        let _guard = crate::test_support::sqlcipher_test_lock();
        let directory = tempdir().unwrap();
        let key = [0x39; 32];
        let mut repository = VaultRepository::open(directory.path(), VaultKey::new(key)).unwrap();
        let receipt = repository
            .archive(ArchiveInput {
                source_locator: "inbox/forget-interrupted.md",
                content: b"# Atomic Forget\n\nStill searchable after rollback.\n",
                status: ArchiveStatus::Archived,
                archived_at_millis: 10,
            })
            .unwrap();
        eam_ingestion::process_archived_markdown(
            &mut repository,
            receipt.archive_id,
            eam_markdown::ParseLimits::default(),
            20,
            21,
        )
        .unwrap();
        eam_ingestion::materialize_accepted_markdown(
            &mut repository,
            receipt.archive_id,
            eam_markdown::CONTRACT_VERSION,
        )
        .unwrap();
        let query = RetrievalQuery::lexical("searchable");
        assert!(
            !eam_retrieval::retrieve(&mut repository, &query)
                .unwrap()
                .candidates()
                .is_empty()
        );

        let result = repository.forget_with_hook(
            ForgetTarget::ArchivedEvidence(receipt.archive_id),
            Timestamp::from_millis(30),
            |_| Err(VaultError::ForgetInterrupted),
        );

        assert!(matches!(result, Err(VaultError::ForgetInterrupted)));
        assert!(repository.deletion_intents().unwrap().is_empty());
        assert_eq!(
            repository
                .read_archived_content(receipt.archive_id)
                .unwrap(),
            b"# Atomic Forget\n\nStill searchable after rollback.\n"
        );
        assert!(
            !eam_retrieval::retrieve(&mut repository, &query)
                .unwrap()
                .candidates()
                .is_empty()
        );
        repository.close().unwrap();

        let mut repository = VaultRepository::open(directory.path(), VaultKey::new(key)).unwrap();
        assert!(repository.deletion_intents().unwrap().is_empty());
        assert!(
            !eam_retrieval::retrieve(&mut repository, &query)
                .unwrap()
                .candidates()
                .is_empty()
        );
    }

    #[test]
    fn reflection_forget_failure_rolls_back_evidence_and_invitation_across_reopen() {
        let _guard = crate::test_support::sqlcipher_test_lock();
        let directory = tempdir().unwrap();
        let key = [0x49; 32];
        let mut repository = VaultRepository::open(directory.path(), VaultKey::new(key)).unwrap();
        let evidence_id = repository.next_evidence_id();
        repository
            .append_evidence(ConversationEvidence::restore(
                evidence_id,
                SessionId::new("reflection-forget-interrupted"),
                Speaker::Person,
                "工作再次挤压了真实生活。".to_owned(),
                Timestamp::from_millis(10),
            ))
            .unwrap();
        let invitation = ReflectionInvitation::restore(
            repository.next_reflection_invitation_id(),
            "工作挤压生活",
            "你刚才明确说工作再次挤压了真实生活。",
            vec![EvidenceCitation::new(
                evidence_id,
                "工作再次挤压了真实生活。",
            )],
            "这是一项有直接证据的重要变化。",
            ReflectionImportance::Important,
            ReflectionInvitationBasis::ImportantSingleChange,
            ReflectionInvitationState::Pending,
            Timestamp::from_millis(20),
            Timestamp::from_millis(20),
            None,
            None,
            0,
            false,
        );
        repository
            .commit_reflection_invitation(invitation.clone())
            .unwrap();

        let result = repository.forget_with_hook(
            ForgetTarget::ConversationEvidence(evidence_id),
            Timestamp::from_millis(30),
            |_| Err(VaultError::ForgetInterrupted),
        );

        assert!(matches!(result, Err(VaultError::ForgetInterrupted)));
        assert!(repository.deletion_intents().unwrap().is_empty());
        assert!(
            MemoryRepository::evidence(&repository, evidence_id)
                .unwrap()
                .is_some()
        );
        assert_eq!(
            repository.reflection_invitation(invitation.id()).unwrap(),
            Some(invitation.clone())
        );
        repository.close().unwrap();

        let repository = VaultRepository::open(directory.path(), VaultKey::new(key)).unwrap();
        assert!(repository.deletion_intents().unwrap().is_empty());
        assert!(
            MemoryRepository::evidence(&repository, evidence_id)
                .unwrap()
                .is_some()
        );
        assert_eq!(
            repository.reflection_invitation(invitation.id()).unwrap(),
            Some(invitation)
        );
    }

    #[test]
    fn pattern_maturity_failure_rolls_back_version_and_qualification_record_across_reopen() {
        let _guard = crate::test_support::sqlcipher_test_lock();
        let directory = tempdir().unwrap();
        let key = [0x59; 32];
        let repository = VaultRepository::open(directory.path(), VaultKey::new(key)).unwrap();
        let (repository, pattern) = seed_interrupted_pattern_maturity(repository);
        repository
            .connection()
            .execute_batch(
                "CREATE TRIGGER interrupt_pattern_maturity
                 BEFORE INSERT ON pattern_maturity_evidence
                 WHEN NEW.role = 1
                 BEGIN
                   SELECT RAISE(ABORT, 'pattern maturity interrupted');
                 END;",
            )
            .unwrap();
        let mut maintenance = MemoryMaintenance::new(repository, IncrementingClock::new(2_000));
        let result = maintenance.mature_pattern(
            &PatternMaturityProposal::new(
                pattern.id().get(),
                pattern.version(),
                "New support survived another review and discussion",
            )
            .with_new_support_claim(ClaimId::from_raw(4))
            .with_counterexample_review(EvidenceCitation::new(
                EvidenceId::from_raw(11),
                "I checked the newer sequence for exceptions",
            ))
            .with_discussion_evidence([
                EvidenceCitation::new(
                    EvidenceId::from_raw(12),
                    "I see the tendency, although it does not fit every week",
                ),
                EvidenceCitation::new(
                    EvidenceId::from_raw(13),
                    "I still see a bounded recurring tendency",
                ),
            ]),
        );
        assert!(matches!(result, Err(MemoryError::Repository(_))));
        assert_pattern_maturity_absent(maintenance.repository(), pattern.id());
        let (repository, _) = maintenance.into_parts();
        repository.close().unwrap();

        let repository = VaultRepository::open(directory.path(), VaultKey::new(key)).unwrap();
        assert_pattern_maturity_absent(&repository, pattern.id());
    }

    fn seed_interrupted_pattern_maturity(
        mut repository: VaultRepository,
    ) -> (VaultRepository, MemoryVersion) {
        append_interrupted_pattern_supports(&mut repository);
        append_interrupted_pattern_qualification_evidence(&mut repository);
        let mut maintenance = MemoryMaintenance::new(repository, IncrementingClock::new(1_000));
        let pattern = maintenance
            .propose(
                &MemoryProposal::new("Planning reviews tend to become calmer across months")
                    .with_subject(MemorySubject::Counterpart)
                    .with_kind(MemoryKind::Hypothesis)
                    .with_source_claims([
                        ClaimId::from_raw(1),
                        ClaimId::from_raw(2),
                        ClaimId::from_raw(3),
                    ])
                    .with_applicable_time(ApplicableTime::Since(Timestamp::from_millis(100)))
                    .with_confidence(MemoryConfidence::Medium)
                    .with_salience_reason("Retain the bounded cross-month pattern")
                    .with_basis(MemoryBasis::PatternCandidate)
                    .with_pattern_counterexample_review(EvidenceCitation::new(
                        EvidenceId::from_raw(10),
                        "I checked the initial sequence for exceptions",
                    )),
            )
            .unwrap();
        let (repository, _) = maintenance.into_parts();
        (repository, pattern)
    }

    fn append_interrupted_pattern_supports(repository: &mut VaultRepository) {
        for (id, quote, recorded_at) in [
            (1, "I reviewed plans calmly in January", 100),
            (2, "I reviewed plans calmly in February", 200),
            (3, "I reviewed plans calmly in March", 300),
            (4, "I reviewed plans calmly in April", 1_200),
        ] {
            repository
                .append_evidence(ConversationEvidence::restore(
                    EvidenceId::from_raw(id),
                    SessionId::new("maturity-interrupted"),
                    Speaker::Person,
                    quote.to_owned(),
                    Timestamp::from_millis(recorded_at),
                ))
                .unwrap();
            repository
                .append_claim(Claim::restore(
                    ClaimId::from_raw(id),
                    ClaimOwner::Counterpart,
                    format!("planning review event {id}"),
                    vec![EvidenceCitation::new(EvidenceId::from_raw(id), quote)],
                    Some(Uncertainty::Medium),
                    ApplicableTime::At(Timestamp::from_millis(recorded_at)),
                    Timestamp::from_millis(recorded_at),
                ))
                .unwrap();
        }
    }

    fn append_interrupted_pattern_qualification_evidence(repository: &mut VaultRepository) {
        for (id, speaker, quote, recorded_at) in [
            (
                10,
                Speaker::Counterpart,
                "I checked the initial sequence for exceptions",
                350,
            ),
            (
                11,
                Speaker::Counterpart,
                "I checked the newer sequence for exceptions",
                1_300,
            ),
            (
                12,
                Speaker::Person,
                "I see the tendency, although it does not fit every week",
                1_400,
            ),
            (
                13,
                Speaker::Counterpart,
                "I still see a bounded recurring tendency",
                1_500,
            ),
        ] {
            repository
                .append_evidence(ConversationEvidence::restore(
                    EvidenceId::from_raw(id),
                    SessionId::new("maturity-interrupted"),
                    speaker,
                    quote.to_owned(),
                    Timestamp::from_millis(recorded_at),
                ))
                .unwrap();
        }
    }

    fn assert_pattern_maturity_absent(repository: &VaultRepository, memory_id: MemoryId) {
        let current = repository.current_memory(memory_id).unwrap().unwrap();
        assert_eq!(current.status(), MemoryStatus::ProvisionalPattern);
        assert_eq!(repository.memory_versions(memory_id).unwrap().len(), 1);
        assert!(
            repository
                .pattern_maturity_records(memory_id)
                .unwrap()
                .is_empty()
        );
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
