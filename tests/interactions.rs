use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend};
use slacker::{
    api::{History, Message},
    app::{App, Focus, Mode, Reply},
    ui::{self, Action, UiState},
};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn conversation_changes_keep_separate_drafts() {
    let mut app = App::demo(false);
    app.editor.insert("general draft");
    app.open_channel(1);
    assert!(app.editor.text.is_empty());
    app.editor.insert("engineering draft");
    app.open_channel(0);
    assert_eq!(app.editor.text, "general draft");
    app.open_channel(1);
    assert_eq!(app.editor.text, "engineering draft");
}

#[test]
fn composing_does_not_trigger_navigation_or_quit_shortcuts() {
    let mut app = App::demo(false);
    app.key(key(KeyCode::Char('i')));
    for c in "qjktv?".chars() {
        app.key(key(KeyCode::Char(c)));
    }
    assert_eq!(app.editor.text, "qjktv?");
    assert!(!app.quit);
    assert!(app.thread.is_none());
    app.key(key(KeyCode::Esc));
    app.key(key(KeyCode::Char('q')));
    assert!(app.quit);
}

#[test]
fn stale_history_cannot_replace_another_channel() {
    let mut app = App::demo(false);
    app.generation = 4;
    app.receive(Reply::History {
        id: 3,
        channel: app.current_id().unwrap().into(),
        thread: None,
        append: false,
        result: Ok(History {
            messages: vec![Message {
                ts: "wrong".into(),
                ..Default::default()
            }],
            cursor: String::new(),
        }),
    });
    assert!(!app.messages.iter().any(|m| m.ts == "wrong"));
}

#[test]
fn mouse_selects_conversation_and_composer_from_rendered_hit_targets() {
    let mut app = App::demo(false);
    let mut ui = UiState::default();
    let mut terminal = Terminal::new(TestBackend::new(110, 35)).unwrap();
    terminal.draw(|f| ui::draw(f, &app, &mut ui)).unwrap();
    let rect = ui
        .hits
        .iter()
        .find(|h| h.action == Action::Channel(1))
        .expect("engineering is clickable")
        .rect;
    ui::mouse(
        &mut app,
        &mut ui,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rect.x,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        },
    );
    assert_eq!(app.channel, 1);
    terminal.draw(|f| ui::draw(f, &app, &mut ui)).unwrap();
    let rect = ui
        .hits
        .iter()
        .find(|h| h.action == Action::Compose)
        .unwrap()
        .rect;
    ui::mouse(
        &mut app,
        &mut ui,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rect.x,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        },
    );
    assert!(app.mode == Mode::Compose);
}

#[test]
fn wheel_scrolls_messages_under_pointer() {
    let mut app = App::demo(false);
    let mut ui = UiState::default();
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|f| ui::draw(f, &app, &mut ui)).unwrap();
    let before = app.selected;
    let rect = ui.message_area;
    ui::mouse(
        &mut app,
        &mut ui,
        MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: rect.x + 2,
            row: rect.y + 2,
            modifiers: KeyModifiers::NONE,
        },
    );
    assert!(app.focus == Focus::Messages);
    assert!(app.selected < before);
}

#[test]
fn thread_reply_and_channel_drafts_are_isolated() {
    let mut app = App::demo(false);
    app.editor.insert("channel draft");
    app.open_thread();
    assert_eq!(app.editor.text, "channel draft");
    assert!(app.active_editor().text.is_empty());
    app.active_editor_mut().insert("thread reply");
    app.close_thread();
    assert_eq!(app.editor.text, "channel draft");
    app.open_thread();
    assert_eq!(app.active_editor().text, "thread reply");
}

#[test]
fn switching_channels_invalidates_inflight_history_even_when_loading() {
    let mut app = App::demo(false);
    app.demo = false;
    app.loading = true;
    let previous = app.generation;
    app.open_channel(1);
    app.receive(Reply::History {
        id: previous,
        channel: "D0".into(),
        thread: None,
        append: false,
        result: Ok(History {
            messages: vec![Message {
                ts: "old-channel".into(),
                ..Default::default()
            }],
            cursor: String::new(),
        }),
    });
    assert!(app.messages.is_empty());
}

#[test]
fn tiny_and_narrow_terminals_and_popups_render_without_panicking() {
    let mut app = App::demo(false);
    for (width, height) in [(1, 1), (38, 12), (55, 18), (120, 40)] {
        let mut ui = UiState::default();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        for mode in [Mode::Normal, Mode::Compose, Mode::Help, Mode::Switcher] {
            app.mode = mode;
            terminal.draw(|f| ui::draw(f, &app, &mut ui)).unwrap();
        }
    }
}

#[test]
fn long_message_reader_scrolls_and_closes_without_changing_selection() {
    let mut app = App::demo(false);
    let selected = app.selected;
    app.messages[selected].text = (0..100).map(|i| format!("Line {i}\n")).collect();
    app.key(key(KeyCode::Char('o')));
    // Reader scroll is independent of conversation selection.
    app.key(key(KeyCode::PageDown));
    assert_eq!(app.selected, selected);
    assert!(app.mode != Mode::Normal);
    app.key(key(KeyCode::Esc));
    assert!(app.mode == Mode::Normal);
}

