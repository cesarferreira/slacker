use slacker::{
    api::{History, Message, Slack, Snapshot},
    app::{App, Job, Reply, Worker},
    cache::{Cache, Event, Feed},
};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

fn message(ts: &str, text: &str) -> Message {
    Message {
        ts: ts.into(),
        text: text.into(),
        user: "maya".into(),
        ..Default::default()
    }
}

fn snapshot(app: &App) -> Snapshot {
    Snapshot {
        workspace: app.workspace.clone(),
        me: app.me.clone(),
        conversations: app.conversations.clone(),
        users: app.users.clone(),
        people: app.people.clone(),
    }
}

fn online_app() -> (App, mpsc::Receiver<Job>) {
    let mut app = App::demo(false);
    app.demo = false;
    let (jobs, requests) = mpsc::channel();
    let (events, replies) = mpsc::channel();
    app.worker = Some(Worker {
        jobs,
        replies,
        events,
        slack: Slack::new("fake".into()).unwrap(),
    });
    app.messages.clear();
    app.selected = 0;
    (app, requests)
}

fn wait(app: &mut App, condition: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !condition(app) {
        app.poll();
        assert!(Instant::now() < deadline, "timed out: {}", app.status);
        std::thread::yield_now();
    }
}

#[test]
fn cached_startup_renders_before_network_and_survives_bootstrap_failure() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.sqlite3");
    let mut original = App::demo(false);
    original.messages = vec![message("1", "cached one"), message("2", "cached two")];
    let mut db = Cache::open(&path, "a").unwrap();
    db.save_snapshot(&snapshot(&original)).unwrap();
    db.select_channel("D1").unwrap();
    db.save_feed(
        "D1",
        None,
        &Feed {
            messages: original.messages.clone(),
            selected_ts: Some("1".into()),
            cursor: "older".into(),
            read_scroll: 2,
        },
    )
    .unwrap();
    drop(db);
    let (mut app, requests) = online_app();
    app.conversations.clear();
    app.enable_cache(path, "a".into());
    wait(&mut app, |a| a.messages.len() == 2);
    assert_eq!(app.current_id(), Some("D1"));
    assert_eq!(app.selected, 0);
    assert_eq!(app.read_scroll, 2);
    assert_eq!(app.users["maya"], "Maya Chen");
    assert!(app.status.contains("cached") || app.status.contains("Cached"));
    assert!(matches!(requests.try_recv().unwrap(), Job::History { .. }));
    app.receive(Reply::Bootstrap(Err(anyhow::anyhow!("offline"))));
    assert_eq!(app.messages[0].text, "cached one");
    app.receive(Reply::Bootstrap(Ok(snapshot(&original))));
    assert_eq!(
        app.current_id(),
        Some("D1"),
        "live bootstrap must not reset the active conversation"
    );
    assert_eq!(app.messages.len(), 2);
}

#[test]
fn refresh_updates_edits_keeps_older_history_and_preserves_reading_position() {
    let (mut app, _) = online_app();
    app.messages = vec![
        message("1", "older"),
        message("2", "before"),
        message("3", "deleted"),
    ];
    app.selected = 1;
    app.cursor = "oldest-cursor".into();
    app.receive(Reply::History {
        id: app.generation,
        channel: "D0".into(),
        thread: None,
        append: false,
        result: Ok(History {
            messages: vec![message("2", "edited"), message("4", "new")],
            cursor: "recent-cursor".into(),
        }),
    });
    assert_eq!(
        app.messages
            .iter()
            .map(|m| m.text.as_str())
            .collect::<Vec<_>>(),
        ["older", "edited", "new"]
    );
    assert_eq!(app.messages[app.selected].ts, "2");
    assert_eq!(
        app.cursor, "recent-cursor",
        "fresh pagination can safely revisit cached rows instead of risking a gap"
    );
}

#[test]
fn switching_back_restores_feed_and_thread_without_waiting_for_network() {
    let (mut app, _) = online_app();
    app.messages = vec![message("1", "parent"), message("2", "last")];
    app.selected = 0;
    app.open_thread();
    let id = app.thread_view.as_ref().unwrap().generation;
    app.receive(Reply::History {
        id,
        channel: "D0".into(),
        thread: Some("1".into()),
        append: true,
        result: Ok(History {
            messages: vec![message("1", "parent"), message("1.1", "reply")],
            cursor: String::new(),
        }),
    });
    app.close_thread();
    app.open_channel(1);
    app.open_channel(0);
    assert_eq!(app.messages.len(), 2);
    assert_eq!(app.selected, 0);
    app.open_thread();
    assert_eq!(app.thread_view.as_ref().unwrap().messages[1].text, "reply");
}

