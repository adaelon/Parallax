use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use tempfile::tempdir;

use super::*;

const PROFILE_TEST_KEY: [u8; 32] = [0x6d; 32];
const OLD_BEARER: &str = "synthetic-old-bearer-1111";
const NEW_BEARER: &str = "synthetic-new-bearer-2222";
const CLASSIFICATION_RESPONSE: &str = r#"{
  "output": [{
    "type": "message",
    "content": [{
      "type": "output_text",
      "text": "{\"classification\":\"question\"}"
    }]
  }]
}"#;
const TURN_RESPONSE: &str = r#"{
  "output": [{
    "type": "message",
    "content": [{
      "type": "output_text",
      "text": "{\"text\":\"synthetic response\",\"citations\":[],\"operations\":[]}"
    }]
  }]
}"#;

#[derive(Clone, Copy)]
struct ServerReply {
    status: &'static str,
    body: &'static str,
}

impl ServerReply {
    const fn ok(body: &'static str) -> Self {
        Self {
            status: "200 OK",
            body,
        }
    }

    const fn rejected(body: &'static str) -> Self {
        Self {
            status: "401 Unauthorized",
            body,
        }
    }
}

struct RuntimeServer {
    base_url: String,
    handle: JoinHandle<Vec<String>>,
}

impl RuntimeServer {
    fn finish(self) -> Vec<String> {
        self.handle.join().unwrap()
    }
}

fn serve_runtime(
    replies: Vec<ServerReply>,
    first_request_seen: Option<Sender<()>>,
    first_request_release: Option<Receiver<()>>,
) -> RuntimeServer {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let mut seen = first_request_seen;
        let mut release = first_request_release;
        let mut requests = Vec::with_capacity(replies.len());
        for (index, reply) in replies.into_iter().enumerate() {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            requests.push(read_request(&mut stream));
            if index == 0 {
                if let Some(sender) = seen.take() {
                    sender.send(()).unwrap();
                }
                if let Some(receiver) = release.take() {
                    receiver.recv_timeout(Duration::from_secs(5)).unwrap();
                }
            }
            write!(
                stream,
                "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                reply.status,
                reply.body.len(),
                reply.body
            )
            .unwrap();
            stream.flush().unwrap();
        }
        requests
    });
    RuntimeServer {
        base_url: format!("http://{address}/v1"),
        handle,
    }
}

fn read_request(stream: &mut TcpStream) -> String {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4_096];
    loop {
        let read = stream.read(&mut buffer).unwrap();
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
        let Some(header_end) = request
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map(|index| index + 4)
        else {
            continue;
        };
        let headers = String::from_utf8_lossy(&request[..header_end]);
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap();
        if request.len() >= header_end + content_length {
            break;
        }
    }
    String::from_utf8(request).unwrap()
}

fn seed_profile(root: &Path, base_url: &str, model: &str, bearer: Option<&str>) {
    let mut repository = VaultRepository::open(root, VaultKey::new(PROFILE_TEST_KEY)).unwrap();
    repository
        .update_runtime_profile(
            base_url,
            model,
            bearer.map_or(
                VaultRuntimeProfileKeyAction::Clear,
                VaultRuntimeProfileKeyAction::Replace,
            ),
        )
        .unwrap();
    let repository = seed_ready_counterpart(repository);
    repository.close().unwrap();
}

fn open_managed(root: &Path) -> ManagedHost {
    let host = HostCore::open_with_key(
        root,
        VaultKey::new(PROFILE_TEST_KEY),
        LaunchMode::Foreground,
    )
    .unwrap();
    ManagedHost {
        inner: Mutex::new(HostSlot::Ready(host)),
        vault_root: root.to_path_buf(),
        launch_mode: LaunchMode::Foreground,
        updater_configured: false,
    }
}

fn draft(
    base_url: impl Into<String>,
    model: impl Into<String>,
    api_key_change: RuntimeProfileApiKeyChange,
) -> RuntimeProfileDraft {
    RuntimeProfileDraft {
        base_url: base_url.into(),
        model: model.into(),
        api_key_change,
    }
}

