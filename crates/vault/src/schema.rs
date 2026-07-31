use rusqlite::{Connection, TransactionBehavior};

use crate::VaultError;

pub(crate) const LATEST_SCHEMA_VERSION: i64 = 17;

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

const MIGRATION_6: &str = r"
DROP INDEX archived_evidence_object_idx;
ALTER TABLE archived_evidence RENAME TO archived_evidence_v5;

CREATE TABLE archived_evidence (
    id INTEGER PRIMARY KEY CHECK (id > 0),
    source_kind INTEGER NOT NULL CHECK (source_kind = 0),
    source_locator TEXT NOT NULL CHECK (length(source_locator) > 0),
    object_id TEXT NOT NULL CHECK (length(object_id) = 64),
    content_length INTEGER NOT NULL CHECK (content_length >= 0),
    status INTEGER NOT NULL CHECK (status IN (0, 1, 2)),
    unparsed_reason INTEGER CHECK (
        unparsed_reason IS NULL OR unparsed_reason BETWEEN 0 AND 8
    ),
    archived_at INTEGER NOT NULL,
    CHECK (
        (status IN (0, 2) AND unparsed_reason IS NULL)
        OR (status = 1 AND unparsed_reason IS NOT NULL)
    ),
    UNIQUE (source_kind, source_locator, object_id)
) STRICT;

INSERT INTO archived_evidence
    (id, source_kind, source_locator, object_id, content_length,
     status, unparsed_reason, archived_at)
SELECT id, source_kind, source_locator, object_id, content_length,
       status, unparsed_reason, archived_at
FROM archived_evidence_v5;

DROP TABLE archived_evidence_v5;
CREATE INDEX archived_evidence_object_idx ON archived_evidence(object_id);

CREATE TABLE markdown_parse_attempts (
    archive_id INTEGER NOT NULL
        REFERENCES archived_evidence(id) ON DELETE RESTRICT,
    parser_version TEXT NOT NULL CHECK (length(trim(parser_version)) > 0),
    state INTEGER NOT NULL CHECK (state BETWEEN 0 AND 3),
    failure_reason INTEGER CHECK (
        failure_reason IS NULL OR failure_reason BETWEEN 1 AND 8
    ),
    started_at INTEGER NOT NULL,
    finished_at INTEGER,
    PRIMARY KEY (archive_id, parser_version),
    CHECK (
        (state = 0 AND failure_reason IS NULL AND finished_at IS NULL)
        OR (state = 1 AND failure_reason IS NULL AND finished_at IS NOT NULL)
        OR (state = 2 AND failure_reason BETWEEN 1 AND 7 AND finished_at IS NOT NULL)
        OR (state = 3 AND failure_reason = 8 AND finished_at IS NULL)
    )
) STRICT;

CREATE TABLE markdown_parse_artifacts (
    archive_id INTEGER NOT NULL,
    parser_version TEXT NOT NULL,
    parsed_json TEXT NOT NULL CHECK (length(parsed_json) > 0),
    accepted_at INTEGER NOT NULL,
    PRIMARY KEY (archive_id, parser_version),
    FOREIGN KEY (archive_id, parser_version)
        REFERENCES markdown_parse_attempts(archive_id, parser_version)
        ON DELETE RESTRICT
) STRICT;
";

const MIGRATION_7: &str = r"
CREATE TABLE extraction_revisions (
    id INTEGER PRIMARY KEY CHECK (id > 0),
    evidence_id INTEGER NOT NULL
        REFERENCES archived_evidence(id) ON DELETE RESTRICT,
    contract_version TEXT NOT NULL CHECK (length(trim(contract_version)) > 0),
    canonical_digest BLOB NOT NULL CHECK (length(canonical_digest) = 32),
    accepted_at INTEGER NOT NULL,
    UNIQUE (evidence_id, contract_version),
    UNIQUE (id, evidence_id),
    FOREIGN KEY (evidence_id, contract_version)
        REFERENCES markdown_parse_artifacts(archive_id, parser_version)
        ON DELETE RESTRICT
) STRICT;

CREATE TABLE evidence_blocks (
    id INTEGER PRIMARY KEY CHECK (id > 0),
    evidence_id INTEGER NOT NULL,
    extraction_revision_id INTEGER NOT NULL,
    parent_id INTEGER,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    kind INTEGER NOT NULL CHECK (kind BETWEEN 0 AND 12),
    start_byte INTEGER NOT NULL CHECK (start_byte >= 0),
    end_byte INTEGER NOT NULL CHECK (end_byte >= start_byte),
    locator_version TEXT,
    locator_kind INTEGER CHECK (locator_kind IS NULL OR locator_kind IN (0, 1)),
    locator_value TEXT,
    heading_level INTEGER CHECK (heading_level IS NULL OR heading_level BETWEEN 1 AND 6),
    list_start TEXT,
    task_checked INTEGER CHECK (task_checked IS NULL OR task_checked IN (0, 1)),
    info_string TEXT,
    UNIQUE (extraction_revision_id, ordinal),
    UNIQUE (id, extraction_revision_id),
    FOREIGN KEY (extraction_revision_id, evidence_id)
        REFERENCES extraction_revisions(id, evidence_id) ON DELETE RESTRICT,
    FOREIGN KEY (parent_id, extraction_revision_id)
        REFERENCES evidence_blocks(id, extraction_revision_id) ON DELETE RESTRICT,
    CHECK (parent_id IS NULL OR parent_id != id),
    CHECK (
        (locator_version IS NULL AND locator_kind IS NULL AND locator_value IS NULL)
        OR
        (length(trim(locator_version)) > 0 AND locator_kind IS NOT NULL
         AND length(locator_value) > 0)
    )
) STRICT;

CREATE INDEX evidence_blocks_evidence_idx
    ON evidence_blocks(evidence_id, id);

CREATE TRIGGER extraction_revisions_immutable
BEFORE UPDATE ON extraction_revisions
BEGIN
    SELECT RAISE(ABORT, 'extraction revisions are immutable');
END;

CREATE TRIGGER evidence_blocks_immutable
BEFORE UPDATE ON evidence_blocks
BEGIN
    SELECT RAISE(ABORT, 'evidence blocks are immutable');
END;
";

const MIGRATION_8: &str = r"
CREATE TABLE source_records (
    id INTEGER PRIMARY KEY CHECK (id > 0),
    source_kind INTEGER NOT NULL CHECK (source_kind = 0),
    source_locator TEXT NOT NULL CHECK (length(source_locator) > 0),
    UNIQUE (source_kind, source_locator)
) STRICT;

INSERT INTO source_records (id, source_kind, source_locator)
SELECT MIN(id), source_kind, source_locator
FROM archived_evidence
GROUP BY source_kind, source_locator;

CREATE TABLE source_record_versions (
    source_record_id INTEGER NOT NULL
        REFERENCES source_records(id) ON DELETE RESTRICT,
    evidence_id INTEGER NOT NULL UNIQUE
        REFERENCES archived_evidence(id) ON DELETE RESTRICT,
    version_ordinal INTEGER NOT NULL CHECK (version_ordinal >= 0),
    PRIMARY KEY (source_record_id, version_ordinal)
) STRICT;

INSERT INTO source_record_versions (source_record_id, evidence_id, version_ordinal)
SELECT s.id,
       a.id,
       (
           SELECT COUNT(*) - 1
           FROM archived_evidence older
           WHERE older.source_kind = a.source_kind
             AND older.source_locator = a.source_locator
             AND (
                 older.archived_at < a.archived_at
                 OR (older.archived_at = a.archived_at AND older.id <= a.id)
             )
       )
