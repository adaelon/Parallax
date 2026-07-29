use rusqlite::{Connection, TransactionBehavior};

use crate::VaultError;

pub(crate) const LATEST_SCHEMA_VERSION: i64 = 5;

const MIGRATION_1: &str = r"
CREATE TABLE conversation_evidence (
    id INTEGER PRIMARY KEY CHECK (id > 0),
    session_id TEXT NOT NULL,
    speaker INTEGER NOT NULL CHECK (speaker IN (0, 1)),
    verbatim TEXT NOT NULL,
    recorded_at INTEGER NOT NULL
) STRICT;

CREATE TABLE claims (
    id INTEGER PRIMARY KEY CHECK (id > 0),
    owner INTEGER NOT NULL CHECK (owner IN (0, 1, 2)),
    statement TEXT NOT NULL,
    uncertainty INTEGER CHECK (uncertainty IS NULL OR uncertainty IN (0, 1, 2)),
    applicable_kind INTEGER NOT NULL CHECK (applicable_kind IN (0, 1, 2, 3)),
    applicable_start INTEGER,
    applicable_end INTEGER,
    recorded_at INTEGER NOT NULL
) STRICT;

CREATE TABLE claim_support (
    claim_id INTEGER NOT NULL REFERENCES claims(id) ON DELETE RESTRICT,
    ordinal INTEGER NOT NULL,
    evidence_id INTEGER NOT NULL REFERENCES conversation_evidence(id) ON DELETE RESTRICT,
    quote TEXT NOT NULL,
    PRIMARY KEY (claim_id, ordinal)
) STRICT;

CREATE INDEX claim_support_evidence_idx ON claim_support(evidence_id);
";

