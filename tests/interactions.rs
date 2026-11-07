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
    assert!(app.editor.text.is_empty());
    app.editor.insert("thread reply");
    app.close_thread();
    assert_eq!(app.editor.text, "channel draft");
    app.open_thread();
    assert_eq!(app.editor.text, "thread reply");
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
    let before = app.messages.len();
    app.receive(Reply::History {
        id: app.generation,
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
    assert_eq!(app.messages.len(), before + 1);
    assert!(app.messages.iter().any(|m| m.text == "new reply"));
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
        id: app.generation,
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
    assert!(app.messages.iter().any(|m| m.text == "sent reply"));
    assert_eq!(app.thread_latest.as_deref(), Some("1731000100.000001"));
}

#[test]
fn grouped_sidebar_keeps_direct_messages_clickable_and_scrolls_to_selection() {
    let mut app = App::demo(false);
    let dm_index = app.conversations.len() - 1;
    app.sidebar = dm_index;
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
    assert!(screen.contains("Direct messages"));
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
