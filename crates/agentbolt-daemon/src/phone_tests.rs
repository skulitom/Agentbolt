//! Wire-level checks against an ephemeral loopback mock. Production settings still require HTTPS;
//! these private-function tests use synthetic credentials and do not contact a phone service.
use super::*;
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::Instant,
};

struct Reply {
    status: u16,
    body: String,
    headers: String,
    declared_extra: usize,
}
impl Reply {
    fn new(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            body: body.into(),
            headers: String::new(),
            declared_extra: 0,
        }
    }
}
#[derive(Clone)]
struct Request {
    target: String,
    headers: BTreeMap<String, String>,
    body: String,
}
fn read_request(stream: &TcpStream) -> Request {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut reader = BufReader::new(stream);
    let mut target = String::new();
    reader.read_line(&mut target).unwrap();
    let mut headers = BTreeMap::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if line == "\r\n" {
            break;
        }
        assert!(!line.is_empty() && line.len() < 16_384);
        let (key, value) = line.split_once(':').unwrap();
        headers.insert(key.to_ascii_lowercase(), value.trim().into());
    }
    let len = headers
        .get("content-length")
        .map(|v: &String| v.parse::<usize>().unwrap())
        .unwrap_or(0);
    assert!(len < 65_536);
    let mut body = vec![0; len];
    reader.read_exact(&mut body).unwrap();
    Request {
        target,
        headers,
        body: String::from_utf8(body).unwrap(),
    }
}
struct Fixture {
    config: Config,
    requests: Arc<Mutex<Vec<Request>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl Fixture {
    fn new(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let config = Config {
            settings: phone::Settings {
                enabled: true,
                server: format!("http://{}", listener.local_addr().unwrap()),
                questions: "questions".into(),
                replies: "replies".into(),
            },
            daemon_token: Secret("fixture-daemon-credential".into()),
            reply_token: Secret("fixture-phone-credential".into()),
            generation: "test-generation".into(),
            since: 100,
        };
        assert!(
            phone::server(&config.settings.server).is_err(),
            "plaintext is not an installable configuration"
        );
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let thread = std::thread::spawn(move || {
            for reply in replies {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    if stopped.load(Ordering::Relaxed) {
                        return;
                    }
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "mock request timeout");
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(e) => panic!("mock accept: {e}"),
                    }
                };
                recorded.lock().unwrap().push(read_request(&stream));
                let response = format!(
                    "HTTP/1.1 {} Fixture\r\nContent-Length: {}\r\nConnection: close\r\nContent-Type: application/json\r\n{}\r\n{}",
                    reply.status,
                    reply.body.len() + reply.declared_extra,
                    reply.headers,
                    reply.body
                );
                // A bounded reader may close before consuming an intentionally oversized fixture.
                let _ = stream.write_all(response.as_bytes());
            }
        });
        Self {
            config,
            requests,
            stop,
            thread: Some(thread),
        }
    }
    fn recorded(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap();
    }
}

