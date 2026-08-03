//! Minimal-permission browser capture contract for S29.
//!
//! The browser extension is an untrusted evidence source. This crate owns the
//! bounded submission model and the authenticated loopback adapter; it never
//! receives a Vault handle, model runtime, or general `WebView` capability.

mod domain;
mod http;
mod ports;

pub use domain::{
    BrowserCaptureError, BrowserCaptureReceipt, BrowserSubmission, BrowserSubmissionPayload,
    BrowserVisit, BrowserVisitId, MAX_DWELL_MILLIS, MAX_PAGE_CONTENT_BYTES, MAX_SUBMISSION_BYTES,
    MAX_TITLE_BYTES, MAX_URL_BYTES, PageContentPayload, UntrustedPageContent,
};
pub use http::{
    BrowserCaptureSession, DEFAULT_BROWSER_CAPTURE_ADDRESS, EXTENSION_ID, EXTENSION_ORIGIN,
    HttpRequest, HttpResponse, PeerScope, bind_loopback, handle_http_request, serve_loopback,
};
pub use ports::BrowserCaptureRepository;
