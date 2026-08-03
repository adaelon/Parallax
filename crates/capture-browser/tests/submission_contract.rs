use eam_capture_browser::{
    BrowserCaptureError, BrowserSubmission, BrowserSubmissionPayload, MAX_PAGE_CONTENT_BYTES,
};

fn payload() -> BrowserSubmissionPayload {
    serde_json::from_value(serde_json::json!({
        "submissionId": "5d56d046-654b-4662-99db-82d53dcbf8f5",
        "url": "https://example.test/article?q=1#section",
        "title": "A bounded title",
        "visitedAtMillis": 1_000,
        "dwellMillis": 500,
        "pageContent": null,
    }))
    .unwrap()
}

#[test]
fn metadata_only_http_visit_is_accepted() {
    let submission = BrowserSubmission::from_payload(payload()).unwrap();

    assert_eq!(submission.url(), "https://example.test/article?q=1#section");
    assert_eq!(submission.visited_at().as_millis(), 1_000);
    assert_eq!(submission.dwell_millis(), 500);
    assert!(submission.page_content().is_none());
}

#[test]
fn content_requires_an_exact_origin_grant_and_interval() {
    let mut allowed = serde_json::to_value(payload()).unwrap();
    allowed["pageContent"] = serde_json::json!({
        "bodyText": "Untrusted page text",
        "capturedAtMillis": 1_500,
        "authorizedOrigin": "https://example.test",
    });
    let allowed =
        BrowserSubmission::from_payload(serde_json::from_value(allowed).unwrap()).unwrap();
    assert_eq!(
        allowed.page_content().unwrap().authorized_origin(),
        "https://example.test"
    );

    let mut forged = serde_json::to_value(payload()).unwrap();
    forged["pageContent"] = serde_json::json!({
        "bodyText": "Untrusted page text",
        "capturedAtMillis": 1_500,
        "authorizedOrigin": "https://other.test",
    });
    assert_eq!(
        BrowserSubmission::from_payload(serde_json::from_value(forged).unwrap()),
        Err(BrowserCaptureError::UnauthorizedPageContent)
    );
}

#[test]
fn non_web_urls_credentials_and_oversized_content_are_rejected() {
    for url in [
        "chrome://history",
        "file:///secret.txt",
        "https://person:secret@example.test/",
    ] {
        let mut candidate = payload();
        candidate.url = url.to_owned();
        assert_eq!(
            BrowserSubmission::from_payload(candidate),
            Err(BrowserCaptureError::InvalidUrl)
        );
    }

    let mut oversized = serde_json::to_value(payload()).unwrap();
    oversized["pageContent"] = serde_json::json!({
        "bodyText": "x".repeat(MAX_PAGE_CONTENT_BYTES + 1),
        "capturedAtMillis": 1_500,
        "authorizedOrigin": "https://example.test",
    });
    assert_eq!(
        BrowserSubmission::from_payload(serde_json::from_value(oversized).unwrap()),
        Err(BrowserCaptureError::PageContentTooLarge)
    );
}