const MIGRATION_2: &str = r"
CREATE TABLE initial_self_introduction (
    category INTEGER PRIMARY KEY CHECK (category BETWEEN 0 AND 5),
    evidence_id INTEGER NOT NULL UNIQUE
        REFERENCES conversation_evidence(id) ON DELETE RESTRICT,
    claim_id INTEGER NOT NULL UNIQUE
        REFERENCES claims(id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE identity_state_versions (
    version INTEGER PRIMARY KEY CHECK (version > 0),
    predecessor_version INTEGER UNIQUE
        REFERENCES identity_state_versions(version) ON DELETE RESTRICT,
    name TEXT NOT NULL CHECK (length(trim(name)) > 0),
    expression_traits TEXT NOT NULL CHECK (length(trim(expression_traits)) > 0),
    viewpoints TEXT NOT NULL CHECK (length(trim(viewpoints)) > 0),
    value_priorities TEXT NOT NULL CHECK (length(trim(value_priorities)) > 0),
    relationship_posture TEXT NOT NULL CHECK (length(trim(relationship_posture)) > 0),
    own_goals TEXT NOT NULL CHECK (length(trim(own_goals)) > 0),
    change_reason TEXT NOT NULL CHECK (length(trim(change_reason)) > 0),
    formed_at INTEGER NOT NULL
) STRICT;

CREATE TABLE identity_state_evidence (
    identity_version INTEGER NOT NULL
        REFERENCES identity_state_versions(version) ON DELETE RESTRICT,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    evidence_id INTEGER NOT NULL
        REFERENCES conversation_evidence(id) ON DELETE RESTRICT,
    PRIMARY KEY (identity_version, ordinal)
) STRICT;

CREATE INDEX identity_state_evidence_source_idx
    ON identity_state_evidence(evidence_id);
";

const MIGRATION_3: &str = r"
CREATE TABLE self_bundle_versions (
    version INTEGER PRIMARY KEY CHECK (version > 0),
    predecessor_version INTEGER UNIQUE
        REFERENCES self_bundle_versions(version) ON DELETE RESTRICT,
    constitution_version INTEGER NOT NULL CHECK (constitution_version > 0),
    identity_state_version INTEGER NOT NULL
        REFERENCES identity_state_versions(version) ON DELETE RESTRICT,
    relationship_state TEXT NOT NULL CHECK (length(trim(relationship_state)) > 0),
    wake_trigger INTEGER CHECK (wake_trigger IS NULL OR wake_trigger BETWEEN 0 AND 3),
    wake_exit INTEGER CHECK (wake_exit IS NULL OR wake_exit BETWEEN 0 AND 3),
    committed_at INTEGER NOT NULL,
    CHECK (
        (version = 1 AND predecessor_version IS NULL
         AND wake_trigger IS NULL AND wake_exit IS NULL)
        OR
        (version > 1 AND predecessor_version IS NOT NULL
         AND wake_trigger IS NOT NULL AND wake_exit IS NOT NULL)
    )
) STRICT;

CREATE TABLE self_bundle_experiences (
    bundle_version INTEGER NOT NULL
        REFERENCES self_bundle_versions(version) ON DELETE RESTRICT,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    experience_ref TEXT NOT NULL CHECK (length(trim(experience_ref)) > 0),
    PRIMARY KEY (bundle_version, ordinal),
    UNIQUE (bundle_version, experience_ref)
) STRICT;

CREATE TABLE self_bundle_beliefs (
    bundle_version INTEGER NOT NULL
        REFERENCES self_bundle_versions(version) ON DELETE RESTRICT,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    claim_id INTEGER NOT NULL REFERENCES claims(id) ON DELETE RESTRICT,
    PRIMARY KEY (bundle_version, ordinal),
    UNIQUE (bundle_version, claim_id)
) STRICT;

CREATE INDEX self_bundle_belief_claim_idx ON self_bundle_beliefs(claim_id);

CREATE TABLE self_bundle_pending_intentions (
    bundle_version INTEGER NOT NULL
        REFERENCES self_bundle_versions(version) ON DELETE RESTRICT,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    intention TEXT NOT NULL CHECK (length(trim(intention)) > 0),
    PRIMARY KEY (bundle_version, ordinal),
    UNIQUE (bundle_version, intention)
) STRICT;
";

const MIGRATION_4: &str = r"
CREATE TABLE host_sessions (
    id INTEGER PRIMARY KEY CHECK (id > 0),
    launch_mode INTEGER NOT NULL CHECK (launch_mode BETWEEN 0 AND 2),
    started_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL CHECK (last_seen_at >= started_at),
    ended_at INTEGER,
    end_reason INTEGER CHECK (end_reason IS NULL OR end_reason IN (0, 1)),
    CHECK (
        (ended_at IS NULL AND end_reason IS NULL)
        OR
        (ended_at IS NOT NULL AND ended_at >= last_seen_at AND end_reason IS NOT NULL)
    )
) STRICT;

CREATE TABLE host_runtime_gaps (
    id INTEGER PRIMARY KEY CHECK (id > 0),
    from_at INTEGER NOT NULL,
    to_at INTEGER NOT NULL CHECK (to_at >= from_at),
    reason INTEGER NOT NULL CHECK (reason BETWEEN 0 AND 2),
    clock_rollback INTEGER NOT NULL CHECK (clock_rollback IN (0, 1)),
    recovered_by_session_id INTEGER NOT NULL
        REFERENCES host_sessions(id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX host_runtime_gap_recovery_idx
    ON host_runtime_gaps(recovered_by_session_id);
";

const MIGRATION_5: &str = r"
CREATE TABLE archived_evidence (
    id INTEGER PRIMARY KEY CHECK (id > 0),
    source_kind INTEGER NOT NULL CHECK (source_kind = 0),
    source_locator TEXT NOT NULL CHECK (length(source_locator) > 0),
    object_id TEXT NOT NULL CHECK (length(object_id) = 64),
    content_length INTEGER NOT NULL CHECK (content_length >= 0),
    status INTEGER NOT NULL CHECK (status IN (0, 1)),
    unparsed_reason INTEGER CHECK (unparsed_reason IS NULL OR unparsed_reason = 0),
    archived_at INTEGER NOT NULL,
    CHECK (
        (status = 0 AND unparsed_reason IS NULL)
        OR (status = 1 AND unparsed_reason = 0)
    ),
    UNIQUE (source_kind, source_locator, object_id)
) STRICT;

CREATE INDEX archived_evidence_object_idx ON archived_evidence(object_id);
";

pub(crate) fn migrate(connection: &mut Connection) -> Result<(), VaultError> {
    migrate_with_hook(connection, |_, _| Ok(()))
}

fn migrate_with_hook<F>(connection: &mut Connection, mut hook: F) -> Result<(), VaultError>
where
    F: FnMut(i64, &rusqlite::Transaction<'_>) -> Result<(), VaultError>,
{
    let mut version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > LATEST_SCHEMA_VERSION {
        return Err(VaultError::UnsupportedSchema(version));
    }

    while version < LATEST_SCHEMA_VERSION {
        let target = version + 1;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        match target {
            1 => transaction.execute_batch(MIGRATION_1)?,
            2 => transaction.execute_batch(MIGRATION_2)?,
            3 => transaction.execute_batch(MIGRATION_3)?,
            4 => transaction.execute_batch(MIGRATION_4)?,
            5 => transaction.execute_batch(MIGRATION_5)?,
            _ => return Err(VaultError::UnsupportedSchema(target)),
        }
        hook(target, &transaction)?;
        transaction.pragma_update(None, "user_version", target)?;
        transaction.commit()?;
        version = target;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupted_migration_rolls_back_before_reopen() {
        let _guard = crate::test_support::sqlcipher_test_lock();
        let mut connection = Connection::open_in_memory().unwrap();
        let result = migrate_with_hook(&mut connection, |target, _| {
            Err(VaultError::MigrationInterrupted(target))
        });

        assert!(matches!(result, Err(VaultError::MigrationInterrupted(1))));
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 0);
        let table_count: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name = 'conversation_evidence'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 0);

        migrate(&mut connection).unwrap();
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, LATEST_SCHEMA_VERSION);
    }

    #[test]
    fn interrupted_identity_migration_keeps_the_previous_schema_reopenable() {
        let _guard = crate::test_support::sqlcipher_test_lock();
        let mut connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(MIGRATION_1).unwrap();
        connection.pragma_update(None, "user_version", 1).unwrap();

        let result = migrate_with_hook(&mut connection, |target, _| {
            if target == 2 {
                Err(VaultError::MigrationInterrupted(target))
            } else {
                Ok(())
            }
        });

        assert!(matches!(result, Err(VaultError::MigrationInterrupted(2))));
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 1);
        let table_count: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name = 'identity_state_versions'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 0);

        migrate(&mut connection).unwrap();
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, LATEST_SCHEMA_VERSION);
    }

    #[test]
    fn interrupted_self_bundle_migration_keeps_identity_schema_reopenable() {
        let _guard = crate::test_support::sqlcipher_test_lock();
        let mut connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(MIGRATION_1).unwrap();
        connection.execute_batch(MIGRATION_2).unwrap();
        connection.pragma_update(None, "user_version", 2).unwrap();

        let result = migrate_with_hook(&mut connection, |target, _| {
            if target == 3 {
                Err(VaultError::MigrationInterrupted(target))
            } else {
                Ok(())
            }
        });

        assert!(matches!(result, Err(VaultError::MigrationInterrupted(3))));
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 2);
        let table_count: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name = 'self_bundle_versions'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 0);

        migrate(&mut connection).unwrap();
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, LATEST_SCHEMA_VERSION);
    }

    #[test]
    fn interrupted_host_lifecycle_migration_keeps_self_bundle_schema_reopenable() {
        let _guard = crate::test_support::sqlcipher_test_lock();
        let mut connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(MIGRATION_1).unwrap();
        connection.execute_batch(MIGRATION_2).unwrap();
        connection.execute_batch(MIGRATION_3).unwrap();
        connection.pragma_update(None, "user_version", 3).unwrap();

        let result = migrate_with_hook(&mut connection, |target, _| {
            if target == 4 {
                Err(VaultError::MigrationInterrupted(target))
            } else {
                Ok(())
            }
        });

        assert!(matches!(result, Err(VaultError::MigrationInterrupted(4))));
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 3);
        let table_count: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name = 'host_sessions'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 0);

        migrate(&mut connection).unwrap();
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, LATEST_SCHEMA_VERSION);
    }

    #[test]
    fn interrupted_archive_migration_keeps_host_schema_reopenable() {
        let _guard = crate::test_support::sqlcipher_test_lock();
        let mut connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(MIGRATION_1).unwrap();
        connection.execute_batch(MIGRATION_2).unwrap();
        connection.execute_batch(MIGRATION_3).unwrap();
        connection.execute_batch(MIGRATION_4).unwrap();
        connection.pragma_update(None, "user_version", 4).unwrap();

        let result = migrate_with_hook(&mut connection, |target, _| {
            if target == 5 {
                Err(VaultError::MigrationInterrupted(target))
            } else {
                Ok(())
            }
        });

        assert!(matches!(result, Err(VaultError::MigrationInterrupted(5))));
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 4);
        let table_count: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name = 'archived_evidence'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 0);

        migrate(&mut connection).unwrap();
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, LATEST_SCHEMA_VERSION);
    }
}
