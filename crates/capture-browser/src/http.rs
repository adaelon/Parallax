use std::{
    io::{self, Read, Write},
    net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};

use serde::Serialize;
use subtle::ConstantTimeEq;

use crate::{BrowserCaptureReceipt, BrowserSubmission, BrowserSubmissionPayload};

pub const DEFAULT_BROWSER_CAPTURE_ADDRESS: SocketAddrV4 =
    SocketAddrV4::new(Ipv4Addr::LOCALHOST, 43_129);
pub const EXTENSION_ID: &str = "knpliheabhbegfjbgdiaclndgnjelggh";
pub const EXTENSION_ORIGIN: &str = "chrome-extension://knpliheabhbegfjbgdiaclndgnjelggh";
const MAX_HEADER_BYTES: usize = 16 * 1_024;
const MAX_REQUEST_BODY_BYTES: usize = 600 * 1_024;

#[derive(Clone)]
pub struct BrowserCaptureSession {
    token: String,
}

impl BrowserCaptureSession {
    /// Creates an unpredictable credential valid only for one host process lifetime.
    ///
    /// # Errors
    ///
    /// Returns an operating-system entropy failure.
    pub fn new_random() -> io::Result<Self> {
        let mut secret = [0_u8; 32];
        getrandom::fill(&mut secret)
            .map_err(|error| io::Error::other(format!("OS entropy failed: {error}")))?;
        Ok(Self::from_secret(secret))
    }

    #[must_use]
    pub fn from_secret(secret: [u8; 32]) -> Self {
        let mut token = String::with_capacity(secret.len() * 2);
        for byte in secret {
            use std::fmt::Write as _;
            write!(&mut token, "{byte:02x}").expect("writing to String cannot fail");
        }
        Self { token }
    }