#[test]
fn stale_disk_responses_never_replace_live_history_or_another_conversation() {
    let (mut app, _) = online_app();
    app.open_channel(0);
    let id = app.generation;
    app.receive(Reply::History {
        id,
        channel: "D0".into(),
        thread: None,
        append: false,
        result: Ok(History {
            messages: vec![],
            cursor: String::new(),
        }),
    });
    let stale = || Feed {
        messages: vec![message("1", "stale")],
        ..Default::default()
    };
    app.receive(Reply::Cache(Event::Feed {
        id,
        channel: "D0".into(),
        thread: None,
        feed: stale(),
    }));
    assert!(app.messages.is_empty());
    app.open_channel(1);
    app.receive(Reply::Cache(Event::Feed {
        id,
        channel: "D0".into(),
        thread: None,
        feed: stale(),
    }));
    assert!(app.messages.is_empty());
}

#[test]
fn incremental_thread_refresh_replaces_edited_replies() {
    let (mut app, _) = online_app();
    app.messages = vec![message("1", "parent")];
    app.open_thread();
    let view = app.thread_view.as_mut().unwrap();
    view.messages.push(message("2", "before"));
    let id = view.generation;
    app.receive(Reply::History {
        id,
        channel: "D0".into(),
        thread: Some("1".into()),
        append: true,
        result: Ok(History {
            messages: vec![message("2", "edited")],
            cursor: String::new(),
        }),
    });
    assert_eq!(app.thread_view.as_ref().unwrap().messages[1].text, "edited");
}

#[test]
fn credential_namespaces_are_opaque_and_isolate_credentials_and_endpoints() {
    let a = Slack::new("credential-a".into()).unwrap().cache_namespace();
    let b = Slack::new("credential-b".into()).unwrap().cache_namespace();
    let c = Slack::with_base_url("credential-a".into(), "https://other.slack.com/api/".into())
        .unwrap()
        .cache_namespace();
    assert_eq!(a.len(), 64);
    assert!(!a.contains("credential"));
    assert_ne!(a, b);
    assert_ne!(a, c);
    assert_eq!(
        a,
        Slack::new("credential-a".into()).unwrap().cache_namespace()
    );
}

#[test]
fn sent_messages_are_persisted_even_after_switching_away() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.sqlite3");
    let (mut app, _) = online_app();
    app.enable_cache(path.clone(), "a".into());
    app.receive(Reply::Bootstrap(Ok(snapshot(&app))));
    app.open_channel(1);
    app.receive(Reply::Sent {
        channel: "D0".into(),
        thread: None,
        text: "sent".into(),
        result: Ok(message("4", "sent")),
    });
    app.save_cache();
    drop(app);
    let db = Cache::open(&path, "a").unwrap();
    assert_eq!(
        db.load_feed("D0", None).unwrap().unwrap().messages[0].text,
        "sent"
    );
}

#[test]
fn thread_rehydrates_from_disk_without_replacing_a_reply_with_its_parent() {
    let (mut app, _) = online_app();
    app.messages = vec![message("1", "parent"), message("3", "other parent")];
    app.selected = 0;
    app.open_thread();
    let id = app.thread_view.as_ref().unwrap().generation;
    app.selected = 1; // Channel selection can move independently of the open thread.
    app.receive(Reply::Cache(Event::Feed {
        id,
        channel: "D0".into(),
        thread: Some("1".into()),
        feed: Feed {
            messages: vec![message("2", "cached reply")],
            selected_ts: Some("2".into()),
            ..Default::default()
        },
    }));
    let view = app.thread_view.as_ref().unwrap();
    assert_eq!(view.messages[0].text, "parent");
    assert_eq!(view.messages[1].text, "cached reply");
    assert_eq!(view.selected, 1);
}

#[test]
fn unread_notifications_are_not_generated_by_cache_restore() {
    let (mut app, _) = online_app();
    app.messages = vec![message("1", "known")];
    app.receive(Reply::History {
        id: app.generation,
        channel: "D0".into(),
        thread: None,
        append: false,
        result: Ok(History {
            messages: vec![message("1", "known")],
            cursor: String::new(),
        }),
    });
    let counts = app.unread_counts.clone();
    app.open_channel(1);
    app.open_channel(0);
    assert_eq!(app.unread_counts, counts);
}

#[test]
fn retry_after_offline_bootstrap_reconnects_and_refreshes_the_restored_channel() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let (mut app, requests) = online_app();
    app.messages = vec![message("1", "cached")];
    app.receive(Reply::Bootstrap(Err(anyhow::anyhow!("offline"))));
    app.key(KeyEvent::new(KeyCode::Char('R'), KeyModifiers::NONE));
    assert!(matches!(requests.try_recv().unwrap(), Job::Bootstrap));
    app.receive(Reply::Bootstrap(Ok(snapshot(&app))));
    assert!(matches!(requests.try_recv().unwrap(), Job::History { .. }));
    assert_eq!(app.messages[0].text, "cached");
}