fn assert_runtime_request(request: &str, model: &str, bearer: Option<&str>) {
    let (headers, body) = request.split_once("\r\n\r\n").unwrap();
    let lowercase_headers = headers.to_ascii_lowercase();
    assert!(body.contains(&format!(r#""model":"{model}""#)));
    match bearer {
        Some(bearer) => {
            assert!(lowercase_headers.contains(&format!(
                "authorization: bearer {}",
                bearer.to_ascii_lowercase()
            )));
            assert!(!body.contains(bearer));
        }
        None => assert!(!lowercase_headers.contains("authorization:")),
    }
}

fn assert_strict_classification_request(request: &str) {
    assert!(request.contains(r#""name":"eam_person_turn_classification_v1""#));
    assert!(request.contains(r#""strict":true"#));
}

#[test]
fn strict_profile_test_uses_only_synthetic_input_without_persisting_or_switching() {
    let _guard = sqlcipher_test_lock();
    let directory = tempdir().unwrap();
    let active_server = serve_runtime(vec![ServerReply::ok(CLASSIFICATION_RESPONSE)], None, None);
    let candidate_server =
        serve_runtime(vec![ServerReply::ok(CLASSIFICATION_RESPONSE)], None, None);
    seed_profile(
        directory.path(),
        &active_server.base_url,
        "old-model",
        Some(OLD_BEARER),
    );
    let managed = open_managed(directory.path());
    let before = managed.get_runtime_profile().unwrap();
    assert_eq!(before.model, "old-model");
    assert_eq!(before.api_key_last_four.as_deref(), Some("1111"));

    let tested = managed
        .test_runtime_profile(&draft(
            candidate_server.base_url.clone(),
            "candidate-model",
            RuntimeProfileApiKeyChange::Keep,
        ))
        .unwrap();
    assert!(tested.succeeded);
    {
        let mut slot = managed.lock();
        let HostSlot::Ready(host) = &mut *slot else {
            panic!("managed host should stay ready");
        };
        assert!(
            host.core
                .repository()
                .all_evidence()
                .unwrap()
                .iter()
                .all(|evidence| evidence.session_id().as_str() == "desktop-test-onboarding")
        );
        let persisted = host.core.repository().runtime_profile().unwrap();
        assert_eq!(persisted.model(), "old-model");
        assert_eq!(persisted.bearer_key(), Some(OLD_BEARER));
        host.core
            .record_person_turn(
                SessionId::new("active-after-profile-test"),
                "active runtime remains selected",
            )
            .unwrap();
    }
    managed.shutdown(ExitReason::Explicit).unwrap();

    let candidate_requests = candidate_server.finish();
    assert_eq!(candidate_requests.len(), 1);
    assert_runtime_request(&candidate_requests[0], "candidate-model", Some(OLD_BEARER));
    assert_strict_classification_request(&candidate_requests[0]);
    assert!(candidate_requests[0].contains(RUNTIME_PROFILE_TEST_INPUT));
    assert!(!candidate_requests[0].contains("active runtime remains selected"));

    let active_requests = active_server.finish();
    assert_eq!(active_requests.len(), 1);
    assert_runtime_request(&active_requests[0], "old-model", Some(OLD_BEARER));
    assert!(active_requests[0].contains("active runtime remains selected"));
}

#[test]
fn clear_profile_test_omits_auth_without_clearing_the_persisted_key() {
    let _guard = sqlcipher_test_lock();
    let directory = tempdir().unwrap();
    let candidate_server =
        serve_runtime(vec![ServerReply::ok(CLASSIFICATION_RESPONSE)], None, None);
    seed_profile(
        directory.path(),
        "http://127.0.0.1:9/v1",
        "old-model",
        Some(OLD_BEARER),
    );
    let managed = open_managed(directory.path());

    let tested = managed
        .test_runtime_profile(&draft(
            candidate_server.base_url.clone(),
            "clear-candidate-model",
            RuntimeProfileApiKeyChange::Clear,
        ))
        .unwrap();
    assert!(tested.succeeded);
    let persisted = managed.get_runtime_profile().unwrap();
    assert_eq!(persisted.model, "old-model");
    assert!(persisted.api_key_configured);
    assert_eq!(persisted.api_key_last_four.as_deref(), Some("1111"));
    managed.shutdown(ExitReason::Explicit).unwrap();

    let candidate_requests = candidate_server.finish();
    assert_eq!(candidate_requests.len(), 1);
    assert_runtime_request(&candidate_requests[0], "clear-candidate-model", None);
    assert_strict_classification_request(&candidate_requests[0]);
}

#[test]
fn profile_test_failure_is_sanitized_and_keeps_the_active_profile() {
    let _guard = sqlcipher_test_lock();
    let directory = tempdir().unwrap();
    let active_server = serve_runtime(vec![ServerReply::ok(CLASSIFICATION_RESPONSE)], None, None);
    let rejected_body = "provider-body-secret-9999";
    let candidate_server = serve_runtime(vec![ServerReply::rejected(rejected_body)], None, None);
    seed_profile(
        directory.path(),
        &active_server.base_url,
        "old-model",
        Some(OLD_BEARER),
    );
    let managed = open_managed(directory.path());

    let error = managed
        .test_runtime_profile(&draft(
            candidate_server.base_url.clone(),
            "rejected-model",
            RuntimeProfileApiKeyChange::Replace(NEW_BEARER.to_owned()),
        ))
        .unwrap_err();
    assert_eq!(error, "runtime profile test failed");
    assert!(!error.contains(NEW_BEARER));
    assert!(!error.contains(rejected_body));
    {
        let mut slot = managed.lock();
        let HostSlot::Ready(host) = &mut *slot else {
            panic!("managed host should stay ready");
        };
        assert!(
            host.core
                .repository()
                .all_evidence()
                .unwrap()
                .iter()
                .all(|evidence| evidence.session_id().as_str() == "desktop-test-onboarding")
        );
        host.core
            .record_person_turn(
                SessionId::new("active-after-rejected-test"),
                "old runtime still handles requests",
            )
            .unwrap();
    }
    assert_eq!(managed.get_runtime_profile().unwrap().model, "old-model");
    managed.shutdown(ExitReason::Explicit).unwrap();

    let candidate_requests = candidate_server.finish();
    assert_runtime_request(&candidate_requests[0], "rejected-model", Some(NEW_BEARER));
    let active_requests = active_server.finish();
    assert_runtime_request(&active_requests[0], "old-model", Some(OLD_BEARER));
}

#[test]
fn failed_vault_commit_keeps_both_the_old_profile_and_runtime() {
    let _guard = sqlcipher_test_lock();
    let directory = tempdir().unwrap();
    let active_server = serve_runtime(vec![ServerReply::ok(CLASSIFICATION_RESPONSE)], None, None);
    seed_profile(
        directory.path(),
        &active_server.base_url,
        "old-model",
        Some(OLD_BEARER),
    );
    let managed = open_managed(directory.path());
    let candidate = draft(
        "http://127.0.0.1:9/v1",
        "candidate-model",
        RuntimeProfileApiKeyChange::Replace(NEW_BEARER.to_owned()),
    );
    let mut persist_called = false;
    {
        let mut slot = managed.lock();
        let HostSlot::Ready(host) = &mut *slot else {
            panic!("managed host should stay ready");
        };
        let error = save_runtime_profile_from_core(
            &mut host.core,
            &candidate,
            |_, _| -> Result<VaultRuntimeProfileView, String> {
                persist_called = true;
                Err("synthetic Vault commit failure".to_owned())
            },
        )
        .unwrap_err();
        assert_eq!(error, "synthetic Vault commit failure");
        let persisted = host.core.repository().runtime_profile().unwrap();
        assert_eq!(persisted.model(), "old-model");
        assert_eq!(persisted.bearer_key(), Some(OLD_BEARER));
        host.core
            .record_person_turn(
                SessionId::new("active-after-failed-save"),
                "failed save keeps old runtime",
            )
            .unwrap();
    }
    assert!(persist_called);
    managed.shutdown(ExitReason::Explicit).unwrap();

    let active_requests = active_server.finish();
    assert_eq!(active_requests.len(), 1);
    assert_runtime_request(&active_requests[0], "old-model", Some(OLD_BEARER));
}

#[test]
fn saved_profile_drives_the_next_request_and_a_reopened_host() {
    let _guard = sqlcipher_test_lock();
    let directory = tempdir().unwrap();
    let new_server = serve_runtime(
        vec![
            ServerReply::ok(CLASSIFICATION_RESPONSE),
            ServerReply::ok(TURN_RESPONSE),
            ServerReply::ok(CLASSIFICATION_RESPONSE),
            ServerReply::ok(TURN_RESPONSE),
        ],
        None,
        None,
    );
    seed_profile(
        directory.path(),
        "http://127.0.0.1:9/v1",
        "old-model",
        Some(OLD_BEARER),
    );
    let managed = open_managed(directory.path());
    let saved = managed
        .save_runtime_profile(&draft(
            new_server.base_url.clone(),
            "new-model",
            RuntimeProfileApiKeyChange::Replace(NEW_BEARER.to_owned()),
        ))
        .unwrap();
    assert_eq!(saved.model, "new-model");
    assert_eq!(saved.api_key_last_four.as_deref(), Some("2222"));
    managed
        .send_message("first request after save".to_owned())
        .unwrap();
    managed.shutdown(ExitReason::Explicit).unwrap();

    let reopened = open_managed(directory.path());
    let reopened_view = reopened.get_runtime_profile().unwrap();
    assert_eq!(reopened_view.model, "new-model");
    assert_eq!(reopened_view.api_key_last_four.as_deref(), Some("2222"));
    reopened
        .send_message("request after host reopen".to_owned())
        .unwrap();
    reopened.shutdown(ExitReason::Explicit).unwrap();

    let requests = new_server.finish();
    assert_eq!(requests.len(), 4);
    for request in &requests {
        assert_runtime_request(request, "new-model", Some(NEW_BEARER));
    }
    assert_strict_classification_request(&requests[0]);
    assert!(requests[1].contains(r#""name":"eam_runtime_response_v1""#));
    assert_strict_classification_request(&requests[2]);
    assert!(requests[3].contains(r#""name":"eam_runtime_response_v1""#));
}

#[test]
fn in_flight_request_serializes_save_and_never_mixes_profiles() {
    let _guard = sqlcipher_test_lock();
    let directory = tempdir().unwrap();
    let (first_request_seen_tx, first_request_seen_rx) = mpsc::channel();
    let (release_first_request_tx, release_first_request_rx) = mpsc::channel();
    let old_server = serve_runtime(
        vec![
            ServerReply::ok(CLASSIFICATION_RESPONSE),
            ServerReply::ok(TURN_RESPONSE),
        ],
        Some(first_request_seen_tx),
        Some(release_first_request_rx),
    );
    let new_server = serve_runtime(
        vec![
            ServerReply::ok(CLASSIFICATION_RESPONSE),
            ServerReply::ok(TURN_RESPONSE),
        ],
        None,
        None,
    );
    seed_profile(
        directory.path(),
        &old_server.base_url,
        "old-model",
        Some(OLD_BEARER),
    );
    let managed = Arc::new(open_managed(directory.path()));
    let request_host = Arc::clone(&managed);
    let request = thread::spawn(move || {
        request_host
            .send_message("request held under the old profile".to_owned())
            .unwrap();
    });
    first_request_seen_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap();

    let (save_started_tx, save_started_rx) = mpsc::channel();
    let (save_finished_tx, save_finished_rx) = mpsc::channel();
    let save_host = Arc::clone(&managed);
    let new_base_url = new_server.base_url.clone();
    let save = thread::spawn(move || {
        save_started_tx.send(()).unwrap();
        let result = save_host.save_runtime_profile(&draft(
            new_base_url,
            "new-model",
            RuntimeProfileApiKeyChange::Replace(NEW_BEARER.to_owned()),
        ));
        save_finished_tx.send(result).unwrap();
    });
    save_started_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    assert!(
        save_finished_rx
            .recv_timeout(Duration::from_millis(150))
            .is_err(),
        "save must wait for the in-flight request's host lock"
    );

    release_first_request_tx.send(()).unwrap();
    request.join().unwrap();
    let saved = save_finished_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    assert_eq!(saved.model, "new-model");
    save.join().unwrap();
    managed
        .send_message("request after serialized switch".to_owned())
        .unwrap();
    managed.shutdown(ExitReason::Explicit).unwrap();

    let old_requests = old_server.finish();
    assert_eq!(old_requests.len(), 2);
    for request in &old_requests {
        assert_runtime_request(request, "old-model", Some(OLD_BEARER));
    }
    let new_requests = new_server.finish();
    assert_eq!(new_requests.len(), 2);
    for request in &new_requests {
        assert_runtime_request(request, "new-model", Some(NEW_BEARER));
    }
}
