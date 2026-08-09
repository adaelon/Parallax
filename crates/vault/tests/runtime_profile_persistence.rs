use std::fmt::Write as _;

use eam_vault::{
    DEFAULT_RUNTIME_BASE_URL, DEFAULT_RUNTIME_MODEL, RuntimeProfileKeyAction, VaultError, VaultKey,
    VaultRepository,
};
use hkdf::Hkdf;
use rusqlite::Connection;
use sha2::Sha256;
use tempfile::tempdir;

const KDF_SALT: &[u8] = b"evrything-about-me/v1/vault-subkeys";
const DATABASE_INFO: &[u8] = b"database";

#[test]
fn v25_reopens_through_v28_with_one_default_runtime_profile() {
    let directory = tempdir().unwrap();
    let key = [0x26; 32];
    let repository = VaultRepository::open(directory.path(), VaultKey::new(key)).unwrap();
    assert_eq!(repository.schema_version().unwrap(), 28);
    let database_path = repository.database_path().to_path_buf();
    repository.close().unwrap();

    let connection = Connection::open(&database_path).unwrap();
    key_sqlcipher_connection(&connection, key);
    connection
        .execute_batch(
            "DROP TRIGGER source_root_lifecycle_events_immutable_update;
             DROP TRIGGER source_root_lifecycle_events_immutable_delete;
             DROP TABLE source_root_lifecycle_events;
             DROP INDEX source_roots_single_active;
             ALTER TABLE source_roots DROP COLUMN lifecycle_state;
             DROP TABLE runtime_profiles;
             DROP INDEX conversation_evidence_counterpart_identity;
             ALTER TABLE conversation_evidence DROP COLUMN counterpart_identity_version;
             PRAGMA user_version = 25;",
        )
        .unwrap();
    connection.close().unwrap();

    let repository = VaultRepository::open(directory.path(), VaultKey::new(key)).unwrap();
    assert_eq!(repository.schema_version().unwrap(), 28);
    let profile = repository.runtime_profile().unwrap();
    assert_eq!(profile.base_url(), DEFAULT_RUNTIME_BASE_URL);
    assert_eq!(profile.model(), DEFAULT_RUNTIME_MODEL);
    assert_eq!(profile.bearer_key(), None);
    let view = repository.runtime_profile_view().unwrap();
    assert!(!view.api_key_configured());
    assert_eq!(view.api_key_last_four(), None);
}

#[test]
fn replace_keep_and_clear_are_durable_while_the_view_stays_redacted() {
    let directory = tempdir().unwrap();
    let key = [0x36; 32];
    let secret = "synthetic-runtime-profile-secret-9876";
    let mut repository = VaultRepository::open(directory.path(), VaultKey::new(key)).unwrap();

    let default = repository.runtime_profile_view().unwrap();
    assert_eq!(default.base_url(), DEFAULT_RUNTIME_BASE_URL);
    assert_eq!(default.model(), DEFAULT_RUNTIME_MODEL);
    assert!(!default.api_key_configured());

    let short_secret = "tiny";
    let short_key_view = repository
        .update_runtime_profile(
            "https://runtime.example.test/openai/v1/",
            "owner/model-v0",
            RuntimeProfileKeyAction::Replace(short_secret),
        )
        .unwrap();
    assert!(short_key_view.api_key_configured());
    assert_eq!(short_key_view.api_key_last_four(), None);
    assert!(!format!("{short_key_view:?}").contains(short_secret));

    let replaced = repository
        .update_runtime_profile(
            "https://runtime.example.test/openai/v1/",
            "owner/model-v1",
            RuntimeProfileKeyAction::Replace(secret),
        )
        .unwrap();
    assert_eq!(
        replaced.base_url(),
        "https://runtime.example.test/openai/v1"
    );
    assert_eq!(replaced.model(), "owner/model-v1");
    assert!(replaced.api_key_configured());
    assert_eq!(replaced.api_key_last_four(), Some("9876"));
    assert!(!format!("{replaced:?}").contains(secret));
    let complete = repository.runtime_profile().unwrap();
    assert_eq!(complete.bearer_key(), Some(secret));
    drop(complete);
    repository.close().unwrap();

    let mut repository = VaultRepository::open(directory.path(), VaultKey::new(key)).unwrap();
    let complete = repository.runtime_profile().unwrap();
    assert_eq!(
        complete.base_url(),
        "https://runtime.example.test/openai/v1"
    );
    assert_eq!(complete.model(), "owner/model-v1");
    assert_eq!(complete.bearer_key(), Some(secret));
    drop(complete);

    let kept = repository
        .update_runtime_profile(
            "http://127.0.0.1:2244/v1",
            "owner/model-v2",
            RuntimeProfileKeyAction::Keep,
        )
        .unwrap();
    assert_eq!(kept.base_url(), "http://127.0.0.1:2244/v1");
    assert_eq!(kept.model(), "owner/model-v2");
    assert_eq!(kept.api_key_last_four(), Some("9876"));
    assert_eq!(
        repository.runtime_profile().unwrap().bearer_key(),
        Some(secret)
    );

    let cleared = repository
        .update_runtime_profile(
            "http://localhost:11434/v1",
            "owner/model-v3",
            RuntimeProfileKeyAction::Clear,
        )
        .unwrap();
    assert!(!cleared.api_key_configured());
    assert_eq!(cleared.api_key_last_four(), None);
    repository.close().unwrap();

    let repository = VaultRepository::open(directory.path(), VaultKey::new(key)).unwrap();
    assert_eq!(repository.runtime_profile().unwrap().bearer_key(), None);
}

