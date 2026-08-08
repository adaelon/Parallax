use std::{error::Error, fmt, time::Duration};

use eam_core::{ClaimId, EvidenceId};
use reqwest::{StatusCode, blocking::Client, redirect::Policy};
use url::{Host, Url};
use zeroize::Zeroizing;

const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_BASE_URL_BYTES: usize = 2 * 1024;
const MAX_MODEL_BYTES: usize = 256;
const MAX_BEARER_TOKEN_BYTES: usize = 8 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeTargetKind {
    Local,
    Cloud,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeProtocol {
    OpenAiResponses,
    DeepSeekChatCompletions,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeTarget {
    kind: RuntimeTargetKind,
    protocol: RuntimeProtocol,
    base_url: Url,
    model: String,
}

impl RuntimeTarget {
    /// Validates and normalizes one configurable runtime target.
    ///
    /// Remote targets require HTTPS. HTTP is accepted only for the literal
    /// loopback hosts `localhost`, `127.0.0.0/8`, and `::1`. Credentials,
    /// query strings, and fragments are rejected before any request exists.
    ///
    /// # Errors
    ///
    /// Returns a sanitized configuration error for an invalid Base URL or
    /// model identifier.
    pub fn new(
        base_url: impl AsRef<str>,
        model: impl Into<String>,
    ) -> Result<Self, RuntimeTargetError> {
        let base_url = base_url.as_ref();
        if base_url.is_empty()
            || base_url.len() > MAX_BASE_URL_BYTES
            || base_url.trim() != base_url
            || base_url.chars().any(char::is_whitespace)
            || base_url.chars().any(char::is_control)
        {
            return Err(RuntimeTargetError::new("invalid runtime Base URL"));
        }
        let mut parsed = Url::parse(base_url)
            .map_err(|_| RuntimeTargetError::new("invalid runtime Base URL"))?;
        if parsed.cannot_be_a_base()
            || parsed.host().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(RuntimeTargetError::new("invalid runtime Base URL"));
        }

        let is_loopback = match parsed.host() {
            Some(Host::Domain(domain)) => domain == "localhost",
            Some(Host::Ipv4(address)) => address.is_loopback(),
            Some(Host::Ipv6(address)) => address.is_loopback(),
            None => false,
        };
        match (parsed.scheme(), is_loopback) {
            ("https", _) | ("http", true) => {}
            _ => return Err(RuntimeTargetError::new("invalid runtime Base URL")),
        }

        let model = model.into();
        if model.is_empty()
            || model.len() > MAX_MODEL_BYTES
            || model.trim() != model
            || model.chars().any(char::is_control)
        {
            return Err(RuntimeTargetError::new("invalid runtime model identifier"));
        }

        let normalized_path = parsed.path().trim_end_matches('/').to_owned();
        parsed.set_path(&normalized_path);
        Ok(Self {
            kind: if is_loopback {
                RuntimeTargetKind::Local
            } else {
                RuntimeTargetKind::Cloud
            },
            protocol: if matches!(parsed.host(), Some(Host::Domain("api.deepseek.com"))) {
                RuntimeProtocol::DeepSeekChatCompletions
            } else {
                RuntimeProtocol::OpenAiResponses
            },
            base_url: parsed,
            model,
        })
    }

    #[must_use]
    pub const fn kind(&self) -> RuntimeTargetKind {
        self.kind
    }

    #[must_use]
    pub const fn protocol(&self) -> RuntimeProtocol {
        self.protocol
    }

    #[must_use]
    pub fn base_url(&self) -> &str {
        self.base_url.as_str()
    }

    #[must_use]
    pub fn endpoint(&self) -> String {
        let mut endpoint = self.base_url.clone();
        let suffix = match self.protocol {
            RuntimeProtocol::OpenAiResponses => "responses",
            RuntimeProtocol::DeepSeekChatCompletions => "chat/completions",
        };
        let endpoint_path = if self.base_url.path() == "/" {
            format!("/{suffix}")
        } else {
            format!("{}/{suffix}", self.base_url.path())
        };
        endpoint.set_path(&endpoint_path);
        endpoint.into()
    }

    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeTargetError {
    message: &'static str,
}

impl RuntimeTargetError {
    const fn new(message: &'static str) -> Self {
        Self { message }
    }
}

impl fmt::Display for RuntimeTargetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message)
    }
}

