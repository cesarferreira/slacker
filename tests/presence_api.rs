use serde_json::{Value, json};
use slacker::api::{Presence, Slack};
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::Duration,
};

fn server(responses: Vec<(&'static str, Value)>) -> (String, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let handle = thread::spawn(move || {
        let mut requests = Vec::new();
        for (status, body) in responses {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(std::time::Instant::now() < deadline, "missing HTTP request");
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 4096];
            loop {
                let n = stream.read(&mut chunk).unwrap();
                assert_ne!(n, 0);
                request.extend_from_slice(&chunk[..n]);
                if let Some(end) = request.windows(4).position(|b| b == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]).to_lowercase();
                    let length = headers
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length: "))
                        .and_then(|length| length.parse::<usize>().ok())
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            requests.push(String::from_utf8(request).unwrap());
            let body = body.to_string();
            write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nRetry-After: 60\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
        requests
    });
    (url, handle)
}

#[test]
fn presence_reads_real_status_deduplicates_and_ignores_empty_ids() {
    let (url, server) = server(vec![
        ("200 OK", json!({"ok":true,"presence":"active"})),
        ("200 OK", json!({"ok":true,"presence":"away"})),
        ("200 OK", json!({"ok":true,"users":{}})),
    ]);
    let statuses = Slack::with_base_url("test-token".into(), url)
        .unwrap()
        .presences(&["U1".into(), "".into(), "U1".into(), "U2".into()])
        .unwrap();
    assert_eq!(statuses.len(), 2);
    assert_eq!(statuses["U1"], Presence::Active);
    assert_eq!(statuses["U2"], Presence::Away);
    let requests = server.join().unwrap();
    assert!(requests[0].starts_with("GET /users.getPresence?user=U1 "));
    assert!(requests[1].starts_with("GET /users.getPresence?user=U2 "));
    assert_eq!(requests.len(), 3);
    assert!(requests[2].starts_with("GET /dnd.teamInfo?users=U1%2CU2 "));
}

#[test]
fn missing_unrecognized_and_malformed_presence_remain_unknown() {
    let (url, server) = server(vec![
        ("200 OK", json!({"ok":true})),
        ("200 OK", json!({"ok":true,"presence":"unavailable"})),
        ("200 OK", json!({"ok":true,"presence":false})),
        ("200 OK", json!({"ok":true,"users":{}})),
    ]);
    let statuses = Slack::with_base_url("test-token".into(), url)
        .unwrap()
        .presences(&["U1".into(), "U2".into(), "U3".into()])
        .unwrap();
    assert_eq!(Presence::default(), Presence::Unknown);
    assert!(statuses.values().all(|status| *status == Presence::Unknown));
    server.join().unwrap();
}

#[test]
fn presence_errors_are_not_reported_as_away_and_stop_the_batch() {
    for response in [
        ("200 OK", json!({"ok":false,"error":"missing_scope"})),
        ("429 Too Many Requests", json!({"ok":false})),
        ("503 Service Unavailable", json!({"ok":false})),
    ] {
        let (url, server) = server(vec![response]);
        let result = Slack::with_base_url("test-token".into(), url)
            .unwrap()
            .presences(&["U1".into(), "U2".into()]);
        assert!(result.is_err());
        assert_eq!(server.join().unwrap().len(), 1);
    }
}

#[test]
fn presence_lookup_bounds_fanout_before_network_access() {
    let client = Slack::with_base_url("test-token".into(), "http://127.0.0.1:1/".into()).unwrap();
    assert!(client.presences(&[]).unwrap().is_empty());
    assert!(client.presences(&["".into()]).unwrap().is_empty());
    let users: Vec<_> = (0..11).map(|i| format!("U{i}")).collect();
    assert!(
        client
            .presences(&users)
            .unwrap_err()
            .to_string()
            .contains("10")
    );
}