#[test]
fn cache_failure_does_not_stop_live_history() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("corrupt.sqlite3");
    std::fs::write(&path, "not a database").unwrap();
    let (mut app, _) = online_app();
    app.enable_cache(path, "a".into());
    wait(&mut app, |a| a.status.contains("cache unavailable"));
    app.receive(Reply::History {
        id: app.generation,
        channel: "D0".into(),
        thread: None,
        append: false,
        result: Ok(History {
            messages: vec![message("1", "live")],
            cursor: String::new(),
        }),
    });
    assert_eq!(app.messages[0].text, "live");
}

#[test]
fn refresh_without_overlap_keeps_the_live_cursor_so_missing_messages_are_reachable() {
    let (mut app, requests) = online_app();
    app.messages = vec![message("10", "cached"), message("20", "cached last")];
    app.cursor = "before-10".into();
    app.receive(Reply::History {
        id: app.generation,
        channel: "D0".into(),
        thread: None,
        append: false,
        result: Ok(History {
            messages: vec![message("40", "fresh")],
            cursor: "before-40".into(),
        }),
    });
    assert_eq!(
        app.cursor, "before-40",
        "must fetch the gap, not skip to before the old cache"
    );
    app.refresh_channel(true);
    let Job::History { id, cursor, .. } = requests.try_recv().unwrap() else {
        panic!("expected history")
    };
    assert_eq!(cursor, "before-40");
    app.receive(Reply::History {
        id,
        channel: "D0".into(),
        thread: None,
        append: true,
        result: Ok(History {
            messages: vec![message("20", "overlap"), message("30", "gap filled")],
            cursor: "before-20".into(),
        }),
    });
    assert_eq!(
        app.messages
            .iter()
            .map(|m| m.ts.as_str())
            .collect::<Vec<_>>(),
        ["10", "20", "30", "40"]
    );
}

#[test]
fn truncated_cached_history_rebuilds_pagination_on_the_first_live_refresh() {
    let (mut app, requests) = online_app();
    app.messages = (0..501)
        .map(|i| message(&format!("{i:04}"), "cached"))
        .collect();
    app.selected = 500;
    app.cursor = "before-0000".into();
    app.open_channel(1);
    app.open_channel(0);
    assert_eq!(app.messages.len(), 500);
    assert!(app.cursor.is_empty());
    app.receive(Reply::History {
        id: app.generation,
        channel: "D0".into(),
        thread: None,
        append: false,
        result: Ok(History {
            messages: vec![message("0500", "latest")],
            cursor: "before-0500".into(),
        }),
    });
    assert_eq!(
        app.cursor, "before-0500",
        "an empty cache cursor must not disable PageUp"
    );
    // The refreshed cursor can safely walk through already cached rows.
    while requests.try_recv().is_ok() {}
    app.refresh_channel(true);
    let Job::History { cursor, .. } = requests.try_recv().unwrap() else {
        panic!("expected history")
    };
    assert_eq!(cursor, "before-0500");
}

#[test]
fn disappearing_active_channel_never_saves_its_messages_under_a_different_id() {
    let (mut app, _) = online_app();
    app.messages = vec![message("1", "only belongs to D0")];
    app.selected = 0;
    let mut updated = snapshot(&app);
    updated.conversations.remove(0);
    app.receive(Reply::Bootstrap(Ok(updated)));
    assert_eq!(app.current_id(), Some("D1"));
    assert!(
        app.messages.is_empty(),
        "D0 history must not appear under D1"
    );
    app.open_channel(1);
    app.open_channel(0);
    assert!(!app.messages.iter().any(|m| m.text == "only belongs to D0"));
}

#[test]
fn a_locally_sent_message_must_not_hide_a_gap_in_server_history() {
    let (mut app, _) = online_app();
    app.messages = vec![message("10", "cached"), message("20", "cached last")];
    app.cursor = "before-10".into();
    app.receive(Reply::Sent {
        channel: "D0".into(),
        thread: None,
        text: "sent".into(),
        result: Ok(message("40", "sent")),
    });
    app.receive(Reply::History {
        id: app.generation,
        channel: "D0".into(),
        thread: None,
        append: false,
        result: Ok(History {
            messages: vec![message("35", "remote"), message("40", "sent")],
            cursor: "before-35".into(),
        }),
    });
    assert_eq!(
        app.cursor, "before-35",
        "locally appended overlap must not prove server history is contiguous"
    );
}

#[test]
fn disappearing_channel_does_not_overwrite_the_fallback_channels_draft() {
    let (mut app, _) = online_app();
    app.open_channel(1);
    app.editor.insert("draft for D1");
    app.open_channel(0);
    app.editor.insert("draft for D0");
    let mut updated = snapshot(&app);
    updated.conversations.remove(0);
    app.receive(Reply::Bootstrap(Ok(updated)));
    assert_eq!(app.current_id(), Some("D1"));
    assert_eq!(app.editor.text, "draft for D1");
    assert_eq!(app.drafts[&("D0".into(), None)].text, "draft for D0");
}
