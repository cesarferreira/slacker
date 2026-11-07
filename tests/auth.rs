use slacker::auth;
use std::{fs, path::Path};
fn hex(s: &str) -> Vec<u8> {
    s.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}
#[test]
fn workspaces_are_sorted_and_unsafe_domains_are_rejected() {
    let parsed=auth::parse_workspaces(br#"{"workspaces":{"T2":{"id":"T2","name":"Zulu","domain":"zulu"},"T1":{"id":"T1","name":"Acme","domain":"acme"},"T3":{"id":"T3","name":"Bad","domain":"evil.test/slack"}}}"#,Path::new("/profile")).unwrap();
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].team_id, "T1");
    assert_eq!(parsed[0].domain, "acme");
}
#[test]
fn chromium_values_decode_latin1_and_utf16() {
    assert_eq!(
        auth::decode_local_value(b"\x01{\"name\":\"Slack\"}").unwrap(),
        "{\"name\":\"Slack\"}"
    );
    let mut utf16 = vec![0];
    for unit in "{\"name\":\"世界\"}".encode_utf16() {
        utf16.extend_from_slice(&unit.to_le_bytes());
    }
    assert_eq!(
        auth::decode_local_value(&utf16).unwrap(),
        "{\"name\":\"世界\"}"
    );
    assert!(auth::decode_local_value(&[0, 1]).is_err());
}
#[test]
fn chromium_cookies_decrypt_with_and_without_domain_binding() {
    for encrypted in [
        "763130dca7a4e527f6be14955eb7219e11bdf904b3cecde95d45dec46d2febb85e8b0f",
        "763130f054818f9ca6ad69c65f974f7ec74780a7f67612e0e21c4fb71761de45cfd4fbf491124a8187944f8d9ba8332541f34fcfde3277f81e97819c7c9e9c0142e565",
    ] {
        assert_eq!(
            auth::decrypt_cookie(&hex(encrypted), b"test-storage-password", 1003).unwrap(),
            "xoxd-unit-test-session"
        );
        assert!(auth::decrypt_cookie(&hex(encrypted), b"wrong-password", 1003).is_err());
    }
    assert!(auth::decrypt_cookie(b"v20invalid", b"password", 1003).is_err());
}
#[test]
fn leveldb_sessions_are_read_from_a_copy_while_the_original_stays_open() {
    use rusty_leveldb::{DB, Options};
    let profile = tempfile::tempdir().unwrap();
    let path = profile.path().join("Local Storage/leveldb");
    fs::create_dir_all(&path).unwrap();
    let mut db = DB::open(&path, Options::default()).unwrap();
    db.put(
        b"_https://app.slack.com\x00\x01localConfig_v2",
        b"\x01{\"teams\":{\"T1\":{\"token\":\"xoxc-unit-test\"}}}",
    )
    .unwrap();
    db.flush().unwrap();
    let sessions = auth::read_tokens(profile.path()).unwrap();
    assert_eq!(
        sessions.get("T1").map(String::as_str),
        Some("xoxc-unit-test")
    );
    assert!(
        db.get(b"_https://app.slack.com\x00\x01localConfig_v2")
            .is_some()
    );
}
#[test]
fn cookie_snapshot_includes_uncheckpointed_wal_values() {
    let profile = tempfile::tempdir().unwrap();
    let db = rusqlite::Connection::open(profile.path().join("Cookies")).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE cookies(host_key TEXT,name TEXT,value TEXT,encrypted_value BLOB); INSERT INTO cookies VALUES('.slack.com','d','xoxd-unit-test',X'');").unwrap();
    assert_eq!(
        auth::read_cookie(profile.path()).unwrap().0,
        "xoxd-unit-test"
    );
}
