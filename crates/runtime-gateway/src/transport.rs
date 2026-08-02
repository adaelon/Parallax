use std::{error::Error, fmt, time::Duration};

use eam_core::{ClaimId, EvidenceId};
use reqwest::{StatusCode, blocking::Client, redirect::Policy};
use zeroize::Zeroizing;

pub const OPENAI_CLOUD_MODEL: &str = "gpt-5.6-terra";
pub const OPENAI_LOCAL_MODEL: &str = "gpt-oss-20b";
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeTargetKind {
    Local,
    Cloud,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeTarget {
    kind: RuntimeTargetKind,
    endpoint: String,
    model: &'static str,
}

impl RuntimeTarget {
    #[must_use]
    pub fn openai_cloud(endpoint: impl Into<String>) -> Self {
        Self {
            kind: RuntimeTargetKind::Cloud,
            endpoint: endpoint.into(),
            model: OPENAI_CLOUD_MODEL,
        }
    }

    #[must_use]
    pub fn openai_local(endpoint: impl Into<String>) -> Self {
        Self {
            kind: RuntimeTargetKind::Local,
            endpoint: endpoint.into(),
            model: OPENAI_LOCAL_MODEL,
        }
    }

    #[must_use]
    pub const fn kind(&self) -> RuntimeTargetKind {
        self.kind
    }

    #[must_use]
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    #[must_use]
    pub const fn model(&self) -> &'static str {
        self.model
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvocationKind {
    Classification,
    Response,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutboundDisclosureRecord {
    sequence: u64,
    target: RuntimeTargetKind,
    model: &'static str,
    invocation: InvocationKind,
    evidence_ids: Vec<EvidenceId>,
    retrieved_sources: Vec<OutboundContextSource>,
    request_json: String,
}

impl OutboundDisclosureRecord {
    pub(crate) fn new(
        sequence: u64,
        target: RuntimeTargetKind,
        model: &'static str,
        invocation: InvocationKind,
        evidence_ids: Vec<EvidenceId>,
        retrieved_sources: Vec<OutboundContextSource>,
        request_json: String,
    ) -> Self {
        Self {
            sequence,
            target,
            model,
            invocation,
            evidence_ids,
            retrieved_sources,
            request_json,
        }
    }

    #[must_use]
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    #[must_use]
    pub const fn target(&self) -> RuntimeTargetKind {
        self.target
    }

    #[must_use]
    pub const fn model(&self) -> &'static str {
        self.model
    }

    #[must_use]
    pub const fn invocation(&self) -> InvocationKind {
        self.invocation
    }

    #[must_use]
    pub fn evidence_ids(&self) -> &[EvidenceId] {
        &self.evidence_ids
    }

    #[must_use]
    pub fn retrieved_sources(&self) -> &[OutboundContextSource] {
        &self.retrieved_sources
    }

    #[must_use]
    pub fn request_json(&self) -> &str {
        &self.request_json
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutboundContextSource {
    EvidenceBlock { evidence_id: u64, block_id: u64 },
    LedgerClaim { claim_id: ClaimId },
    MemoryDispute { memory_id: u64, dispute_id: u64 },
    IdentityState { version: u64 },
    ReflectionInvitation { invitation_id: u64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportErrorKind {
    Timeout,
    Unavailable,
    InvalidResponse,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransportError {
    kind: TransportErrorKind,
    message: String,
}

impl TransportError {
    #[must_use]
    pub fn timeout(message: impl Into<String>) -> Self {
        Self {
            kind: TransportErrorKind::Timeout,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self {
            kind: TransportErrorKind::Unavailable,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn invalid_response(message: impl Into<String>) -> Self {
        Self {
            kind: TransportErrorKind::InvalidResponse,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn other(message: impl Into<String>) -> Self {
        Self {
            kind: TransportErrorKind::Other,
            message: message.into(),
        }
    }

    #[must_use]
    pub const fn kind(&self) -> TransportErrorKind {
        self.kind
    }
}

impl fmt::Display for TransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for TransportError {}

/// Sends one already-serialized Responses v1 request.
///
/// Authentication belongs to the host transport implementation. The trait
/// intentionally exposes no repository or credential inspection capability.
pub trait ResponsesTransport {
    /// Sends one request and returns the unmodified provider response body.
    ///
    /// # Errors
    ///
    /// Returns a categorized timeout, availability, or transport failure.
    fn send(
        &mut self,
        target: &RuntimeTarget,
        request_json: &str,
        timeout: Duration,
    ) -> Result<String, TransportError>;
}

/// Blocking HTTPS/HTTP transport for the Responses v1 endpoint.
///
/// The optional bearer token is held in zeroizing memory and added only as an
/// HTTP header, after the auditable request body has been recorded.
pub struct HttpResponsesTransport {
    client: Client,
    bearer_token: Option<Zeroizing<String>>,
}

impl HttpResponsesTransport {
    /// Creates a cloud transport that adds one bearer token per request.
    ///
    /// # Errors
    ///
    /// Returns a transport error when the HTTP client cannot be constructed.
    pub fn openai_cloud(bearer_token: impl Into<String>) -> Result<Self, TransportError> {
        let bearer_token = bearer_token.into();
        if bearer_token.trim().is_empty() {
            return Err(TransportError::other(
                "cloud Responses transport requires a non-empty bearer token",
            ));
        }
        Self::build(Some(Zeroizing::new(bearer_token)), false)
    }

    /// Creates a local transport without cloud credentials or system proxies.
    ///
    /// # Errors
    ///
    /// Returns a transport error when the HTTP client cannot be constructed.
    pub fn openai_local() -> Result<Self, TransportError> {
        Self::build(None, true)
    }

    fn build(
        bearer_token: Option<Zeroizing<String>>,
        no_proxy: bool,
    ) -> Result<Self, TransportError> {
        let mut builder = Client::builder().redirect(Policy::none());
        if no_proxy {
            builder = builder.no_proxy();
        }
        let client = builder
            .build()
            .map_err(|error| TransportError::other(error.to_string()))?;
        Ok(Self {
            client,
            bearer_token,
        })
    }
}

impl ResponsesTransport for HttpResponsesTransport {
    fn send(
        &mut self,
        target: &RuntimeTarget,
        request_json: &str,
        timeout: Duration,
    ) -> Result<String, TransportError> {
        match (target.kind(), self.bearer_token.as_ref()) {
            (RuntimeTargetKind::Cloud, None) => {
                return Err(TransportError::other(
                    "cloud Responses transport requires a bearer token",
                ));
            }
            (RuntimeTargetKind::Cloud, Some(_)) if !target.endpoint().starts_with("https://") => {
                return Err(TransportError::other(
                    "cloud Responses endpoint must use HTTPS",
                ));
            }
            (RuntimeTargetKind::Local, Some(_)) => {
                return Err(TransportError::other(
                    "local Responses transport cannot carry a cloud bearer token",
                ));
            }
            (RuntimeTargetKind::Cloud, Some(_)) | (RuntimeTargetKind::Local, None) => {}
        }

        let mut request = self
            .client
            .post(target.endpoint())
            .timeout(timeout)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(request_json.to_owned());
        if let Some(token) = &self.bearer_token {
            request = request.bearer_auth(token.as_str());
        }

        let response = request.send().map_err(|error| map_http_error(&error))?;
        let status = response.status();
        if !status.is_success() {
            return Err(map_http_status(status));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            return Err(TransportError::invalid_response(
                "provider response exceeds the S06 byte limit",
            ));
        }
        let bytes = response.bytes().map_err(|error| map_http_error(&error))?;
        if bytes.len() > MAX_RESPONSE_BYTES {
            return Err(TransportError::invalid_response(
                "provider response exceeds the S06 byte limit",
            ));
        }
        String::from_utf8(bytes.to_vec())
            .map_err(|error| TransportError::invalid_response(error.to_string()))
    }
}

fn map_http_error(error: &reqwest::Error) -> TransportError {
    if error.is_timeout() {
        TransportError::timeout(error.to_string())
    } else if error.is_connect() {
        TransportError::unavailable(error.to_string())
    } else {
        TransportError::other(error.to_string())
    }
}

fn map_http_status(status: StatusCode) -> TransportError {
    let message = format!("Responses endpoint returned HTTP {status}");
    if status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
    {
        TransportError::unavailable(message)
    } else {
        TransportError::other(message)
    }
}