#[test]
fn desktop_presence_uses_existing_cookie_and_form_transport() {
    let (url, server) = server(vec![
        ("200 OK", json!({"ok":true,"presence":"active"})),
        ("200 OK", json!({"ok":true,"users":{}})),
    ]);
    let statuses = Slack::with_session_base_url("xoxc-test".into(), "xoxd-test".into(), url)
        .unwrap()
        .presences(&["U1".into()])
        .unwrap();
    assert_eq!(statuses["U1"], Presence::Active);
    let requests = server.join().unwrap();
    let (headers, body) = requests[0].split_once("\r\n\r\n").unwrap();
    assert!(headers.starts_with("POST /users.getPresence "));
    assert!(headers.to_lowercase().contains("cookie: d=xoxd-test"));
    assert!(!headers.contains("xoxc-test"));
    assert_eq!(body, "user=U1&token=xoxc-test");
    assert!(requests[1].starts_with("POST /dnd.teamInfo "));
    assert!(requests[1].ends_with("users=U1&token=xoxc-test"));
}

#[test]
fn bootstrap_retains_profile_status_without_extra_requests() {
    let (url, server) = server(vec![
        ("200 OK", json!({"ok":true,"team":"Acme","user_id":"U1"})),
        (
            "200 OK",
            json!({"ok":true,"members":[
                {"id":"U1","real_name":"Alice","profile":{"display_name":"Ali","status_emoji":":headphones:","status_text":"Focusing"}},
                {"id":"U2","name":"helper","is_bot":true,"profile":{}}
            ]}),
        ),
        ("200 OK", json!({"ok":true,"channels":[]})),
    ]);
    let snapshot = Slack::with_base_url("test-token".into(), url)
        .unwrap()
        .bootstrap()
        .unwrap();
    assert_eq!(snapshot.people["U1"].display_name, "Ali");
    assert_eq!(snapshot.people["U1"].status_emoji, ":headphones:");
    assert_eq!(snapshot.people["U1"].status_text, "Focusing");
    assert!(!snapshot.people["U1"].is_bot);
    assert_eq!(snapshot.people["U2"].display_name, "helper");
    assert!(snapshot.people["U2"].is_bot);
    assert!(snapshot.people["U2"].status_emoji.is_empty());
    assert_eq!(snapshot.users["U1"], "Ali");
    assert_eq!(server.join().unwrap().len(), 3);
}

#[test]
fn dnd_only_overrides_presence_while_suppression_is_current() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    for (dnd, expected) in [
        (
            json!({"dnd_enabled":true,"next_dnd_start_ts":now + 3600,"next_dnd_end_ts":now + 7200}),
            Presence::Active,
        ),
        (
            json!({"dnd_enabled":true,"next_dnd_start_ts":now - 3600,"next_dnd_end_ts":now + 3600}),
            Presence::DoNotDisturb,
        ),
        (
            json!({"dnd_enabled":true,"next_dnd_start_ts":now - 7200,"next_dnd_end_ts":now - 3600}),
            Presence::Active,
        ),
        (
            json!({"dnd_enabled":false,"next_dnd_start_ts":now - 3600,"next_dnd_end_ts":now + 3600}),
            Presence::Active,
        ),
        (
            json!({"dnd_enabled":false,"snooze_enabled":true,"snooze_endtime":now + 3600}),
            Presence::DoNotDisturb,
        ),
        (
            json!({"snooze_active":true,"snooze_endtime":now + 3600}),
            Presence::DoNotDisturb,
        ),
        (
            json!({"snooze_enabled":true,"snooze_endtime":now - 3600}),
            Presence::Active,
        ),
        (json!({"dnd_enabled":true}), Presence::Active),
        (json!({}), Presence::Active),
    ] {
        let (url, server) = server(vec![
            ("200 OK", json!({"ok":true,"presence":"active"})),
            ("200 OK", json!({"ok":true,"users":{"U1":dnd}})),
        ]);
        let statuses = Slack::with_base_url("test-token".into(), url)
            .unwrap()
            .presences(&["U1".into()])
            .unwrap();
        assert_eq!(statuses["U1"], expected, "DND response: {dnd}");
        server.join().unwrap();
    }
}

