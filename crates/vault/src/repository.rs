use std::{
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
use fs4::{FileExt, TryLockError};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use crate::{VaultError, VaultKey, crypto::sqlcipher_key_pragma, schema::migrate};

const DATABASE_FILE: &str = "self.db";
const WRITER_LOCK_FILE: &str = "self.db.writer.lock";

pub struct VaultRepository {
    connection: Option<Connection>,
    writer_lock: Option<File>,
    vault_key: VaultKey,
    database_path: PathBuf,
    next_evidence_id: u64,
    next_claim_id: u64,
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

        let next_evidence_id = next_identifier(&connection, "conversation_evidence")?;
        let next_claim_id = next_identifier(&connection, "claims")?;

        Ok(Self {
            connection: Some(connection),
            writer_lock: Some(writer_lock),
            vault_key,
            database_path,
            next_evidence_id,
            next_claim_id,
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

fn next_identifier(connection: &Connection, table: &str) -> Result<u64, VaultError> {
    let query = format!("SELECT COALESCE(MAX(id), 0) FROM {table}");
    let maximum: i64 = connection.query_row(&query, [], |row| row.get(0))?;
    let maximum = u64::try_from(maximum).map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
    maximum
        .checked_add(1)
        .ok_or(VaultError::InvalidKeyOrCorrupt)
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
}