#[test]
fn blank_control_and_over_limit_fields_are_rejected_without_mutation() {
    let directory = tempdir().unwrap();
    let key = [0x46; 32];
    let mut repository = VaultRepository::open(directory.path(), VaultKey::new(key)).unwrap();
    repository
        .update_runtime_profile(
            "https://runtime.example.test/v1",
            "stable-model",
            RuntimeProfileKeyAction::Replace("stable-synthetic-key"),
        )
        .unwrap();

    let oversized_base_url = format!("https://example.test/{}", "a".repeat(2_049));
    let oversized_model = "m".repeat(257);
    let oversized_key = "k".repeat(8_193);
    for result in [
        repository.update_runtime_profile("   ", "valid-model", RuntimeProfileKeyAction::Keep),
        repository.update_runtime_profile(
            &oversized_base_url,
            "valid-model",
            RuntimeProfileKeyAction::Keep,
        ),
        repository.update_runtime_profile(
            "https://example.test/v1",
            "   ",
            RuntimeProfileKeyAction::Keep,
        ),
        repository.update_runtime_profile(
            "https://example.test/v1",
            &oversized_model,
            RuntimeProfileKeyAction::Keep,
        ),
        repository.update_runtime_profile(
            "https://example.test/v1",
            "valid-model",
            RuntimeProfileKeyAction::Replace("   "),
        ),
        repository.update_runtime_profile(
            "https://example.test/v1",
            "valid-model",
            RuntimeProfileKeyAction::Replace("control\nkey"),
        ),
        repository.update_runtime_profile(
            "https://example.test/v1",
            "valid-model",
            RuntimeProfileKeyAction::Replace(&oversized_key),
        ),
    ] {
        assert!(matches!(result, Err(VaultError::InvalidRuntimeProfile)));
    }

    let profile = repository.runtime_profile().unwrap();
    assert_eq!(profile.base_url(), "https://runtime.example.test/v1");
    assert_eq!(profile.model(), "stable-model");
    assert_eq!(profile.bearer_key(), Some("stable-synthetic-key"));
}

fn key_sqlcipher_connection(connection: &Connection, vault_key: [u8; 32]) {
    let hkdf = Hkdf::<Sha256>::new(Some(KDF_SALT), &vault_key);
    let mut database_key = [0_u8; 32];
    hkdf.expand(DATABASE_INFO, &mut database_key).unwrap();
    let mut pragma = String::from("PRAGMA key = \"x'");
    for byte in database_key {
        write!(&mut pragma, "{byte:02x}").unwrap();
    }
    pragma.push_str("'\";");
    connection.execute_batch(&pragma).unwrap();
}