#[test]
fn dnd_errors_and_missing_users_preserve_base_presence() {
    for response in [
        ("200 OK", json!({"ok":false,"error":"missing_scope"})),
        ("429 Too Many Requests", json!({"ok":false})),
        ("503 Service Unavailable", json!({"ok":false})),
        ("200 OK", json!({"ok":true,"users":{}})),
        ("200 OK", json!({"ok":true})),
    ] {
        let (url, server) = server(vec![
            ("200 OK", json!({"ok":true,"presence":"active"})),
            ("200 OK", json!({"ok":true,"presence":"away"})),
            response,
        ]);
        let statuses = Slack::with_base_url("test-token".into(), url)
            .unwrap()
            .presences(&["U1".into(), "U2".into()])
            .unwrap();
        assert_eq!(statuses["U1"], Presence::Active);
        assert_eq!(statuses["U2"], Presence::Away);
        assert_eq!(server.join().unwrap().len(), 3);
    }
}

#[test]
fn desktop_unreads_parse_all_conversation_kinds_with_indicator_fallback() {
    let (url, server) = server(vec![(
        "200 OK",
        json!({
            "ok":true,
            "channels":[
                {"id":"C1","has_unreads":true,"unread_count_display":5,"unread_count":9},
                {"id":"C2","has_unreads":false,"unread_count_display":0},
                {"id":"C3","has_unreads":true,"unread_count":3},
                {"id":"C4","has_unreads":true,"unread_count_display":"bad","unread_count":2},
                {"id":"C5"},
                {"id":"","has_unreads":true},
                {"has_unreads":true}
            ],
            "ims":[{"id":"D1","has_unreads":true},{"id":"D2","has_unreads":false}],
            "mpims":[{"id":"G1","has_unreads":true}],
            "threads":{"has_unreads":true,"unread_count":42}
        }),
    )]);
    let counts = Slack::with_session_base_url("xoxc-test".into(), "xoxd-test".into(), url)
        .unwrap()
        .unread_counts()
        .unwrap();
    assert_eq!(counts.len(), 7);
    assert_eq!(counts["C1"], 5);
    assert_eq!(counts["C2"], 0);
    assert_eq!(counts["C3"], 3);
    assert_eq!(counts["C4"], 2);
    assert_eq!(counts["D1"], 1);
    assert_eq!(counts["D2"], 0);
    assert_eq!(counts["G1"], 1);
    assert!(!counts.contains_key("C5"));
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 1);
    let (headers, body) = requests[0].split_once("\r\n\r\n").unwrap();
    assert!(headers.starts_with("POST /client.counts "));
    assert!(headers.to_lowercase().contains("cookie: d=xoxd-test"));
    assert_eq!(body, "token=xoxc-test");
}

#[test]
fn unreads_preserve_unknown_when_endpoint_errors_or_collection_is_missing() {
    for response in [
        (
            "200 OK",
            json!({"ok":false,"error":"enterprise_is_restricted"}),
        ),
        ("429 Too Many Requests", json!({"ok":false})),
        ("200 OK", json!({"ok":true})),
        ("200 OK", json!({"ok":true,"channels":{}})),
    ] {
        let (url, server) = server(vec![response]);
        let result = Slack::with_session_base_url("xoxc-test".into(), "xoxd-test".into(), url)
            .unwrap()
            .unread_counts();
        assert!(result.is_err());
        server.join().unwrap();
    }
    let (url, server) = server(vec![(
        "200 OK",
        json!({"ok":true,"channels":[],"ims":[],"mpims":[]}),
    )]);
    assert!(
        Slack::with_session_base_url("xoxc-test".into(), "xoxd-test".into(), url)
            .unwrap()
            .unread_counts()
            .unwrap()
            .is_empty()
    );
    server.join().unwrap();
}

#[test]
fn unread_lookup_rejects_bearer_auth_without_network_access() {
    let client = Slack::with_base_url("test-token".into(), "http://127.0.0.1:1/".into()).unwrap();
    assert!(
        client
            .unread_counts()
            .unwrap_err()
            .to_string()
            .contains("desktop")
    );
}