#[test]
fn protection_checks_both_topics_without_publishing_or_leaking_auth_to_anonymous_probe() {
    let f = Fixture::new(vec![
        Reply::new(401, ""),
        Reply::new(200, r#"{"success":true}"#),
        Reply::new(403, ""),
        Reply::new(200, r#"{"success":true}"#),
    ]);
    protection(&f.config).unwrap();
    let requests = f.recorded();
    assert_eq!(requests.len(), 4);
    for (i, request) in requests.iter().enumerate() {
        assert!(request.target.starts_with(if i < 2 {
            "GET /questions/auth "
        } else {
            "GET /replies/auth "
        }));
        assert!(request.body.is_empty());
        if i % 2 == 0 {
            assert!(!request.headers.contains_key("authorization"));
        } else {
            assert_eq!(
                request.headers["authorization"],
                "Bearer fixture-daemon-credential"
            );
        }
    }
    let public = Fixture::new(vec![Reply::new(200, r#"{"success":true}"#)]);
    assert!(
        protection(&public.config)
            .unwrap_err()
            .contains("anonymous")
    );
    assert_eq!(public.recorded().len(), 1);
    let denied = Fixture::new(vec![
        Reply::new(401, ""),
        Reply::new(401, "fixture-daemon-credential"),
    ]);
    let error = protection(&denied.config).unwrap_err();
    assert!(!error.contains("fixture-daemon-credential"));
}

#[test]
fn question_publication_separates_daemon_credential_and_phone_action_credential() {
    let f = Fixture::new(vec![Reply::new(
        200,
        r#"{"event":"message","topic":"questions","id":"msg1"}"#,
    )]);
    publish_question(
        &f.config,
        Notification {
            ask_id: "ask_test".into(),
            toast_id: "notification1".into(),
            title: "Choose café".into(),
            body: "Complete question".into(),
            buttons: vec![("Yes".into(), Secret("fixture-answer-token".into()))],
        },
    )
    .unwrap();
    let requests = f.recorded();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert!(request.target.starts_with("POST / "));
    assert_eq!(
        request.headers["authorization"],
        "Bearer fixture-daemon-credential"
    );
    assert!(!request.body.contains("fixture-daemon-credential"));
    let payload: Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(payload["title"], "Choose café");
    assert_eq!(payload["priority"], 2);
    assert_eq!(payload["sequence_id"], "notification1");
    let action = &payload["actions"][0];
    assert_eq!(
        action["url"],
        format!("{}/replies", f.config.settings.server)
    );
    assert_eq!(action["method"], "POST");
    assert_eq!(action["body"], "fixture-answer-token");
    assert_eq!(
        action["headers"]["Authorization"],
        "Bearer fixture-phone-credential"
    );
}

#[test]
fn publish_refuses_redirects_wrong_confirmations_and_upstream_secret_echoes() {
    for response in [
        Reply::new(401, "fixture-daemon-credential fixture-answer-token"),
        Reply::new(
            200,
            r#"{"event":"message","topic":"another-topic","id":"msg1"}"#,
        ),
        Reply::new(200, "fixture-phone-credential"),
        Reply {
            headers: "Location: http://127.0.0.1:1/never-follow\r\n".into(),
            ..Reply::new(302, "")
        },
    ] {
        let f = Fixture::new(vec![response]);
        let error = publish(&f.config, json!({"topic":"questions","message":"test"})).unwrap_err();
        for secret in [
            "fixture-daemon-credential",
            "fixture-phone-credential",
            "fixture-answer-token",
            "never-follow",
        ] {
            assert!(!error.contains(secret));
        }
        assert_eq!(f.recorded().len(), 1);
    }
}

#[test]
fn poll_recovers_saved_cursor_and_ignores_old_or_unusable_messages() {
    let body=[
        json!({"event":"open"}),
        json!({"event":"message","topic":"replies","id":"old","time":99,"message":"old-token"}),
        json!({"event":"message","topic":"replies","id":"answer","time":101,"message":"fixture-answer-token"}),
        json!({"event":"message","topic":"replies","id":"large","time":102,"message":"x".repeat(129)}),
    ].iter().map(Value::to_string).collect::<Vec<_>>().join("\n");
    let f = Fixture::new(vec![Reply::new(200, body)]);
    let (messages, cursor) = poll(&f.config, Some("saved-cursor")).unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].id, "answer");
    assert_eq!(messages[0].token.0, "fixture-answer-token");
    assert_eq!(cursor.as_deref(), Some("large"));
    let requests = f.recorded();
    assert!(
        requests[0]
            .target
            .starts_with("GET /replies/json?poll=1&since=saved-cursor ")
    );
    assert_eq!(
        requests[0].headers["authorization"],
        "Bearer fixture-daemon-credential"
    );
}

#[test]
fn incomplete_or_unbounded_phone_history_never_returns_a_checkpoint() {
    for response in [
        Reply::new(
            200,
            r#"{"event":"message","topic":"other","id":"m1","time":101}"#,
        ),
        Reply::new(
            200,
            r#"{"event":"message","topic":"replies","id":"bad?cursor","time":101}"#,
        ),
        Reply::new(200, "{broken"),
        Reply::new(200, "{\"event\":\"keepalive\"}\n".repeat(501)),
        Reply::new(200, "x".repeat(2_097_153)),
        Reply {
            headers: "X-Messages-Truncated: 1\r\n".into(),
            ..Reply::new(200, "")
        },
        Reply {
            declared_extra: 100,
            ..Reply::new(200, "{\"event\":\"open\"}\n")
        },
    ] {
        let f = Fixture::new(vec![response]);
        assert!(poll(&f.config, None).is_err());
    }
}
