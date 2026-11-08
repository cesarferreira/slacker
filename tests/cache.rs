use slacker::{
    api::{Conversation, Message, Snapshot},
    cache::{Cache, Feed, MAX_FEEDS, MAX_MESSAGES},
};
use std::collections::HashMap;

fn snapshot() -> Snapshot {
    Snapshot {
        workspace: "Example".into(),
        me: "U1".into(),
        conversations: vec![Conversation {
            id: "C1".into(),
            name: "general".into(),
            ..Default::default()
        }],
        users: HashMap::from([("U1".into(), "Ada".into())]),
        people: HashMap::new(),
    }
}

fn feed(text: &str) -> Feed {
    Feed {
        messages: vec![Message {
            ts: "1700000000.000001".into(),
            text: text.into(),
            ..Default::default()
        }],
        cursor: "older-page".into(),
        selected_ts: Some("1700000000.000001".into()),
        read_scroll: 4,
    }
}

#[test]
fn sqlite_cache_survives_reopen_and_separates_accounts_channels_and_threads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache/history.sqlite3");
    {
        let mut cache = Cache::open(&path, "account-a").unwrap();
        assert!(cache.load_startup().unwrap().is_none());
        cache.save_snapshot(&snapshot()).unwrap();
        cache.select_channel("C1").unwrap();
        cache.save_feed("C1", None, &feed("channel")).unwrap();
        cache.save_feed("C1", Some("root"), &feed("reply")).unwrap();
        cache.save_feed("C2", None, &feed("other channel")).unwrap();
    }
    let cache = Cache::open(&path, "account-a").unwrap();
    let startup = cache.load_startup().unwrap().unwrap();
    assert_eq!(startup.snapshot.users["U1"], "Ada");
    assert_eq!(startup.channel, "C1");
    let restored = cache.load_feed("C1", None).unwrap().unwrap();
    assert_eq!(restored.messages[0].text, "channel");
    assert_eq!(restored.selected_ts.as_deref(), Some("1700000000.000001"));
    assert_eq!(restored.read_scroll, 4);
    assert_eq!(restored.cursor, "older-page");
    assert_eq!(
        cache
            .load_feed("C1", Some("root"))
            .unwrap()
            .unwrap()
            .messages[0]
            .text,
        "reply"
    );
    assert_eq!(
        cache.load_feed("C2", None).unwrap().unwrap().messages[0].text,
        "other channel"
    );
    let mut other = Cache::open(&path, "account-b").unwrap();
    assert!(other.load_startup().unwrap().is_none());
    assert!(other.load_feed("C1", None).unwrap().is_none());
    other.save_feed("C1", None, &feed("other account")).unwrap();
    assert_eq!(
        cache.load_feed("C1", None).unwrap().unwrap().messages[0].text,
        "channel"
    );
}

#[test]
fn cache_replaces_edited_messages_and_removes_missing_rows() {
    let dir = tempfile::tempdir().unwrap();
    let mut cache = Cache::open(&dir.path().join("history.sqlite3"), "a").unwrap();
    let mut original = feed("before");
    original.messages.push(Message {
        ts: "1700000001.000001".into(),
        text: "deleted".into(),
        ..Default::default()
    });
    cache.save_feed("C1", None, &original).unwrap();
    cache.save_feed("C1", None, &feed("edited")).unwrap();
    let result = cache.load_feed("C1", None).unwrap().unwrap();
    assert_eq!(result.messages.len(), 1);
    assert_eq!(result.messages[0].text, "edited");
}

#[test]
fn cache_retention_bounds_messages_and_least_recently_used_feeds() {
    let dir = tempfile::tempdir().unwrap();
    let mut cache = Cache::open(&dir.path().join("history.sqlite3"), "a").unwrap();
    let mut large = feed("large");
    large.messages = (0..MAX_MESSAGES + 5)
        .map(|i| Message {
            ts: format!("{i:010}.000001"),
            ..Default::default()
        })
        .collect();
    cache.save_feed("C1", None, &large).unwrap();
    let restored = cache.load_feed("C1", None).unwrap().unwrap();
    assert_eq!(restored.messages.len(), MAX_MESSAGES);
    assert_eq!(restored.messages[0].ts, "0000000005.000001");
    assert!(
        restored.cursor.is_empty(),
        "trimming must not leave a cursor that skips uncached messages"
    );
    for i in 0..MAX_FEEDS {
        cache
            .save_feed(&format!("C{i:04}"), None, &feed("small"))
            .unwrap();
    }
    assert!(cache.load_feed("C1", None).unwrap().is_none());
    assert!(
        cache
            .load_feed(&format!("C{:04}", MAX_FEEDS - 1), None)
            .unwrap()
            .is_some()
    );
}

#[test]
fn unused_cached_history_expires_after_thirty_days() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.sqlite3");
    let mut cache = Cache::open(&path, "a").unwrap();
    cache.save_snapshot(&snapshot()).unwrap();
    cache.save_feed("C1", None, &feed("old")).unwrap();
    drop(cache);
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute(
        "UPDATE feeds SET touched = unixepoch() - 31 * 24 * 60 * 60",
        [],
    )
    .unwrap();
    db.execute(
        "UPDATE accounts SET updated_at = unixepoch() - 31 * 24 * 60 * 60",
        [],
    )
    .unwrap();
    drop(db);
    let cache = Cache::open(&path, "a").unwrap();
    assert!(cache.load_startup().unwrap().is_none());
    assert!(cache.load_feed("C1", None).unwrap().is_none());
    let db = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM messages", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[cfg(unix)]
#[test]
fn cache_files_are_private_and_symlinks_are_rejected() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache/history.sqlite3");
    Cache::open(&path, "a").unwrap();
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    let link = dir.path().join("link.sqlite3");
    symlink(&path, &link).unwrap();
    assert!(Cache::open(&link, "a").is_err());
}
