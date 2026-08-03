use eam_capture_browser::{
    BrowserCaptureRepository, BrowserSubmission, BrowserSubmissionPayload, PageContentPayload,
};
use eam_desktop_host::{HostLifecycleRepository, LaunchMode};
use eam_ingestion::{ArchiveStatus, UnparsedReason};
use eam_vault::{VaultKey, VaultRepository};
use tempfile::tempdir;

fn submission(id: &str, title: &str, content: Option<&str>) -> BrowserSubmission {
    BrowserSubmission::from_payload(BrowserSubmissionPayload {
        submission_id: id.to_owned(),
        url: "https://example.test/article?q=1".to_owned(),
        title: title.to_owned(),
        visited_at_millis: 1_000,
        dwell_millis: 500,
        page_content: content.map(|body_text| PageContentPayload {
            body_text: body_text.to_owned(),
            captured_at_millis: 1_500,
            authorized_origin: "https://example.test".to_owned(),
        }),
    })
    .unwrap()
}

#[test]
fn metadata_and_authorized_page_text_survive_reopen_as_untrusted_evidence() {
    let directory = tempdir().unwrap();
    let key = [0x29; 32];
    let mut repository = VaultRepository::open(directory.path(), VaultKey::new(key)).unwrap();
    let session = repository
        .begin_host_session(
            eam_core::Timestamp::from_millis(900),
            LaunchMode::Foreground,
        )
        .unwrap()
        .session()
        .id();
    let captured = submission(
        "930d44db-47d1-4a37-ad6c-427608cb1cf3",
        "Article",
        Some("untrusted page text"),
    );

    let first = repository
        .record_browser_submission(session, &captured)
        .unwrap();
    let retried = repository
        .record_browser_submission(session, &captured)
        .unwrap();

    assert!(!first.reused());
    assert!(retried.reused());
    assert_eq!(first.visit_id(), retried.visit_id());
    assert_eq!(first.content_archive_id(), retried.content_archive_id());
    let archive_id = first.content_archive_id().unwrap();
    assert_eq!(
        repository.read_archived_content(archive_id).unwrap(),
        b"untrusted page text"
    );
    let archived = repository.archived_evidence().unwrap();
    assert_eq!(archived.len(), 1);
    assert_eq!(
        archived[0].status,
        ArchiveStatus::ArchivedUnparsed(UnparsedReason::UnsupportedFormat)
    );
    repository.close().unwrap();

    let repository = VaultRepository::open(directory.path(), VaultKey::new(key)).unwrap();
    let visits = repository.all_browser_visits().unwrap();
    assert_eq!(visits.len(), 1);
    assert_eq!(visits[0].submission(), &captured);
    assert_eq!(visits[0].content_archive_id(), Some(archive_id));
}

#[test]
fn conflicting_retry_and_stale_host_session_are_rejected() {
    let directory = tempdir().unwrap();
    let mut repository =
        VaultRepository::open(directory.path(), VaultKey::new([0x31; 32])).unwrap();
    let first_session = repository
        .begin_host_session(
            eam_core::Timestamp::from_millis(900),
            LaunchMode::Foreground,
        )
        .unwrap()
        .session()
        .id();
    let captured = submission("event-conflict", "Original", None);
    repository
        .record_browser_submission(first_session, &captured)
        .unwrap();

    let conflict = repository.record_browser_submission(
        first_session,
        &submission("event-conflict", "Changed", None),
    );
    assert!(conflict.is_err());
    repository
        .finish_host_session(
            first_session,
            eam_core::Timestamp::from_millis(2_000),
            eam_desktop_host::ExitReason::Explicit,
        )
        .unwrap();
    let second_session = repository
        .begin_host_session(
            eam_core::Timestamp::from_millis(3_000),
            LaunchMode::Foreground,
        )
        .unwrap()
        .session()
        .id();
    assert_ne!(first_session, second_session);

    assert!(
        repository
            .record_browser_submission(first_session, &submission("event-stale", "Stale", None),)
            .is_err()
    );
    assert_eq!(repository.all_browser_visits().unwrap().len(), 1);
}