FROM archived_evidence a
JOIN source_records s
  ON s.source_kind = a.source_kind
 AND s.source_locator = a.source_locator;

CREATE UNIQUE INDEX evidence_blocks_id_evidence_unique
    ON evidence_blocks(id, evidence_id);

CREATE TABLE block_lineage_batches (
    id INTEGER PRIMARY KEY CHECK (id > 0),
    source_record_id INTEGER NOT NULL
        REFERENCES source_records(id) ON DELETE RESTRICT,
    from_revision_id INTEGER NOT NULL
        REFERENCES extraction_revisions(id) ON DELETE RESTRICT,
    to_revision_id INTEGER NOT NULL
        REFERENCES extraction_revisions(id) ON DELETE RESTRICT,
    decided_at INTEGER NOT NULL,
    rule_version TEXT NOT NULL CHECK (length(trim(rule_version)) > 0),
    CHECK (from_revision_id != to_revision_id),
    UNIQUE (to_revision_id, rule_version),
    UNIQUE (id, to_revision_id)
) STRICT;

CREATE TABLE block_lineages (
    batch_id INTEGER NOT NULL
        REFERENCES block_lineage_batches(id) ON DELETE RESTRICT,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    from_evidence_id INTEGER NOT NULL,
    from_block_id INTEGER NOT NULL,
    to_evidence_id INTEGER,
    to_block_id INTEGER,
    status INTEGER NOT NULL CHECK (status BETWEEN 0 AND 4),
    basis_kind INTEGER NOT NULL CHECK (basis_kind BETWEEN 0 AND 4),
    similarity_basis_points INTEGER CHECK (
        similarity_basis_points IS NULL
        OR similarity_basis_points BETWEEN 0 AND 10000
    ),
    PRIMARY KEY (batch_id, ordinal),
    UNIQUE (batch_id, from_evidence_id, from_block_id),
    FOREIGN KEY (from_block_id, from_evidence_id)
        REFERENCES evidence_blocks(id, evidence_id) ON DELETE RESTRICT,
    FOREIGN KEY (to_block_id, to_evidence_id)
        REFERENCES evidence_blocks(id, evidence_id) ON DELETE RESTRICT,
    CHECK ((to_evidence_id IS NULL) = (to_block_id IS NULL)),
    CHECK (
        (status IN (0, 1) AND to_block_id IS NOT NULL
         AND basis_kind IN (0, 1) AND similarity_basis_points IS NULL)
        OR (status = 2 AND to_block_id IS NOT NULL
            AND basis_kind IN (0, 2)
            AND ((basis_kind = 0 AND similarity_basis_points IS NULL)
                 OR (basis_kind = 2 AND similarity_basis_points IS NOT NULL)))
        OR (status = 3 AND to_block_id IS NULL
            AND basis_kind = 3 AND similarity_basis_points IS NULL)
        OR (status = 4 AND to_block_id IS NULL
            AND basis_kind = 4 AND similarity_basis_points IS NULL)
    )
) STRICT;

