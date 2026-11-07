use serde_json::json;
use slacker::{
    api::Slack,
    app::{App, Reply},
};
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

fn server(
    responses: Vec<(&'static str, serde_json::Value)>,
) -> (String, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let handle = thread::spawn(move || {
        let mut requests = Vec::new();
        for (status, body) in responses {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 4096];
            loop {
                let n = stream.read(&mut chunk).unwrap();
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..n]);
                if let Some(end) = request.windows(4).position(|b| b == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]).to_lowercase();
                    let length = headers
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length: "))
                        .and_then(|s| s.parse::<usize>().ok())
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            requests.push(String::from_utf8(request).unwrap());
            let body = body.to_string();
            write!(stream,"HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nRetry-After: 42\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        }
        requests
    });
    (url, handle)
}

#[test]
fn bootstrap_paginates_channels_and_resolves_dm_names() {
    let (url, server) = server(vec![
        ("200 OK", json!({"ok":true,"team":"Test","user_id":"U1"})),
        (
            "200 OK",
            json!({"ok":true,"members":[{"id":"U2","profile":{"display_name":"Maya"}}]}),
        ),
        (
            "200 OK",
            json!({"ok":true,"channels":[{"id":"C1","name":"engineering","is_member":true},{"id":"C2","name":"unjoined","is_member":false}],"response_metadata":{"next_cursor":"second"}}),
        ),
        (
            "200 OK",
            json!({"ok":true,"channels":[{"id":"D1","user":"U2","is_im":true}]}),
        ),
    ]);
    let client = Slack::with_base_url("test-token".into(), url).unwrap();
    let data = client.bootstrap().unwrap();
    assert_eq!(data.conversations.len(), 2);
    assert_eq!(data.conversations[1].label(), "@Maya");
    let requests = server.join().unwrap();
    assert!(requests.iter().all(|r| {
        r.to_lowercase()
            .contains("authorization: bearer test-token")
    }));
    assert!(
        requests[3]
            .lines()
            .next()
            .unwrap()
            .contains("cursor=second")
    );
    assert!(
        !requests
            .iter()
            .any(|r| r.lines().next().unwrap().contains("test-token"))
    );
}

#[test]
fn thread_send_uses_json_and_preserves_unicode_and_thread_target() {
    let (url, server) = server(vec![(
        "200 OK",
        json!({"ok":true,"ts":"1731000200.000001"}),
    )]);
    Slack::with_base_url("test-token".into(), url)
        .unwrap()
        .send("C1", Some("1731000000.000001"), "hello 🦀\nworld")
        .unwrap();
    let requests = server.join().unwrap();
    assert!(requests[0].starts_with("POST /chat.postMessage "));
    let (_, body) = requests[0].split_once("\r\n\r\n").unwrap();
    let body: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(
        body,
        json!({"channel":"C1","thread_ts":"1731000000.000001","text":"hello 🦀\nworld"})
    );
}

#[test]
fn rate_limit_reports_retry_after_without_retrying_post() {
    let (url, server) = server(vec![("429 Too Many Requests", json!({"ok":false}))]);
    let error = Slack::with_base_url("test-token".into(), url)
        .unwrap()
        .send("C1", None, "hello")
        .unwrap_err();
    assert!(error.to_string().contains("42s"));
    assert_eq!(server.join().unwrap().len(), 1);
}

#[test]
fn failed_send_preserves_draft_and_success_does_not_erase_a_different_draft() {
    let mut app = App::demo(false);
    app.editor.insert("keep this 🦀");
    let channel = app.current_id().unwrap().to_owned();
    app.sending = true;
    app.receive(Reply::Sent {
        channel: channel.clone(),
        thread: None,
        text: "keep this 🦀".into(),
        result: Err(anyhow::anyhow!("network failure")),
    });
    assert_eq!(app.editor.text, "keep this 🦀");
    assert!(!app.sending);
    app.receive(Reply::Sent {
        channel,
        thread: None,
        text: "a previous draft".into(),
        result: Ok(slacker::api::Message {
            ts: "1731000200.000001".into(),
            ..Default::default()
        }),
    });
    assert_eq!(app.editor.text, "keep this 🦀");
}

#[test]
fn thread_incremental_fetch_uses_the_last_loaded_timestamp() {
    let (url, server) = server(vec![("200 OK", json!({"ok":true,"messages":[]}))]);
    Slack::with_base_url("test-token".into(), url)
        .unwrap()
        .history(
            "C1",
            Some("1731000000.000001"),
            "",
            Some("1731000060.000001"),
        )
        .unwrap();
    let requests = server.join().unwrap();
    let request = requests[0].lines().next().unwrap();
    assert!(request.starts_with("GET /conversations.replies?"));
    assert!(request.contains("oldest=1731000060.000001"));
    assert!(request.contains("ts=1731000000.000001"));
}

#[test]
fn desktop_session_uses_cookie_and_form_auth_without_secrets_in_the_url() {
    let (url, server) = server(vec![("200 OK", json!({"ok":true}))]);
    Slack::with_session_base_url("xoxc-unit-test".into(), "xoxd-unit-test".into(), url)
        .unwrap()
        .validate()
        .unwrap();
    let requests = server.join().unwrap();
    assert!(requests[0].starts_with("POST /auth.test HTTP/1.1"));
    assert!(
        requests[0]
            .to_lowercase()
            .contains("cookie: d=xoxd-unit-test")
    );
    let (headers, body) = requests[0].split_once("\r\n\r\n").unwrap();
    assert!(!headers.contains("xoxc-unit-test"));
    assert_eq!(body, "token=xoxc-unit-test");
}

#[test]
fn enterprise_session_routes_to_the_trusted_workspace_api_host() {
    use slacker::api::session_api_url;
    assert_eq!(
        session_api_url("https://acme.enterprise.slack.com/").unwrap(),
        "https://acme.enterprise.slack.com/api/"
    );
    assert!(session_api_url("https://slack.com.attacker.test/").is_err());
    assert!(session_api_url("http://acme.slack.com/").is_err());
    assert!(session_api_url("https://user:password@acme.slack.com/").is_err());
}

#[test]
fn desktop_bootstrap_lists_joined_conversations_without_enterprise_restricted_enumeration() {
    let (url, server) = server(vec![
        ("200 OK", json!({"ok":true,"team":"Acme","user_id":"U1"})),
        ("200 OK", json!({"ok":true,"members":[]})),
        (
            "200 OK",
            json!({"ok":true,"channels":[{"id":"C1","name":"engineering"}]}),
        ),
    ]);
    let snapshot =
        Slack::with_session_base_url("xoxc-unit-test".into(), "xoxd-unit-test".into(), url)
            .unwrap()
            .bootstrap()
            .unwrap();
    assert_eq!(snapshot.conversations.len(), 1);
    let requests = server.join().unwrap();
    assert!(requests[2].starts_with("POST /client.userBoot "));
}