#[test]
fn thread_refresh_merges_replies_instead_of_discarding_loaded_history() {
    let mut app = App::demo(false);
    app.open_thread();
    app.demo = false;
    let before = app.thread_view.as_ref().unwrap().messages.len();
    app.receive(Reply::History {
        id: app.thread_view.as_ref().unwrap().generation,
        channel: app.current_id().unwrap().into(),
        thread: app.thread.clone(),
        append: false,
        result: Ok(History {
            messages: vec![Message {
                ts: "1731000100.000001".into(),
                text: "new reply".into(),
                ..Default::default()
            }],
            cursor: String::new(),
        }),
    });
    assert_eq!(app.thread_view.as_ref().unwrap().messages.len(), before + 1);
    assert!(
        app.thread_view
            .as_ref()
            .unwrap()
            .messages
            .iter()
            .any(|m| m.text == "new reply")
    );
}

#[test]
fn full_message_reader_exposes_later_media_attachments_after_long_text() {
    use slacker::api::Attachment;
    let mut app = App::demo(false);
    app.messages[app.selected].text = "long text\n".repeat(50);
    app.messages[app.selected].files = vec![
        Attachment {
            name: "one.png".into(),
            mimetype: "image/png".into(),
            ..Default::default()
        },
        Attachment {
            name: "two.png".into(),
            mimetype: "image/png".into(),
            ..Default::default()
        },
    ];
    app.key(key(KeyCode::Char('o')));
    for _ in 0..10 {
        app.key(key(KeyCode::PageDown));
    }
    let mut ui = UiState::default();
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|f| ui::draw(f, &app, &mut ui)).unwrap();
    assert!(
        ui.hits
            .iter()
            .any(|hit| hit.action == Action::Attachment(app.selected, 1))
    );
}

#[test]
fn sent_reply_stays_visible_while_intermediate_thread_pages_are_loading() {
    let mut app = App::demo(false);
    app.open_thread();
    app.demo = false;
    let channel = app.current_id().unwrap().to_owned();
    app.receive(Reply::Sent {
        channel,
        thread: app.thread.clone(),
        text: "sent reply".into(),
        result: Ok(Message {
            ts: "1731000900.000001".into(),
            text: "sent reply".into(),
            ..Default::default()
        }),
    });
    app.receive(Reply::History {
        id: app.thread_view.as_ref().unwrap().generation,
        channel: app.current_id().unwrap().into(),
        thread: app.thread.clone(),
        append: true,
        result: Ok(History {
            messages: vec![Message {
                ts: "1731000100.000001".into(),
                text: "intermediate reply".into(),
                ..Default::default()
            }],
            cursor: "more".into(),
        }),
    });
    assert!(
        app.thread_view
            .as_ref()
            .unwrap()
            .messages
            .iter()
            .any(|m| m.text == "sent reply")
    );
    assert_eq!(
        app.thread_view.as_ref().unwrap().latest.as_deref(),
        Some("1731000100.000001")
    );
}

#[test]
fn grouped_sidebar_keeps_direct_messages_clickable_and_scrolls_to_selection() {
    let mut app = App::demo(false);
    let dm_index = app.conversations.len() - 1;
    app.person = dm_index;
    let mut ui = UiState::default();
    let mut terminal = Terminal::new(TestBackend::new(100, 22)).unwrap();
    terminal.draw(|f| ui::draw(f, &app, &mut ui)).unwrap();
    let screen = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(screen.contains("People"));
    assert!(screen.contains("Channels"));
    let rect = ui
        .hits
        .iter()
        .find(|h| h.action == Action::Channel(dm_index))
        .unwrap()
        .rect;
    ui::mouse(
        &mut app,
        &mut ui,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rect.x,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        },
    );
    assert_eq!(app.channel, dm_index);
}

#[test]
fn refresh_keeps_the_message_open_in_the_full_reader() {
    let mut app = App::demo(false);
    app.demo = false;
    app.selected = 0;
    let reading = app.messages[0].ts.clone();
    app.key(key(KeyCode::Char('o')));
    app.read_scroll = 7;
    app.receive(Reply::History {
        id: app.generation,
        channel: app.current_id().unwrap().into(),
        thread: None,
        append: false,
        result: Ok(History {
            messages: vec![Message {
                ts: "1731000999.000001".into(),
                text: "newest page".into(),
                ..Default::default()
            }],
            cursor: "older".into(),
        }),
    });
    assert_eq!(app.messages[app.selected].ts, reading);
    assert_eq!(app.read_scroll, 7);
}