CREATE TABLE block_lineage_candidates (
    batch_id INTEGER NOT NULL,
    lineage_ordinal INTEGER NOT NULL,
    candidate_ordinal INTEGER NOT NULL CHECK (candidate_ordinal >= 0),
    candidate_evidence_id INTEGER NOT NULL,
    candidate_block_id INTEGER NOT NULL,
    PRIMARY KEY (batch_id, lineage_ordinal, candidate_ordinal),
    UNIQUE (batch_id, lineage_ordinal, candidate_evidence_id, candidate_block_id),
    FOREIGN KEY (batch_id, lineage_ordinal)
        REFERENCES block_lineages(batch_id, ordinal) ON DELETE RESTRICT,
    FOREIGN KEY (candidate_block_id, candidate_evidence_id)
        REFERENCES evidence_blocks(id, evidence_id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE incremental_work_items (
    batch_id INTEGER NOT NULL
        REFERENCES block_lineage_batches(id) ON DELETE RESTRICT,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    action INTEGER NOT NULL CHECK (action BETWEEN 0 AND 3),
    from_evidence_id INTEGER,
    from_block_id INTEGER,
    to_evidence_id INTEGER,
    to_block_id INTEGER,
    review_reason INTEGER CHECK (review_reason IS NULL OR review_reason BETWEEN 2 AND 4),
    PRIMARY KEY (batch_id, ordinal),
    FOREIGN KEY (from_block_id, from_evidence_id)
        REFERENCES evidence_blocks(id, evidence_id) ON DELETE RESTRICT,
    FOREIGN KEY (to_block_id, to_evidence_id)
        REFERENCES evidence_blocks(id, evidence_id) ON DELETE RESTRICT,
    CHECK ((from_evidence_id IS NULL) = (from_block_id IS NULL)),
    CHECK ((to_evidence_id IS NULL) = (to_block_id IS NULL)),
    CHECK (
        (action IN (0, 1) AND from_block_id IS NOT NULL
         AND to_block_id IS NOT NULL AND review_reason IS NULL)
        OR (action = 2 AND from_block_id IS NULL
            AND to_block_id IS NOT NULL AND review_reason IS NULL)
        OR (action = 3 AND from_block_id IS NOT NULL
            AND to_block_id IS NULL AND review_reason IS NOT NULL)
    )
) STRICT;

CREATE TRIGGER block_lineage_batches_immutable
BEFORE UPDATE ON block_lineage_batches
BEGIN
    SELECT RAISE(ABORT, 'block lineage batches are immutable');
END;

CREATE TRIGGER block_lineages_immutable
BEFORE UPDATE ON block_lineages
BEGIN
    SELECT RAISE(ABORT, 'block lineages are immutable');
END;

CREATE TRIGGER block_lineage_candidates_immutable
BEFORE UPDATE ON block_lineage_candidates
BEGIN
    SELECT RAISE(ABORT, 'block lineage candidates are immutable');
END;

CREATE TRIGGER incremental_work_items_immutable
BEFORE UPDATE ON incremental_work_items
BEGIN
    SELECT RAISE(ABORT, 'incremental work items are immutable');
END;
";

const MIGRATION_9: &str = r"
CREATE TABLE source_roots (
    id INTEGER PRIMARY KEY CHECK (id > 0),
    root_kind INTEGER NOT NULL CHECK (root_kind = 0),
    root_locator TEXT NOT NULL CHECK (length(trim(root_locator)) > 0),
    availability INTEGER NOT NULL CHECK (availability IN (0, 1)),
    first_seen_at INTEGER NOT NULL,
    last_reconciled_at INTEGER,
    UNIQUE (root_kind, root_locator)
) STRICT;

ALTER TABLE source_records ADD COLUMN origin_kind INTEGER NOT NULL DEFAULT 0
    CHECK (origin_kind IN (0, 1));
ALTER TABLE source_records ADD COLUMN root_id INTEGER
    REFERENCES source_roots(id) ON DELETE RESTRICT;
ALTER TABLE source_records ADD COLUMN current_locator TEXT;
ALTER TABLE source_records ADD COLUMN record_state INTEGER NOT NULL DEFAULT 0
    CHECK (record_state IN (0, 1));
ALTER TABLE source_records ADD COLUMN first_seen_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE source_records ADD COLUMN last_seen_at INTEGER NOT NULL DEFAULT 0;

UPDATE source_records
SET current_locator = source_locator,
    first_seen_at = COALESCE((
        SELECT MIN(a.archived_at)
        FROM source_record_versions v
        JOIN archived_evidence a ON a.id = v.evidence_id
        WHERE v.source_record_id = source_records.id
    ), 0),
    last_seen_at = COALESCE((
        SELECT MAX(a.archived_at)
        FROM source_record_versions v
        JOIN archived_evidence a ON a.id = v.evidence_id
        WHERE v.source_record_id = source_records.id
    ), 0);

CREATE UNIQUE INDEX obsidian_source_records_current_locator_unique
    ON source_records(root_id, current_locator)
    WHERE origin_kind = 1;

CREATE TABLE source_root_state_events (
    root_id INTEGER NOT NULL REFERENCES source_roots(id) ON DELETE RESTRICT,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    availability INTEGER NOT NULL CHECK (availability IN (0, 1)),
    occurred_at INTEGER NOT NULL,
    PRIMARY KEY (root_id, ordinal)
) STRICT;

CREATE TABLE source_record_state_events (
    source_record_id INTEGER NOT NULL
        REFERENCES source_records(id) ON DELETE RESTRICT,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    record_state INTEGER NOT NULL CHECK (record_state IN (0, 1)),
    locator TEXT NOT NULL CHECK (length(locator) > 0),
    occurred_at INTEGER NOT NULL,
    PRIMARY KEY (source_record_id, ordinal)
) STRICT;

CREATE TABLE obsidian_properties (
    evidence_id INTEGER NOT NULL
        REFERENCES archived_evidence(id) ON DELETE RESTRICT,
    property_ordinal INTEGER NOT NULL CHECK (property_ordinal >= 0),
    value_ordinal INTEGER NOT NULL CHECK (value_ordinal >= 0),
    name TEXT NOT NULL CHECK (length(name) > 0),
    value TEXT NOT NULL,
    PRIMARY KEY (evidence_id, property_ordinal, value_ordinal)
) STRICT;

CREATE TABLE obsidian_tags (
    evidence_id INTEGER NOT NULL
        REFERENCES archived_evidence(id) ON DELETE RESTRICT,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    value TEXT NOT NULL CHECK (length(value) > 0),
    PRIMARY KEY (evidence_id, ordinal)
) STRICT;

CREATE TABLE obsidian_aliases (
    evidence_id INTEGER NOT NULL
        REFERENCES archived_evidence(id) ON DELETE RESTRICT,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    value TEXT NOT NULL CHECK (length(value) > 0),
    PRIMARY KEY (evidence_id, ordinal)
) STRICT;

CREATE TABLE obsidian_relations (
    evidence_id INTEGER NOT NULL
        REFERENCES archived_evidence(id) ON DELETE RESTRICT,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    relation_kind INTEGER NOT NULL CHECK (relation_kind BETWEEN 0 AND 4),
    target TEXT NOT NULL,
    alias TEXT,
    heading TEXT,
    block_id TEXT,
    start_byte INTEGER NOT NULL CHECK (start_byte >= 0),
    end_byte INTEGER NOT NULL CHECK (end_byte >= start_byte),
    PRIMARY KEY (evidence_id, ordinal)
) STRICT;

CREATE TABLE obsidian_relation_resolutions (
    evidence_id INTEGER NOT NULL,
    relation_ordinal INTEGER NOT NULL,
    resolved_source_record_id INTEGER NOT NULL
        REFERENCES source_records(id) ON DELETE RESTRICT,
    PRIMARY KEY (evidence_id, relation_ordinal),
    FOREIGN KEY (evidence_id, relation_ordinal)
        REFERENCES obsidian_relations(evidence_id, ordinal) ON DELETE RESTRICT
) STRICT;

CREATE TRIGGER source_root_state_events_immutable
BEFORE UPDATE ON source_root_state_events
BEGIN
    SELECT RAISE(ABORT, 'source root state events are immutable');
END;

CREATE TRIGGER source_record_state_events_immutable
BEFORE UPDATE ON source_record_state_events
BEGIN
    SELECT RAISE(ABORT, 'source record state events are immutable');
END;

CREATE TRIGGER obsidian_properties_immutable
BEFORE UPDATE ON obsidian_properties
BEGIN
    SELECT RAISE(ABORT, 'Obsidian properties are immutable');
END;

CREATE TRIGGER obsidian_tags_immutable
BEFORE UPDATE ON obsidian_tags
BEGIN
    SELECT RAISE(ABORT, 'Obsidian tags are immutable');
END;

CREATE TRIGGER obsidian_aliases_immutable
BEFORE UPDATE ON obsidian_aliases
BEGIN
    SELECT RAISE(ABORT, 'Obsidian aliases are immutable');
END;

CREATE TRIGGER obsidian_relations_immutable
BEFORE UPDATE ON obsidian_relations
BEGIN
    SELECT RAISE(ABORT, 'Obsidian relations are immutable');
END;
";

const MIGRATION_10: &str = r"
CREATE TABLE retrieval_index_metadata (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    contract_version TEXT NOT NULL CHECK (length(trim(contract_version)) > 0),
    authority_digest BLOB NOT NULL CHECK (length(authority_digest) = 32),
    index_digest BLOB NOT NULL CHECK (length(index_digest) = 32),
    built_at INTEGER NOT NULL,
    evidence_block_count INTEGER NOT NULL CHECK (evidence_block_count >= 0),
    ledger_claim_count INTEGER NOT NULL CHECK (ledger_claim_count >= 0),
    relation_count INTEGER NOT NULL CHECK (relation_count >= 0)
) STRICT;

CREATE TABLE retrieval_evidence_availability (
    evidence_id INTEGER PRIMARY KEY
        REFERENCES archived_evidence(id) ON DELETE CASCADE,
    state INTEGER NOT NULL CHECK (state = 1)
) STRICT;

CREATE TABLE retrieval_block_documents (
    evidence_id INTEGER NOT NULL,
    block_id INTEGER NOT NULL,
    source_record_id INTEGER NOT NULL
        REFERENCES source_records(id) ON DELETE CASCADE,
    version_ordinal INTEGER NOT NULL CHECK (version_ordinal >= 0),
    recorded_at INTEGER NOT NULL,
    content_digest BLOB NOT NULL CHECK (length(content_digest) = 32),
    PRIMARY KEY (evidence_id, block_id),
    FOREIGN KEY (block_id, evidence_id)
        REFERENCES evidence_blocks(id, evidence_id) ON DELETE CASCADE
) STRICT;

CREATE TABLE retrieval_block_terms (
    term TEXT NOT NULL CHECK (length(term) > 0),
    evidence_id INTEGER NOT NULL,
    block_id INTEGER NOT NULL,
    PRIMARY KEY (term, evidence_id, block_id),
    FOREIGN KEY (evidence_id, block_id)
        REFERENCES retrieval_block_documents(evidence_id, block_id) ON DELETE CASCADE
) STRICT;

CREATE TABLE retrieval_claim_documents (
    claim_id INTEGER PRIMARY KEY REFERENCES claims(id) ON DELETE CASCADE,
    applicable_start INTEGER,
    applicable_end INTEGER,
    applicable_unknown INTEGER NOT NULL CHECK (applicable_unknown IN (0, 1)),
    recorded_at INTEGER NOT NULL,
    statement_digest BLOB NOT NULL CHECK (length(statement_digest) = 32),
    CHECK (
        (applicable_unknown = 1 AND applicable_start IS NULL AND applicable_end IS NULL)
        OR
        (applicable_unknown = 0 AND applicable_start IS NOT NULL
         AND (applicable_end IS NULL OR applicable_end >= applicable_start))
    )
) STRICT;

CREATE TABLE retrieval_claim_terms (
    term TEXT NOT NULL CHECK (length(term) > 0),
    claim_id INTEGER NOT NULL
        REFERENCES retrieval_claim_documents(claim_id) ON DELETE CASCADE,
    PRIMARY KEY (term, claim_id)
) STRICT;

CREATE TABLE retrieval_entity_terms (
    term TEXT NOT NULL CHECK (length(term) > 0),
    source_record_id INTEGER NOT NULL
        REFERENCES source_records(id) ON DELETE CASCADE,
    PRIMARY KEY (term, source_record_id)
) STRICT;

CREATE TABLE retrieval_relation_edges (
    from_evidence_id INTEGER NOT NULL,
    from_block_id INTEGER NOT NULL,
    relation_ordinal INTEGER NOT NULL CHECK (relation_ordinal >= 0),
    to_source_record_id INTEGER NOT NULL
        REFERENCES source_records(id) ON DELETE CASCADE,
    relation_kind INTEGER NOT NULL CHECK (relation_kind BETWEEN 0 AND 4),
    PRIMARY KEY (from_evidence_id, relation_ordinal),
    FOREIGN KEY (from_evidence_id, from_block_id)
        REFERENCES retrieval_block_documents(evidence_id, block_id) ON DELETE CASCADE
) STRICT;

CREATE INDEX retrieval_block_terms_lookup
    ON retrieval_block_terms(term);
CREATE INDEX retrieval_claim_terms_lookup
    ON retrieval_claim_terms(term);
CREATE INDEX retrieval_entity_terms_lookup
    ON retrieval_entity_terms(term);
CREATE INDEX retrieval_block_time_lookup
    ON retrieval_block_documents(recorded_at);
CREATE INDEX retrieval_claim_time_lookup
    ON retrieval_claim_documents(applicable_start, applicable_end);
CREATE INDEX retrieval_relation_target_lookup
    ON retrieval_relation_edges(to_source_record_id);
";

const MIGRATION_11: &str = r"
CREATE TABLE retrieval_block_vectors (
    evidence_id INTEGER NOT NULL,
    block_id INTEGER NOT NULL,
    model_version TEXT NOT NULL CHECK (length(trim(model_version)) > 0),
    dimensions INTEGER NOT NULL CHECK (dimensions = 256),
    embedding BLOB NOT NULL CHECK (length(embedding) = 512),
    PRIMARY KEY (evidence_id, block_id),
    FOREIGN KEY (evidence_id, block_id)
        REFERENCES retrieval_block_documents(evidence_id, block_id) ON DELETE CASCADE
) STRICT;

CREATE INDEX retrieval_block_vectors_model
    ON retrieval_block_vectors(model_version);
";

const MIGRATION_12: &str = r"
CREATE TABLE understanding_projections (
    id INTEGER PRIMARY KEY CHECK (id > 0),
    contract_version TEXT NOT NULL CHECK (length(trim(contract_version)) > 0),
    trigger_kind INTEGER NOT NULL CHECK (trigger_kind BETWEEN 0 AND 3),
    trigger_detail TEXT NOT NULL CHECK (length(trim(trigger_detail)) > 0),
    recall_count INTEGER CHECK (recall_count IS NULL OR recall_count >= 2),
    projection_kind INTEGER NOT NULL CHECK (projection_kind BETWEEN 0 AND 2),
    subject TEXT NOT NULL CHECK (length(trim(subject)) > 0),
    requested_at INTEGER NOT NULL,
    generation INTEGER NOT NULL CHECK (generation > 0),
    status INTEGER NOT NULL CHECK (status IN (0, 1)),
    material_digest BLOB NOT NULL CHECK (length(material_digest) = 32),
    CHECK (
        (trigger_kind = 1 AND recall_count IS NOT NULL)
        OR
        (trigger_kind != 1 AND recall_count IS NULL)
    )
) STRICT;

CREATE TABLE understanding_projection_sources (
    projection_id INTEGER NOT NULL
        REFERENCES understanding_projections(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    evidence_id INTEGER NOT NULL,
    block_id INTEGER NOT NULL,
    PRIMARY KEY (projection_id, ordinal),
    UNIQUE (projection_id, evidence_id, block_id),
    FOREIGN KEY (block_id, evidence_id)
        REFERENCES evidence_blocks(id, evidence_id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE understanding_projection_statements (
    projection_id INTEGER NOT NULL
        REFERENCES understanding_projections(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    statement TEXT NOT NULL CHECK (length(trim(statement)) > 0),
    PRIMARY KEY (projection_id, ordinal)
) STRICT;

CREATE TABLE understanding_projection_statement_sources (
    projection_id INTEGER NOT NULL,
    statement_ordinal INTEGER NOT NULL,
    source_ordinal INTEGER NOT NULL,
    PRIMARY KEY (projection_id, statement_ordinal, source_ordinal),
    FOREIGN KEY (projection_id, statement_ordinal)
        REFERENCES understanding_projection_statements(projection_id, ordinal)
        ON DELETE CASCADE,
    FOREIGN KEY (projection_id, source_ordinal)
        REFERENCES understanding_projection_sources(projection_id, ordinal)
        ON DELETE CASCADE
) STRICT;

CREATE TABLE understanding_projection_artifacts (
    projection_id INTEGER PRIMARY KEY
        REFERENCES understanding_projections(id) ON DELETE CASCADE,
    contract_version TEXT NOT NULL CHECK (length(trim(contract_version)) > 0),
    material_digest BLOB NOT NULL CHECK (length(material_digest) = 32),
    built_at INTEGER NOT NULL
) STRICT;

CREATE TABLE understanding_projection_terms (
    projection_id INTEGER NOT NULL
        REFERENCES understanding_projection_artifacts(projection_id) ON DELETE CASCADE,
    term TEXT NOT NULL CHECK (length(term) > 0),
    PRIMARY KEY (projection_id, term)
) STRICT;

CREATE TABLE understanding_projection_events (
    projection_id INTEGER NOT NULL
        REFERENCES understanding_projections(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    status INTEGER NOT NULL CHECK (status IN (0, 1)),
    reason_evidence_id INTEGER,
    reason_block_id INTEGER,
    occurred_at INTEGER NOT NULL,
    PRIMARY KEY (projection_id, ordinal),
    FOREIGN KEY (reason_block_id, reason_evidence_id)
        REFERENCES evidence_blocks(id, evidence_id) ON DELETE RESTRICT,
    CHECK (
        (reason_evidence_id IS NULL AND reason_block_id IS NULL)
        OR
        (reason_evidence_id IS NOT NULL AND reason_block_id IS NOT NULL)
    )
) STRICT;

CREATE INDEX understanding_projection_terms_lookup
    ON understanding_projection_terms(term);
CREATE INDEX understanding_projection_sources_lookup
    ON understanding_projection_sources(evidence_id, block_id);
";

const MIGRATION_13: &str = r"
CREATE TABLE long_term_memories (
    id INTEGER PRIMARY KEY CHECK (id > 0),
    created_at INTEGER NOT NULL
) STRICT;

CREATE TABLE long_term_memory_versions (
    memory_id INTEGER NOT NULL
        REFERENCES long_term_memories(id) ON DELETE RESTRICT,
    version INTEGER NOT NULL CHECK (version > 0),
    predecessor_version INTEGER,
    subject INTEGER NOT NULL CHECK (subject BETWEEN 0 AND 2),
    kind INTEGER NOT NULL CHECK (kind BETWEEN 0 AND 4),
    statement TEXT NOT NULL CHECK (length(trim(statement)) > 0),
    confidence INTEGER NOT NULL CHECK (confidence BETWEEN 0 AND 2),
    applicable_kind INTEGER NOT NULL CHECK (applicable_kind BETWEEN 0 AND 3),
    applicable_start INTEGER,
    applicable_end INTEGER,
    salience_reason TEXT NOT NULL CHECK (length(trim(salience_reason)) > 0),
    basis INTEGER NOT NULL CHECK (basis BETWEEN 0 AND 2),
    formed_at INTEGER NOT NULL,
    PRIMARY KEY (memory_id, version),
    FOREIGN KEY (memory_id, predecessor_version)
        REFERENCES long_term_memory_versions(memory_id, version) ON DELETE RESTRICT,
    CHECK (
        (version = 1 AND predecessor_version IS NULL)
        OR
        (version > 1 AND predecessor_version = version - 1)
    ),
    CHECK (
        (applicable_kind = 0 AND applicable_start IS NOT NULL
         AND applicable_end IS NULL)
        OR
        (applicable_kind = 1 AND applicable_start IS NOT NULL
         AND applicable_end IS NULL)
        OR
        (applicable_kind = 2 AND applicable_start IS NOT NULL
         AND applicable_end IS NOT NULL AND applicable_end >= applicable_start)
        OR
        (applicable_kind = 3 AND applicable_start IS NULL
         AND applicable_end IS NULL)
    )
) STRICT;

CREATE TABLE long_term_memory_sources (
    memory_id INTEGER NOT NULL,
    version INTEGER NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    claim_id INTEGER NOT NULL REFERENCES claims(id) ON DELETE RESTRICT,
    PRIMARY KEY (memory_id, version, ordinal),
    UNIQUE (memory_id, version, claim_id),
    FOREIGN KEY (memory_id, version)
        REFERENCES long_term_memory_versions(memory_id, version) ON DELETE CASCADE
) STRICT;

CREATE TABLE long_term_memory_state_events (
    memory_id INTEGER NOT NULL,
    version INTEGER NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    status INTEGER NOT NULL CHECK (status BETWEEN 0 AND 3),
    occurred_at INTEGER NOT NULL,
    PRIMARY KEY (memory_id, version, ordinal),
    FOREIGN KEY (memory_id, version)
        REFERENCES long_term_memory_versions(memory_id, version) ON DELETE CASCADE
) STRICT;

CREATE TABLE long_term_memory_terms (
    memory_id INTEGER NOT NULL,
    version INTEGER NOT NULL,
    term TEXT NOT NULL CHECK (length(term) > 0),
    PRIMARY KEY (memory_id, version, term),
    FOREIGN KEY (memory_id, version)
        REFERENCES long_term_memory_versions(memory_id, version) ON DELETE CASCADE
) STRICT;

CREATE INDEX long_term_memory_terms_lookup ON long_term_memory_terms(term);
CREATE INDEX long_term_memory_sources_claim
    ON long_term_memory_sources(claim_id);
";

const MIGRATION_14: &str = r"
ALTER TABLE long_term_memory_state_events
    RENAME TO long_term_memory_state_events_v13;

CREATE TABLE long_term_memory_state_events (
    memory_id INTEGER NOT NULL,
    version INTEGER NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    status INTEGER NOT NULL CHECK (status BETWEEN 0 AND 5),
    occurred_at INTEGER NOT NULL,
    PRIMARY KEY (memory_id, version, ordinal),
    FOREIGN KEY (memory_id, version)
        REFERENCES long_term_memory_versions(memory_id, version) ON DELETE CASCADE
) STRICT;

INSERT INTO long_term_memory_state_events
    (memory_id, version, ordinal, status, occurred_at)
SELECT memory_id, version, ordinal, status, occurred_at
FROM long_term_memory_state_events_v13;

DROP TABLE long_term_memory_state_events_v13;

CREATE TABLE memory_disputes (
    id INTEGER PRIMARY KEY CHECK (id > 0),
    memory_id INTEGER NOT NULL,
    memory_version INTEGER NOT NULL CHECK (memory_version > 0),
    reason TEXT NOT NULL CHECK (length(trim(reason)) > 0),
    raised_at INTEGER NOT NULL,
    outcome INTEGER NOT NULL CHECK (outcome BETWEEN 0 AND 3),
    reviewed_at INTEGER,
    review_rationale TEXT,
    revised_version INTEGER,
    FOREIGN KEY (memory_id, memory_version)
        REFERENCES long_term_memory_versions(memory_id, version) ON DELETE RESTRICT,
    FOREIGN KEY (memory_id, revised_version)
        REFERENCES long_term_memory_versions(memory_id, version) ON DELETE RESTRICT,
    CHECK (
        (outcome = 0 AND reviewed_at IS NULL AND review_rationale IS NULL
                     AND revised_version IS NULL)
        OR
        (outcome BETWEEN 1 AND 3 AND reviewed_at IS NOT NULL
                                 AND length(trim(review_rationale)) > 0)
    ),
    CHECK (
        (outcome = 2 AND revised_version IS NOT NULL)
        OR
        (outcome <> 2 AND revised_version IS NULL)
    )
) STRICT;

CREATE UNIQUE INDEX one_open_memory_dispute
    ON memory_disputes(memory_id) WHERE outcome = 0;

CREATE TABLE memory_dispute_counter_evidence (
    dispute_id INTEGER NOT NULL REFERENCES memory_disputes(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    evidence_id INTEGER NOT NULL REFERENCES conversation_evidence(id) ON DELETE RESTRICT,
    quote TEXT NOT NULL CHECK (length(trim(quote)) > 0),
    PRIMARY KEY (dispute_id, ordinal),
    UNIQUE (dispute_id, evidence_id)
) STRICT;

CREATE TABLE memory_dispute_review_evidence (
    dispute_id INTEGER NOT NULL REFERENCES memory_disputes(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    evidence_id INTEGER NOT NULL REFERENCES conversation_evidence(id) ON DELETE RESTRICT,
    quote TEXT NOT NULL CHECK (length(trim(quote)) > 0),
    PRIMARY KEY (dispute_id, ordinal),
    UNIQUE (dispute_id, evidence_id)
) STRICT;

CREATE TABLE memory_dispute_terms (
    dispute_id INTEGER NOT NULL REFERENCES memory_disputes(id) ON DELETE CASCADE,
    term TEXT NOT NULL CHECK (length(term) > 0),
    PRIMARY KEY (dispute_id, term)
) STRICT;

CREATE INDEX memory_dispute_terms_lookup ON memory_dispute_terms(term);
CREATE INDEX memory_disputes_memory ON memory_disputes(memory_id, memory_version);
";

const MIGRATION_15: &str = r"
ALTER TABLE claims ADD COLUMN supersedes_claim_id INTEGER
    REFERENCES claims(id) ON DELETE RESTRICT;

CREATE UNIQUE INDEX claims_single_successor
    ON claims(supersedes_claim_id) WHERE supersedes_claim_id IS NOT NULL;

DROP INDEX one_open_memory_dispute;
CREATE UNIQUE INDEX one_open_memory_dispute
    ON memory_disputes(memory_id, memory_version) WHERE outcome = 0;

CREATE TABLE claim_state_events (
    claim_id INTEGER NOT NULL REFERENCES claims(id) ON DELETE RESTRICT,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    status INTEGER NOT NULL CHECK (status IN (0, 1)),
    caused_by_claim_id INTEGER REFERENCES claims(id) ON DELETE RESTRICT,
    occurred_at INTEGER NOT NULL,
    PRIMARY KEY (claim_id, ordinal),
    CHECK (
        (status = 0 AND caused_by_claim_id IS NULL)
        OR
        (status = 1 AND caused_by_claim_id IS NOT NULL
                    AND caused_by_claim_id <> claim_id)
    )
) STRICT;

INSERT INTO claim_state_events (claim_id, ordinal, status, caused_by_claim_id, occurred_at)
SELECT id, 0, 0, NULL, recorded_at FROM claims;

CREATE INDEX claim_state_events_status ON claim_state_events(status, claim_id);

CREATE TRIGGER claim_state_events_immutable
BEFORE UPDATE ON claim_state_events
BEGIN
    SELECT RAISE(ABORT, 'claim state events are immutable');
END;

CREATE TABLE claim_correction_memory_work_items (
    correction_claim_id INTEGER NOT NULL REFERENCES claims(id) ON DELETE RESTRICT,
    memory_id INTEGER NOT NULL,
    affected_version INTEGER NOT NULL CHECK (affected_version > 0),
    action INTEGER NOT NULL CHECK (action IN (0, 1)),
    rebuilt_version INTEGER,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (correction_claim_id, memory_id, affected_version),
    FOREIGN KEY (memory_id, affected_version)
        REFERENCES long_term_memory_versions(memory_id, version) ON DELETE RESTRICT,
    FOREIGN KEY (memory_id, rebuilt_version)
        REFERENCES long_term_memory_versions(memory_id, version) ON DELETE RESTRICT,
    CHECK (
        (action = 0 AND rebuilt_version = affected_version + 1)
        OR
        (action = 1 AND rebuilt_version IS NULL)
    )
) STRICT;

ALTER TABLE retrieval_claim_documents ADD COLUMN claim_status INTEGER
    NOT NULL DEFAULT 0 CHECK (claim_status IN (0, 1));

CREATE INDEX retrieval_claim_status_lookup
    ON retrieval_claim_documents(claim_status, claim_id);
";

const MIGRATION_16: &str = r"
CREATE TABLE deletion_intents (
    id INTEGER PRIMARY KEY CHECK (id > 0),
    target_kind INTEGER NOT NULL CHECK (target_kind IN (0, 1)),
    target_id INTEGER NOT NULL CHECK (target_id > 0),
    requested_at INTEGER NOT NULL,
    removed_authority_records INTEGER NOT NULL
        CHECK (removed_authority_records >= 0),
    removed_derived_records INTEGER NOT NULL
        CHECK (removed_derived_records >= 0),
    released_object_references INTEGER NOT NULL
        CHECK (released_object_references >= 0),
    UNIQUE (target_kind, target_id)
) STRICT;

CREATE INDEX deletion_intents_target
    ON deletion_intents(target_kind, target_id);
";

const MIGRATION_17: &str = r"
CREATE TABLE shared_agreement_candidates (
    id INTEGER PRIMARY KEY CHECK (id > 0),
    statement TEXT NOT NULL CHECK (length(trim(statement)) > 0),
    occurred_at INTEGER NOT NULL,
    recorded_at INTEGER NOT NULL,
    status INTEGER NOT NULL CHECK (status IN (0, 1, 2)),
    decided_at INTEGER,
    confirmed_claim_id INTEGER UNIQUE
        REFERENCES claims(id) ON DELETE RESTRICT,
    CHECK (
        (status = 0 AND decided_at IS NULL AND confirmed_claim_id IS NULL)
        OR
        (status = 1 AND decided_at IS NOT NULL AND confirmed_claim_id IS NULL)
        OR
        (status = 2 AND decided_at IS NOT NULL AND confirmed_claim_id IS NOT NULL)
    )
) STRICT;

CREATE TABLE shared_agreement_candidate_support (
    candidate_id INTEGER NOT NULL
        REFERENCES shared_agreement_candidates(id) ON DELETE RESTRICT,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    evidence_id INTEGER NOT NULL
        REFERENCES conversation_evidence(id) ON DELETE RESTRICT,
    quote TEXT NOT NULL CHECK (length(quote) > 0),
    PRIMARY KEY (candidate_id, ordinal)
) STRICT;

CREATE INDEX shared_agreement_candidate_support_evidence
    ON shared_agreement_candidate_support(evidence_id);

CREATE TABLE shared_experiences (
    claim_id INTEGER PRIMARY KEY REFERENCES claims(id) ON DELETE RESTRICT,
    kind INTEGER NOT NULL CHECK (kind IN (0, 1, 2, 3)),
    candidate_id INTEGER UNIQUE
        REFERENCES shared_agreement_candidates(id) ON DELETE RESTRICT,
    ceremony_dismissed INTEGER NOT NULL DEFAULT 0
        CHECK (ceremony_dismissed IN (0, 1)),
    CHECK (
        (kind = 0 AND candidate_id IS NOT NULL)
        OR
        (kind <> 0 AND candidate_id IS NULL)
    )
) STRICT;
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
            6 => transaction.execute_batch(MIGRATION_6)?,
            7 => transaction.execute_batch(MIGRATION_7)?,
            8 => transaction.execute_batch(MIGRATION_8)?,
            9 => transaction.execute_batch(MIGRATION_9)?,
            10 => transaction.execute_batch(MIGRATION_10)?,
            11 => transaction.execute_batch(MIGRATION_11)?,
            12 => transaction.execute_batch(MIGRATION_12)?,
            13 => transaction.execute_batch(MIGRATION_13)?,
            14 => transaction.execute_batch(MIGRATION_14)?,
            15 => transaction.execute_batch(MIGRATION_15)?,
            16 => transaction.execute_batch(MIGRATION_16)?,
            17 => transaction.execute_batch(MIGRATION_17)?,
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

    #[test]
    fn interrupted_markdown_migration_keeps_archive_schema_reopenable() {
        let _guard = crate::test_support::sqlcipher_test_lock();
        let mut connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(MIGRATION_1).unwrap();
        connection.execute_batch(MIGRATION_2).unwrap();
        connection.execute_batch(MIGRATION_3).unwrap();
        connection.execute_batch(MIGRATION_4).unwrap();
        connection.execute_batch(MIGRATION_5).unwrap();
        connection.pragma_update(None, "user_version", 5).unwrap();

        let result = migrate_with_hook(&mut connection, |target, _| {
            if target == 6 {
                Err(VaultError::MigrationInterrupted(target))
            } else {
                Ok(())
            }
        });

        assert!(matches!(result, Err(VaultError::MigrationInterrupted(6))));
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 5);
        let archive_table_count: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name = 'archived_evidence'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let attempt_table_count: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name = 'markdown_parse_attempts'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(archive_table_count, 1);
        assert_eq!(attempt_table_count, 0);

        migrate(&mut connection).unwrap();
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, LATEST_SCHEMA_VERSION);
    }

    #[test]
    fn interrupted_extraction_migration_keeps_markdown_schema_reopenable() {
        let _guard = crate::test_support::sqlcipher_test_lock();
        let mut connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(MIGRATION_1).unwrap();
        connection.execute_batch(MIGRATION_2).unwrap();
        connection.execute_batch(MIGRATION_3).unwrap();
        connection.execute_batch(MIGRATION_4).unwrap();
        connection.execute_batch(MIGRATION_5).unwrap();
        connection.execute_batch(MIGRATION_6).unwrap();
        connection.pragma_update(None, "user_version", 6).unwrap();

        let result = migrate_with_hook(&mut connection, |target, _| {
            if target == 7 {
                Err(VaultError::MigrationInterrupted(target))
            } else {
                Ok(())
            }
        });

        assert!(matches!(result, Err(VaultError::MigrationInterrupted(7))));
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 6);
        let revision_table_count: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name = 'extraction_revisions'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(revision_table_count, 0);

        migrate(&mut connection).unwrap();
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, LATEST_SCHEMA_VERSION);
    }

    #[test]
    fn interrupted_lineage_migration_keeps_extraction_schema_reopenable() {
        let _guard = crate::test_support::sqlcipher_test_lock();
        let mut connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(MIGRATION_1).unwrap();
        connection.execute_batch(MIGRATION_2).unwrap();
        connection.execute_batch(MIGRATION_3).unwrap();
        connection.execute_batch(MIGRATION_4).unwrap();
        connection.execute_batch(MIGRATION_5).unwrap();
        connection.execute_batch(MIGRATION_6).unwrap();
        connection.execute_batch(MIGRATION_7).unwrap();
        connection.pragma_update(None, "user_version", 7).unwrap();

        let result = migrate_with_hook(&mut connection, |target, _| {
            if target == 8 {
                Err(VaultError::MigrationInterrupted(target))
            } else {
                Ok(())
            }
        });

        assert!(matches!(result, Err(VaultError::MigrationInterrupted(8))));
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 7);
        let lineage_table_count: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name = 'block_lineages'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(lineage_table_count, 0);

        migrate(&mut connection).unwrap();
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, LATEST_SCHEMA_VERSION);
    }

    #[test]
    fn lineage_migration_backfills_stable_sources_and_version_order() {
        let _guard = crate::test_support::sqlcipher_test_lock();
        let mut connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(MIGRATION_1).unwrap();
        connection.execute_batch(MIGRATION_2).unwrap();
        connection.execute_batch(MIGRATION_3).unwrap();
        connection.execute_batch(MIGRATION_4).unwrap();
        connection.execute_batch(MIGRATION_5).unwrap();
        connection.execute_batch(MIGRATION_6).unwrap();
        connection.execute_batch(MIGRATION_7).unwrap();
        connection
            .execute(
                "INSERT INTO archived_evidence
                 (id, source_kind, source_locator, object_id, content_length,
                  status, unparsed_reason, archived_at)
                 VALUES (1, 0, 'inbox/same.md', ?1, 1, 0, NULL, 10),
                        (2, 0, 'inbox/same.md', ?2, 1, 0, NULL, 20),
                        (3, 0, 'inbox/other.md', ?3, 1, 0, NULL, 15)",
                [
                    format!("{:064x}", 1),
                    format!("{:064x}", 2),
                    format!("{:064x}", 3),
                ],
            )
            .unwrap();
        connection.pragma_update(None, "user_version", 7).unwrap();

        migrate(&mut connection).unwrap();

        let source_count: i64 = connection
            .query_row("SELECT count(*) FROM source_records", [], |row| row.get(0))
            .unwrap();
        assert_eq!(source_count, 2);
        let versions = connection
            .prepare(
                "SELECT v.evidence_id, v.version_ordinal
                 FROM source_record_versions v
                 JOIN source_records s ON s.id = v.source_record_id
                 WHERE s.source_locator = 'inbox/same.md'
                 ORDER BY v.version_ordinal",
            )
            .unwrap()
            .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(versions, vec![(1, 0), (2, 1)]);
    }

    #[test]
    fn interrupted_obsidian_migration_keeps_lineage_schema_reopenable() {
        let _guard = crate::test_support::sqlcipher_test_lock();
        let mut connection = Connection::open_in_memory().unwrap();
        for migration in [
            MIGRATION_1,
            MIGRATION_2,
            MIGRATION_3,
            MIGRATION_4,
            MIGRATION_5,
            MIGRATION_6,
            MIGRATION_7,
            MIGRATION_8,
        ] {
            connection.execute_batch(migration).unwrap();
        }
        connection.pragma_update(None, "user_version", 8).unwrap();

        let result = migrate_with_hook(&mut connection, |target, _| {
            if target == 9 {
                Err(VaultError::MigrationInterrupted(target))
            } else {
                Ok(())
            }
        });

        assert!(matches!(result, Err(VaultError::MigrationInterrupted(9))));
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 8);
        let root_table_count: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name = 'source_roots'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(root_table_count, 0);

        migrate(&mut connection).unwrap();
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, LATEST_SCHEMA_VERSION);
    }

    #[test]
    fn interrupted_retrieval_migration_keeps_obsidian_schema_reopenable() {
        let _guard = crate::test_support::sqlcipher_test_lock();
        let mut connection = Connection::open_in_memory().unwrap();
        for migration in [
            MIGRATION_1,
            MIGRATION_2,
            MIGRATION_3,
            MIGRATION_4,
            MIGRATION_5,
            MIGRATION_6,
            MIGRATION_7,
            MIGRATION_8,
            MIGRATION_9,
        ] {
            connection.execute_batch(migration).unwrap();
        }
        connection.pragma_update(None, "user_version", 9).unwrap();

        let result = migrate_with_hook(&mut connection, |target, _| {
            if target == 10 {
                Err(VaultError::MigrationInterrupted(target))
            } else {
                Ok(())
            }
        });

        assert!(matches!(result, Err(VaultError::MigrationInterrupted(10))));
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 9);
        let table_count: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name = 'retrieval_index_metadata'",
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
    fn interrupted_vector_migration_keeps_retrieval_schema_reopenable() {
        let _guard = crate::test_support::sqlcipher_test_lock();
        let mut connection = Connection::open_in_memory().unwrap();
        for migration in [
            MIGRATION_1,
            MIGRATION_2,
            MIGRATION_3,
            MIGRATION_4,
            MIGRATION_5,
            MIGRATION_6,
            MIGRATION_7,
            MIGRATION_8,
            MIGRATION_9,
            MIGRATION_10,
        ] {
            connection.execute_batch(migration).unwrap();
        }
        connection.pragma_update(None, "user_version", 10).unwrap();

        let result = migrate_with_hook(&mut connection, |target, _| {
            if target == 11 {
                Err(VaultError::MigrationInterrupted(target))
            } else {
                Ok(())
            }
        });

        assert!(matches!(result, Err(VaultError::MigrationInterrupted(11))));
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 10);
        let table_count: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name = 'retrieval_block_vectors'",
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
    fn interrupted_understanding_migration_keeps_vector_schema_reopenable() {
        let _guard = crate::test_support::sqlcipher_test_lock();
        let mut connection = Connection::open_in_memory().unwrap();
        for migration in [
            MIGRATION_1,
            MIGRATION_2,
            MIGRATION_3,
            MIGRATION_4,
            MIGRATION_5,
            MIGRATION_6,
            MIGRATION_7,
            MIGRATION_8,
            MIGRATION_9,
            MIGRATION_10,
            MIGRATION_11,
        ] {
            connection.execute_batch(migration).unwrap();
        }
        connection.pragma_update(None, "user_version", 11).unwrap();

        let result = migrate_with_hook(&mut connection, |target, _| {
            if target == 12 {
                Err(VaultError::MigrationInterrupted(target))
            } else {
                Ok(())
            }
        });

        assert!(matches!(result, Err(VaultError::MigrationInterrupted(12))));
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 11);
        let table_count: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name = 'understanding_projections'",
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
    fn interrupted_memory_migration_keeps_understanding_schema_reopenable() {
        let _guard = crate::test_support::sqlcipher_test_lock();
        let mut connection = Connection::open_in_memory().unwrap();
        for migration in [
            MIGRATION_1,
            MIGRATION_2,
            MIGRATION_3,
            MIGRATION_4,
            MIGRATION_5,
            MIGRATION_6,
            MIGRATION_7,
            MIGRATION_8,
            MIGRATION_9,
            MIGRATION_10,
            MIGRATION_11,
            MIGRATION_12,
        ] {
            connection.execute_batch(migration).unwrap();
        }
        connection.pragma_update(None, "user_version", 12).unwrap();

        let result = migrate_with_hook(&mut connection, |target, _| {
            if target == 13 {
                Err(VaultError::MigrationInterrupted(target))
            } else {
                Ok(())
            }
        });

        assert!(matches!(result, Err(VaultError::MigrationInterrupted(13))));
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 12);
        let table_count: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name = 'long_term_memories'",
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
    fn interrupted_dispute_migration_keeps_memory_schema_reopenable() {
        let _guard = crate::test_support::sqlcipher_test_lock();
        let mut connection = Connection::open_in_memory().unwrap();
        for migration in [
            MIGRATION_1,
            MIGRATION_2,
            MIGRATION_3,
            MIGRATION_4,
            MIGRATION_5,
            MIGRATION_6,
            MIGRATION_7,
            MIGRATION_8,
            MIGRATION_9,
            MIGRATION_10,
            MIGRATION_11,
            MIGRATION_12,
            MIGRATION_13,
        ] {
            connection.execute_batch(migration).unwrap();
        }
        connection.pragma_update(None, "user_version", 13).unwrap();

        let result = migrate_with_hook(&mut connection, |target, _| {
            if target == 14 {
                Err(VaultError::MigrationInterrupted(target))
            } else {
                Ok(())
            }
        });

        assert!(matches!(result, Err(VaultError::MigrationInterrupted(14))));
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 13);
        let table_count: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name = 'memory_disputes'",
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
    fn interrupted_claim_correction_migration_keeps_dispute_schema_reopenable() {
        let _guard = crate::test_support::sqlcipher_test_lock();
        let mut connection = Connection::open_in_memory().unwrap();
        for migration in [
            MIGRATION_1,
            MIGRATION_2,
            MIGRATION_3,
            MIGRATION_4,
            MIGRATION_5,
            MIGRATION_6,
            MIGRATION_7,
            MIGRATION_8,
            MIGRATION_9,
            MIGRATION_10,
            MIGRATION_11,
            MIGRATION_12,
            MIGRATION_13,
            MIGRATION_14,
        ] {
            connection.execute_batch(migration).unwrap();
        }
        connection.pragma_update(None, "user_version", 14).unwrap();

        let result = migrate_with_hook(&mut connection, |target, _| {
            if target == 15 {
                Err(VaultError::MigrationInterrupted(target))
            } else {
                Ok(())
            }
        });

        assert!(matches!(result, Err(VaultError::MigrationInterrupted(15))));
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 14);
        let table_count: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name = 'claim_state_events'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 0);
        let claim_columns: i64 = connection
            .query_row(
                "SELECT count(*) FROM pragma_table_info('claims')
                 WHERE name = 'supersedes_claim_id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(claim_columns, 0);

        migrate(&mut connection).unwrap();
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, LATEST_SCHEMA_VERSION);
    }

    #[test]
    fn claim_correction_migration_backfills_existing_claim_and_retrieval_state() {
        let _guard = crate::test_support::sqlcipher_test_lock();
        let mut connection = Connection::open_in_memory().unwrap();
        for migration in [
            MIGRATION_1,
            MIGRATION_2,
            MIGRATION_3,
            MIGRATION_4,
            MIGRATION_5,
            MIGRATION_6,
            MIGRATION_7,
            MIGRATION_8,
            MIGRATION_9,
            MIGRATION_10,
            MIGRATION_11,
            MIGRATION_12,
            MIGRATION_13,
            MIGRATION_14,
        ] {
            connection.execute_batch(migration).unwrap();
        }
        connection
            .execute(
                "INSERT INTO conversation_evidence
                 (id, session_id, speaker, verbatim, recorded_at)
                 VALUES (1, 'migration', 0, 'I live in Shenzhen', 123)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO claims
                 (id, owner, statement, uncertainty, applicable_kind,
                  applicable_start, applicable_end, recorded_at)
                 VALUES (1, 0, 'I live in Shenzhen', NULL, 3, NULL, NULL, 123)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO claim_support (claim_id, ordinal, evidence_id, quote)
                 VALUES (1, 0, 1, 'I live in Shenzhen')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO retrieval_claim_documents
                 (claim_id, applicable_start, applicable_end, applicable_unknown,
                  recorded_at, statement_digest)
                 VALUES (1, NULL, NULL, 1, 123, ?1)",
                [vec![0_u8; 32]],
            )
            .unwrap();
        connection.pragma_update(None, "user_version", 14).unwrap();

        migrate(&mut connection).unwrap();

        let claim_state: (i64, Option<i64>, i64) = connection
            .query_row(
                "SELECT status, caused_by_claim_id, occurred_at
                 FROM claim_state_events WHERE claim_id = 1 AND ordinal = 0",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(claim_state, (0, None, 123));
        let retrieval_status: i64 = connection
            .query_row(
                "SELECT claim_status FROM retrieval_claim_documents WHERE claim_id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(retrieval_status, 0);
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, LATEST_SCHEMA_VERSION);
    }

    #[test]
    fn interrupted_forget_migration_keeps_claim_correction_schema_reopenable() {
        let _guard = crate::test_support::sqlcipher_test_lock();
        let mut connection = Connection::open_in_memory().unwrap();
        for migration in [
            MIGRATION_1,
            MIGRATION_2,
            MIGRATION_3,
            MIGRATION_4,
            MIGRATION_5,
            MIGRATION_6,
            MIGRATION_7,
            MIGRATION_8,
            MIGRATION_9,
            MIGRATION_10,
            MIGRATION_11,
            MIGRATION_12,
            MIGRATION_13,
            MIGRATION_14,
            MIGRATION_15,
        ] {
            connection.execute_batch(migration).unwrap();
        }
        connection.pragma_update(None, "user_version", 15).unwrap();

        let result = migrate_with_hook(&mut connection, |target, _| {
            if target == 16 {
                Err(VaultError::MigrationInterrupted(target))
            } else {
                Ok(())
            }
        });

        assert!(matches!(result, Err(VaultError::MigrationInterrupted(16))));
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 15);
        let table_count: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name = 'deletion_intents'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 0);

        migrate(&mut connection).unwrap();
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            LATEST_SCHEMA_VERSION
        );
    }

    #[test]
    fn interrupted_shared_experience_migration_keeps_forget_schema_reopenable() {
        let _guard = crate::test_support::sqlcipher_test_lock();
        let mut connection = Connection::open_in_memory().unwrap();
        for migration in [
            MIGRATION_1,
            MIGRATION_2,
            MIGRATION_3,
            MIGRATION_4,
            MIGRATION_5,
            MIGRATION_6,
            MIGRATION_7,
            MIGRATION_8,
            MIGRATION_9,
            MIGRATION_10,
            MIGRATION_11,
            MIGRATION_12,
            MIGRATION_13,
            MIGRATION_14,
            MIGRATION_15,
            MIGRATION_16,
        ] {
            connection.execute_batch(migration).unwrap();
        }
        connection.pragma_update(None, "user_version", 16).unwrap();

        let result = migrate_with_hook(&mut connection, |target, _| {
            if target == 17 {
                Err(VaultError::MigrationInterrupted(target))
            } else {
                Ok(())
            }
        });

        assert!(matches!(result, Err(VaultError::MigrationInterrupted(17))));
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            16
        );
        let table_count: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema
                 WHERE name IN ('shared_agreement_candidates', 'shared_experiences')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 0);

        migrate(&mut connection).unwrap();
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            LATEST_SCHEMA_VERSION
        );
    }
}