    fn authorizes(&self, supplied: &str) -> bool {
        supplied.len() == self.token.len()
            && bool::from(self.token.as_bytes().ct_eq(supplied.as_bytes()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerScope {
    Loopback,
    Remote,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: String,
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    fn json(status: u16, body: impl Serialize, cors: bool) -> Self {
        let body = serde_json::to_vec(&body).expect("fixed response serialization cannot fail");
        let mut headers = vec![
            ("Content-Type".to_owned(), "application/json".to_owned()),
            ("Cache-Control".to_owned(), "no-store".to_owned()),
        ];
        if cors {
            headers.push((
                "Access-Control-Allow-Origin".to_owned(),
                EXTENSION_ORIGIN.to_owned(),
            ));
            headers.push(("Vary".to_owned(), "Origin".to_owned()));
        }
        Self {
            status,
            headers,
            body,
        }
    }

    fn empty(status: u16, cors: bool) -> Self {
        let mut response = Self::json(status, serde_json::json!({}), cors);
        response.body.clear();
        response
    }
}

/// Applies the fixed-origin, loopback, session-token, and bounded JSON contract.
#[must_use]
pub fn handle_http_request(
    request: &HttpRequest,
    peer: PeerScope,
    session: &BrowserCaptureSession,
    submit: impl FnOnce(BrowserSubmission) -> Result<BrowserCaptureReceipt, String>,
) -> HttpResponse {
    if peer != PeerScope::Loopback {
        return error_response(403, false);
    }
    let Some(origin) = unique_header(request, "origin") else {
        return error_response(403, false);
    };
    if origin != EXTENSION_ORIGIN {
        return error_response(403, false);
    }

    if request.method == "OPTIONS" && request.target == "/v1/browser-events" {
        let mut response = HttpResponse::empty(204, true);
        response.headers.extend([
            ("Access-Control-Allow-Methods".to_owned(), "POST".to_owned()),
            (
                "Access-Control-Allow-Headers".to_owned(),
                "authorization, content-type".to_owned(),
            ),
            ("Access-Control-Max-Age".to_owned(), "300".to_owned()),
        ]);
        return response;
    }
    if request.method == "GET" && request.target == "/v1/session" {
        if !request.body.is_empty() {
            return error_response(400, true);
        }
        return HttpResponse::json(
            200,
            serde_json::json!({
                "protocolVersion": "eam-browser-capture-v1",
                "token": session.token,
            }),
            true,
        );
    }
    if request.method != "POST" || request.target != "/v1/browser-events" {
        return error_response(404, true);
    }
    let Some(authorization) = unique_header(request, "authorization") else {
        return error_response(401, true);
    };
    let Some(token) = authorization.strip_prefix("Bearer ") else {
        return error_response(401, true);
    };
    if !session.authorizes(token) {
        return error_response(401, true);
    }
    if unique_header(request, "content-type") != Some("application/json") {
        return error_response(415, true);
    }
    if request.body.len() > MAX_REQUEST_BODY_BYTES {
        return error_response(413, true);
    }
    let Ok(payload) = serde_json::from_slice::<BrowserSubmissionPayload>(&request.body) else {
        return error_response(400, true);
    };
    let Ok(submission) = BrowserSubmission::from_payload(payload) else {
        return error_response(422, true);
    };
    match submit(submission) {
        Ok(receipt) => HttpResponse::json(
            202,
            serde_json::json!({
                "visitId": receipt.visit_id().get(),
                "contentArchiveId": receipt.content_archive_id(),
                "reused": receipt.reused(),
            }),
            true,
        ),
        Err(_) => error_response(503, true),
    }
}

/// Binds the only network surface to IPv4 loopback on the fixed extension port.
///
/// # Errors
///
/// Returns the socket bind or nonblocking configuration error.
pub fn bind_loopback() -> io::Result<TcpListener> {
    let listener = TcpListener::bind(DEFAULT_BROWSER_CAPTURE_ADDRESS)?;
    listener.set_nonblocking(true)?;
    Ok(listener)
}

/// Serves bounded one-request HTTP connections until the host stop flag is set.
///
/// # Errors
///
/// Returns an unexpected listener accept error. Individual malformed or stalled
/// clients receive a bounded error or are dropped without affecting the host.
pub fn serve_loopback<F>(
    listener: &TcpListener,
    session: &BrowserCaptureSession,
    stop: &AtomicBool,
    submit: F,
) -> io::Result<()>
where
    F: Fn(BrowserSubmission) -> Result<BrowserCaptureReceipt, String>,
{
    while !stop.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((mut stream, peer)) => {
                let scope = if peer.ip().is_loopback() {
                    PeerScope::Loopback
                } else {
                    PeerScope::Remote
                };
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
                let response = read_request(&mut stream).map_or_else(
                    |status| error_response(status, false),
                    |request| handle_http_request(&request, scope, session, &submit),
                );
                let _ = write_response(&mut stream, &response);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(25));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn unique_header<'a>(request: &'a HttpRequest, name: &str) -> Option<&'a str> {
    let mut values = request
        .headers
        .iter()
        .filter(|(header, _)| header.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str());
    let first = values.next()?;
    values.next().is_none().then_some(first)
}

fn error_response(status: u16, cors: bool) -> HttpResponse {
    HttpResponse::json(
        status,
        serde_json::json!({ "error": "request rejected" }),
        cors,
    )
}

fn read_request(stream: &mut TcpStream) -> Result<HttpRequest, u16> {
    let mut bytes = Vec::with_capacity(1_024);
    let mut chunk = [0_u8; 1_024];
    let header_end = loop {
        let read = stream.read(&mut chunk).map_err(|_| 400_u16)?;
        if read == 0 {
            return Err(400);
        }
        bytes.extend_from_slice(&chunk[..read]);
        if let Some(position) = find_header_end(&bytes) {
            break position;
        }
        if bytes.len() > MAX_HEADER_BYTES {
            return Err(431);
        }
    };
    if header_end > MAX_HEADER_BYTES {
        return Err(431);
    }
    let header_text = std::str::from_utf8(&bytes[..header_end]).map_err(|_| 400_u16)?;
    let mut lines = header_text.split("\r\n");
    let request_line = lines.next().ok_or(400_u16)?;
    let mut request_parts = request_line.split(' ');
    let method = request_parts.next().ok_or(400_u16)?.to_owned();
    let target = request_parts.next().ok_or(400_u16)?.to_owned();
    if request_parts.next() != Some("HTTP/1.1") || request_parts.next().is_some() {
        return Err(400);
    }
    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (name, value) = line.split_once(':').ok_or(400_u16)?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(400);
        }
        headers.push((name.to_ascii_lowercase(), value.trim().to_owned()));
    }
    let request = HttpRequest {
        method,
        target,
        headers,
        body: Vec::new(),
    };
    if unique_header(&request, "transfer-encoding").is_some() {
        return Err(400);
    }
    let content_length = match unique_header(&request, "content-length") {
        Some(value) => value.parse::<usize>().map_err(|_| 400_u16)?,
        None => 0,
    };
    if content_length > MAX_REQUEST_BODY_BYTES {
        return Err(413);
    }
    let body_start = header_end + 4;
    while bytes.len().saturating_sub(body_start) < content_length {
        let read = stream.read(&mut chunk).map_err(|_| 400_u16)?;
        if read == 0 {
            return Err(400);
        }
        bytes.extend_from_slice(&chunk[..read]);
        if bytes.len().saturating_sub(body_start) > MAX_REQUEST_BODY_BYTES {
            return Err(413);
        }
    }
    if bytes.len().saturating_sub(body_start) != content_length {
        return Err(400);
    }
    Ok(HttpRequest {
        body: bytes[body_start..].to_vec(),
        ..request
    })
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

fn write_response(stream: &mut TcpStream, response: &HttpResponse) -> io::Result<()> {
    let reason = match response.status {
        200 => "OK",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        422 => "Unprocessable Content",
        431 => "Request Header Fields Too Large",
        503 => "Service Unavailable",
        _ => "Error",
    };
    write!(stream, "HTTP/1.1 {} {}\r\n", response.status, reason)?;
    for (name, value) in &response.headers {
        write!(stream, "{name}: {value}\r\n")?;
    }
    write!(
        stream,
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        response.body.len()
    )?;
    stream.write_all(&response.body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BrowserVisitId;

    #[test]
    fn loopback_origin_is_pinned_to_the_manifest_public_key_id() {
        assert_eq!(EXTENSION_ID, "knpliheabhbegfjbgdiaclndgnjelggh");
        assert_eq!(
            EXTENSION_ORIGIN,
            "chrome-extension://knpliheabhbegfjbgdiaclndgnjelggh"
        );
    }

    fn request(
        method: &str,
        target: &str,
        token: Option<&str>,
        body: &serde_json::Value,
    ) -> HttpRequest {
        let mut headers = vec![("origin".to_owned(), EXTENSION_ORIGIN.to_owned())];
        if let Some(token) = token {
            headers.push(("authorization".to_owned(), format!("Bearer {token}")));
            headers.push(("content-type".to_owned(), "application/json".to_owned()));
        }
        HttpRequest {
            method: method.to_owned(),
            target: target.to_owned(),
            headers,
            body: if body.is_null() {
                Vec::new()
            } else {
                serde_json::to_vec(body).unwrap()
            },
        }
    }

    fn valid_body() -> serde_json::Value {
        serde_json::json!({
            "submissionId": "ad7e35a4-63b9-4c7f-a2ec-e9011d6c35b2",
            "url": "https://example.test/article",
            "title": "Article",
            "visitedAtMillis": 1_000,
            "dwellMillis": 500,
            "pageContent": null,
        })
    }

    #[test]
    fn remote_and_forged_submissions_are_rejected_before_the_sink() {
        let session = BrowserCaptureSession::from_secret([0x11; 32]);
        let mut called = false;
        let remote = handle_http_request(
            &request(
                "POST",
                "/v1/browser-events",
                Some(&session.token),
                &valid_body(),
            ),
            PeerScope::Remote,
            &session,
            |_| {
                called = true;
                Ok(BrowserCaptureReceipt::new(
                    BrowserVisitId::from_raw(1),
                    None,
                    false,
                ))
            },
        );
        assert_eq!(remote.status, 403);
        assert!(!called);

        let forged = handle_http_request(
            &request("POST", "/v1/browser-events", Some("00"), &valid_body()),
            PeerScope::Loopback,
            &session,
            |_| {
                called = true;
                Ok(BrowserCaptureReceipt::new(
                    BrowserVisitId::from_raw(1),
                    None,
                    false,
                ))
            },
        );
        assert_eq!(forged.status, 401);
        assert!(!called);
    }

    #[test]
    fn exact_extension_origin_bootstraps_and_submits_one_bounded_event() {
        let session = BrowserCaptureSession::from_secret([0x22; 32]);
        let bootstrap = handle_http_request(
            &request("GET", "/v1/session", None, &serde_json::Value::Null),
            PeerScope::Loopback,
            &session,
            |_| unreachable!(),
        );
        assert_eq!(bootstrap.status, 200);
        let body: serde_json::Value = serde_json::from_slice(&bootstrap.body).unwrap();
        let token = body["token"].as_str().unwrap();

        let accepted = handle_http_request(
            &request("POST", "/v1/browser-events", Some(token), &valid_body()),
            PeerScope::Loopback,
            &session,
            |submission| {
                assert_eq!(submission.url(), "https://example.test/article");
                Ok(BrowserCaptureReceipt::new(
                    BrowserVisitId::from_raw(7),
                    None,
                    false,
                ))
            },
        );
        assert_eq!(accepted.status, 202);
        assert!(!String::from_utf8_lossy(&accepted.body).contains(token));
    }

    #[test]
    fn wrong_extension_origin_cannot_bootstrap_a_session() {
        let session = BrowserCaptureSession::from_secret([0x33; 32]);
        let mut request = request("GET", "/v1/session", None, &serde_json::Value::Null);
        request.headers[0].1 = "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned();

        let response =
            handle_http_request(&request, PeerScope::Loopback, &session, |_| unreachable!());

        assert_eq!(response.status, 403);
        assert!(
            response
                .headers
                .iter()
                .all(|(name, _)| name != "Access-Control-Allow-Origin")
        );
    }
}