impl Error for RuntimeTargetError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvocationKind {
    InitialIdentity,
    Classification,
    Response,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutboundDisclosureRecord {
    sequence: u64,
    target: RuntimeTargetKind,
    model: String,
    invocation: InvocationKind,
    evidence_ids: Vec<EvidenceId>,
    retrieved_sources: Vec<OutboundContextSource>,
    request_json: String,
}

impl OutboundDisclosureRecord {
    pub(crate) fn new(
        sequence: u64,
        target: RuntimeTargetKind,
        model: String,
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
    pub fn model(&self) -> &str {
        &self.model
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

/// Sends one request already serialized for the target's selected protocol.
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
        endpoint: &str,
        request_json: &str,
        timeout: Duration,
    ) -> Result<String, TransportError>;
}

/// Blocking HTTPS/loopback-HTTP transport for a validated runtime endpoint.
///
/// The optional bearer token is held in zeroizing memory and added only as an
/// HTTP header, after the auditable request body has been recorded.
pub struct HttpResponsesTransport {
    remote_client: Client,
    loopback_client: Client,
    bearer_token: Option<Zeroizing<String>>,
}

impl HttpResponsesTransport {
    /// Creates a transport with an optional bearer token.
    ///
    /// # Errors
    ///
    /// Returns a transport error when the HTTP client cannot be constructed.
    pub fn new(bearer_token: Option<String>) -> Result<Self, TransportError> {
        let bearer_token = bearer_token.map(Zeroizing::new);
        validate_responses_bearer_token(bearer_token.as_ref().map(|token| token.as_str()))?;
        let remote_client = build_http_client(false)?;
        let loopback_client = build_http_client(true)?;
        Ok(Self {
            remote_client,
            loopback_client,
            bearer_token,
        })
    }
}

/// Applies the runtime bearer-token field boundary without
/// constructing an HTTP client or retaining the secret.
///
/// # Errors
///
/// Rejects blank, control-containing, or over-limit tokens with a sanitized
/// transport error that never includes the candidate value.
pub fn validate_responses_bearer_token(bearer_token: Option<&str>) -> Result<(), TransportError> {
    if bearer_token.is_some_and(|token| {
        token.trim().is_empty()
            || token.len() > MAX_BEARER_TOKEN_BYTES
            || token.chars().any(char::is_control)
    }) {
        return Err(TransportError::other("invalid Responses bearer token"));
    }
    Ok(())
}

impl ResponsesTransport for HttpResponsesTransport {
    fn send(
        &mut self,
        target: &RuntimeTarget,
        endpoint: &str,
        request_json: &str,
        timeout: Duration,
    ) -> Result<String, TransportError> {
        if target.endpoint() != endpoint {
            return Err(TransportError::other(
                "runtime endpoint does not match the validated target",
            ));
        }
        let client = match target.kind() {
            RuntimeTargetKind::Local => &self.loopback_client,
            RuntimeTargetKind::Cloud => &self.remote_client,
        };
        let mut request = client
            .post(endpoint)
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
        TransportError::timeout("runtime request timed out")
    } else if error.is_connect() {
        TransportError::unavailable("runtime endpoint is unavailable")
    } else {
        TransportError::other("runtime request failed")
    }
}

fn build_http_client(no_proxy: bool) -> Result<Client, TransportError> {
    let mut builder = Client::builder().redirect(Policy::none());
    if no_proxy {
        builder = builder.no_proxy();
    }
    builder
        .build()
        .map_err(|_| TransportError::other("runtime HTTP client construction failed"))
}

fn map_http_status(status: StatusCode) -> TransportError {
    let message = format!("runtime endpoint returned HTTP {status}");
    if status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
    {
        TransportError::unavailable(message)
    } else {
        TransportError::other(message)
    }
}
