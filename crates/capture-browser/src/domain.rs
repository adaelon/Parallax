use std::{error::Error, fmt};

use eam_core::Timestamp;
use eam_desktop_host::HostSessionId;
use serde::{Deserialize, Serialize};
use url::Url;

pub const MAX_SUBMISSION_BYTES: usize = 64;
pub const MAX_URL_BYTES: usize = 16 * 1_024;
pub const MAX_TITLE_BYTES: usize = 16 * 1_024;
pub const MAX_PAGE_CONTENT_BYTES: usize = 512 * 1_024;
pub const MAX_DWELL_MILLIS: i64 = 24 * 60 * 60 * 1_000;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserSubmissionPayload {
    pub submission_id: String,
    pub url: String,
    pub title: String,
    pub visited_at_millis: i64,
    pub dwell_millis: i64,
    pub page_content: Option<PageContentPayload>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageContentPayload {
    pub body_text: String,
    pub captured_at_millis: i64,
    pub authorized_origin: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrowserSubmission {
    submission_id: String,
    url: String,
    title: String,
    visited_at: Timestamp,
    dwell_millis: i64,
    page_content: Option<UntrustedPageContent>,
}

impl BrowserSubmission {
    /// Validates one metadata-only or explicitly source-authorized browser event.
    ///
    /// # Errors
    ///
    /// Rejects malformed identifiers, non-HTTP(S) URLs, credentials in URLs,
    /// unbounded fields, invalid intervals, and page content whose declared
    /// authorization origin does not exactly match the visited URL origin.
    pub fn from_payload(payload: BrowserSubmissionPayload) -> Result<Self, BrowserCaptureError> {
        validate_submission_id(&payload.submission_id)?;
        if payload.url.is_empty() || payload.url.len() > MAX_URL_BYTES {
            return Err(BrowserCaptureError::InvalidUrl);
        }
        if payload.title.len() > MAX_TITLE_BYTES {
            return Err(BrowserCaptureError::TitleTooLarge);
        }
        if payload.visited_at_millis < 0 || !(0..=MAX_DWELL_MILLIS).contains(&payload.dwell_millis)
        {
            return Err(BrowserCaptureError::InvalidInterval);
        }
        let ended_at_millis = payload
            .visited_at_millis
            .checked_add(payload.dwell_millis)
            .ok_or(BrowserCaptureError::InvalidInterval)?;
        let parsed = Url::parse(&payload.url).map_err(|_| BrowserCaptureError::InvalidUrl)?;
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err(BrowserCaptureError::InvalidUrl);
        }
        let canonical_url = parsed.to_string();
        let origin = parsed.origin().ascii_serialization();
        let page_content = payload
            .page_content
            .map(|content| {
                UntrustedPageContent::from_payload(
                    content,
                    &origin,
                    payload.visited_at_millis,
                    ended_at_millis,
                )
            })
            .transpose()?;

        Ok(Self {
            submission_id: payload.submission_id,
            url: canonical_url,
            title: payload.title,
            visited_at: Timestamp::from_millis(payload.visited_at_millis),
            dwell_millis: payload.dwell_millis,
            page_content,
        })
    }

    #[must_use]
    pub fn submission_id(&self) -> &str {
        &self.submission_id
    }

    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    #[must_use]
    pub const fn visited_at(&self) -> Timestamp {
        self.visited_at
    }

    #[must_use]
    pub const fn dwell_millis(&self) -> i64 {
        self.dwell_millis
    }

    #[must_use]
    pub const fn page_content(&self) -> Option<&UntrustedPageContent> {
        self.page_content.as_ref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UntrustedPageContent {
    body_text: String,
    captured_at: Timestamp,
    authorized_origin: String,
}

impl UntrustedPageContent {
    fn from_payload(
        payload: PageContentPayload,
        expected_origin: &str,
        visited_at_millis: i64,
        ended_at_millis: i64,
    ) -> Result<Self, BrowserCaptureError> {
        if payload.body_text.is_empty() || payload.body_text.len() > MAX_PAGE_CONTENT_BYTES {
            return Err(BrowserCaptureError::PageContentTooLarge);
        }
        if payload.authorized_origin != expected_origin {
            return Err(BrowserCaptureError::UnauthorizedPageContent);
        }
        if payload.captured_at_millis < visited_at_millis
            || payload.captured_at_millis > ended_at_millis
        {
            return Err(BrowserCaptureError::InvalidInterval);
        }
        Ok(Self {
            body_text: payload.body_text,
            captured_at: Timestamp::from_millis(payload.captured_at_millis),
            authorized_origin: payload.authorized_origin,
        })
    }

    #[must_use]
    pub fn body_text(&self) -> &str {
        &self.body_text
    }

    #[must_use]
    pub const fn captured_at(&self) -> Timestamp {
        self.captured_at
    }

    #[must_use]
    pub fn authorized_origin(&self) -> &str {
        &self.authorized_origin
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BrowserVisitId(u64);

impl BrowserVisitId {
    #[must_use]
    pub const fn from_raw(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrowserVisit {
    id: BrowserVisitId,
    host_session_id: HostSessionId,
    submission: BrowserSubmission,
    content_archive_id: Option<u64>,
}

impl BrowserVisit {
    #[must_use]
    pub const fn restore(
        id: BrowserVisitId,
        host_session_id: HostSessionId,
        submission: BrowserSubmission,
        content_archive_id: Option<u64>,
    ) -> Self {
        Self {
            id,
            host_session_id,
            submission,
            content_archive_id,
        }
    }

    #[must_use]
    pub const fn id(&self) -> BrowserVisitId {
        self.id
    }

    #[must_use]
    pub const fn host_session_id(&self) -> HostSessionId {
        self.host_session_id
    }

    #[must_use]
    pub const fn submission(&self) -> &BrowserSubmission {
        &self.submission
    }

    #[must_use]
    pub const fn content_archive_id(&self) -> Option<u64> {
        self.content_archive_id
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BrowserCaptureReceipt {
    visit_id: BrowserVisitId,
    content_archive_id: Option<u64>,
    reused: bool,
}

impl BrowserCaptureReceipt {
    #[must_use]
    pub const fn new(
        visit_id: BrowserVisitId,
        content_archive_id: Option<u64>,
        reused: bool,
    ) -> Self {
        Self {
            visit_id,
            content_archive_id,
            reused,
        }
    }

    #[must_use]
    pub const fn visit_id(self) -> BrowserVisitId {
        self.visit_id
    }

    #[must_use]
    pub const fn content_archive_id(self) -> Option<u64> {
        self.content_archive_id
    }

    #[must_use]
    pub const fn reused(self) -> bool {
        self.reused
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowserCaptureError {
    InvalidSubmissionId,
    InvalidUrl,
    TitleTooLarge,
    InvalidInterval,
    PageContentTooLarge,
    UnauthorizedPageContent,
}

impl fmt::Display for BrowserCaptureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidSubmissionId => "browser submission identifier is invalid",
            Self::InvalidUrl => "browser URL is invalid",
            Self::TitleTooLarge => "browser title exceeds the fixed limit",
            Self::InvalidInterval => "browser visit interval is invalid",
            Self::PageContentTooLarge => "page content exceeds the fixed limit",
            Self::UnauthorizedPageContent => "page content does not match its source authorization",
        })
    }
}

impl Error for BrowserCaptureError {}

fn validate_submission_id(value: &str) -> Result<(), BrowserCaptureError> {
    if value.is_empty()
        || value.len() > MAX_SUBMISSION_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        Err(BrowserCaptureError::InvalidSubmissionId)
    } else {
        Ok(())
    }
}