#[test]
fn channel_and_thread_history_route_independently() {
    let mut app = App::demo(false);
    let original = app.messages[app.selected].ts.clone();
    app.open_thread();
    app.demo = false;
    let channel = app.current_id().unwrap().to_owned();
    let thread_generation = app.thread_view.as_ref().unwrap().generation;
    app.receive(Reply::History {
        id: app.generation,
        channel: channel.clone(),
        thread: None,
        append: false,
        result: Ok(History {
            messages: vec![Message {
                ts: "2000000000.000001".into(),
                text: "channel update".into(),
                ..Default::default()
            }],
            cursor: String::new(),
        }),
    });
    assert_eq!(app.messages[0].text, "channel update");
    assert_eq!(app.thread_view.as_ref().unwrap().messages[0].ts, original);
    app.receive(Reply::History {
        id: thread_generation,
        channel,
        thread: app.thread.clone(),
        append: true,
        result: Ok(History {
            messages: vec![Message {
                ts: "2000000001.000001".into(),
                text: "thread update".into(),
                ..Default::default()
            }],
            cursor: String::new(),
        }),
    });
    assert_eq!(app.messages.len(), 1);
    assert!(
        app.thread_view
            .as_ref()
            .unwrap()
            .messages
            .iter()
            .any(|m| m.text == "thread update")
    );
}

#[test]
fn delayed_thread_send_does_not_clear_channel_draft_or_steal_focus() {
    let mut app = App::demo(false);
    app.editor.insert("channel draft");
    app.open_thread();
    app.active_editor_mut().insert("reply");
    let channel = app.current_id().unwrap().to_owned();
    let thread = app.thread.clone();
    app.close_thread();
    app.receive(Reply::Sent {
        channel,
        thread,
        text: "reply".into(),
        result: Ok(Message {
            ts: "2000000000.000001".into(),
            text: "reply".into(),
            ..Default::default()
        }),
    });
    assert_eq!(app.editor.text, "channel draft");
    assert!(app.focus == Focus::Messages);
    app.open_thread();
    assert!(app.active_editor().text.is_empty());
}

#[test]
fn reopening_a_thread_rejects_its_previous_history_response() {
    let mut app = App::demo(false);
    app.open_thread();
    let old_generation = app.thread_view.as_ref().unwrap().generation;
    let root = app.thread.clone();
    app.close_thread();
    app.open_thread();
    app.receive(Reply::History {
        id: old_generation,
        channel: app.current_id().unwrap().into(),
        thread: root,
        append: true,
        result: Ok(History {
            messages: vec![Message {
                ts: "2000000010.000001".into(),
                text: "stale thread page".into(),
                ..Default::default()
            }],
            cursor: "stale".into(),
        }),
    });
    assert!(
        !app.thread_view
            .as_ref()
            .unwrap()
            .messages
            .iter()
            .any(|m| m.text == "stale thread page")
    );
    assert!(app.thread_view.as_ref().unwrap().cursor.is_empty());
}

#[test]
fn presence_is_deduplicated_capped_and_throttled_without_network_access() {
    use slacker::{
        api::{Presence, Slack},
        app::{Job, Worker},
    };
    use std::sync::mpsc;
    let (jobs, queued) = mpsc::channel();
    let (events, replies) = mpsc::channel();
    let worker = Worker {
        jobs,
        replies,
        events,
        slack: Slack::new("test-token".into()).unwrap(),
    };
    let mut app = App::demo(false);
    app.demo = false;
    app.worker = Some(worker);
    let users: Vec<String> = (0..12).map(|i| format!("U{i}")).collect();
    let mut duplicated = users.clone();
    duplicated.insert(0, "U0".into());
    app.refresh_presence(&duplicated);
    let Job::Presence { users: requested } = queued.try_recv().unwrap() else {
        panic!("expected presence request")
    };
    assert_eq!(requested.len(), 10);
    app.refresh_presence(&users);
    assert!(queued.try_recv().is_err());
    let status = app.status.clone();
    app.receive(Reply::Presence {
        users: requested.clone(),
        result: Err(anyhow::anyhow!("offline")),
    });
    assert_eq!(app.presences["U0"], Presence::Unknown);
    assert_eq!(app.status, status);
    app.refresh_presence(&requested);
    assert!(queued.try_recv().is_err());
    app.refresh_presence(&users);
    let Job::Presence { users: remaining } = queued.try_recv().unwrap() else {
        panic!("expected uncached users")
    };
    assert_eq!(remaining, vec!["U10", "U11"]);
}

#[test]
fn unread_refresh_retains_previous_counts_on_error_and_is_throttled() {
    use slacker::{
        api::Slack,
        app::{Job, Worker},
    };
    use std::sync::mpsc;
    let (jobs, queued) = mpsc::channel();
    let (events, replies) = mpsc::channel();
    let mut app = App::demo(false);
    app.demo = false;
    app.worker = Some(Worker {
        jobs,
        replies,
        events,
        slack: Slack::new("test-token".into()).unwrap(),
    });
    app.refresh_unreads();
    assert!(matches!(queued.try_recv().unwrap(), Job::Counts));
    app.receive(Reply::Counts(Err(anyhow::anyhow!("offline"))));
    assert_eq!(app.unread_counts["D1"], 2);
    app.refresh_unreads();
    assert!(queued.try_recv().is_err());
}
