use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend};
use slacker::{
    api::{History, Message},
    app::{App, Focus, Mode, Reply, ThreadView},
    ui::{self, Action, UiState},
};

fn message(index: usize) -> Message {
    Message {
        ts: format!("170000{index:04}.000001"),
        user: "maya".into(),
        text: format!("Message {index}: <https://example.com/{index}|link>"),
        ..Default::default()
    }
}

#[test]
fn short_channel_and_thread_feeds_end_above_the_composer_and_links_move_with_them() {
    for thread in [false, true] {
        let mut app = App::demo(false);
        app.messages = vec![message(0)];
        app.selected = 0;
        if thread {
            app.thread = Some(app.messages[0].ts.clone());
            app.thread_view = Some(ThreadView {
                messages: vec![message(0)],
                ..Default::default()
            });
        }
        let mut ui = UiState::default();
        for (width, height) in [(200, 40), (160, 25)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|f| ui::draw(f, &app, &mut ui)).unwrap();
            let area = if thread {
                ui.thread_area
            } else {
                ui.message_area
            };
            let action = if thread {
                Action::ThreadMessage(0)
            } else {
                Action::Message(0)
            };
            let rect = ui.hits.iter().find(|h| h.action == action).unwrap().rect;
            assert_eq!(
                rect.bottom(),
                area.bottom(),
                "short feeds should end above the composer"
            );
            assert!(
                rect.y > area.y,
                "empty space belongs above the conversation"
            );
            let target = ui
                .hits
                .iter()
                .find(|h| {
                    matches!(h.action, Action::Link(_))
                        && area.contains(ratatui::layout::Position::new(h.rect.x, h.rect.y))
                })
                .unwrap()
                .rect;
            let row: String = (area.x..area.right())
                .map(|x| terminal.backend().buffer()[(x, target.y)].symbol())
                .collect();
            assert!(
                row.contains("Message 0"),
                "link hit must move with the displayed message"
            );
            ui::mouse(
                &mut app,
                &mut ui,
                MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column: target.x,
                    row: target.y,
                    modifiers: KeyModifiers::NONE,
                },
            );
            assert_eq!(
                ui.pending_url.take().as_deref(),
                Some("https://example.com/0")
            );
        }
    }
}

#[test]
fn latest_message_is_bottom_aligned_even_when_loaded_history_exceeds_the_viewport() {
    let mut app = App::demo(false);
    app.messages = (0..100).map(message).collect();
    app.selected = 99;
    let mut ui = UiState::default();
    let mut terminal = Terminal::new(TestBackend::new(120, 35)).unwrap();
    terminal.draw(|f| ui::draw(f, &app, &mut ui)).unwrap();
    let rect = ui
        .hits
        .iter()
        .find(|h| h.action == Action::Message(99))
        .unwrap()
        .rect;
    assert_eq!(rect.bottom(), ui.message_area.bottom());
}

#[test]
fn new_messages_follow_the_tail_only_when_already_latest_and_not_in_the_reader() {
    for thread in [false, true] {
        for (selected, mode, expected) in [
            (1, Mode::Normal, 2),
            (1, Mode::Compose, 2),
            (0, Mode::Normal, 0),
            (1, Mode::Read, 1),
        ] {
            let mut app = App::demo(false);
            app.demo = false;
            app.messages = vec![message(0), message(1)];
            app.selected = selected;
            app.mode = mode;
            app.focus = if thread {
                Focus::Thread
            } else {
                Focus::Messages
            };
            app.read_scroll = 7;
            if thread {
                app.thread = Some(app.messages[0].ts.clone());
                app.thread_view = Some(ThreadView {
                    messages: app.messages.clone(),
                    selected,
                    read_scroll: 7,
                    ..Default::default()
                });
            }
            let root = app.thread.clone();
            app.receive(Reply::History {
                id: 0,
                channel: "D0".into(),
                thread: root,
                append: thread,
                result: Ok(History {
                    messages: vec![message(0), message(1), message(2)],
                    cursor: "older".into(),
                }),
            });
            let (messages, selected) = app.active_messages();
            assert_eq!(messages.len(), 3);
            assert_eq!(
                selected, expected,
                "follow state must respect reading position"
            );
            if mode == Mode::Read {
                assert_eq!(
                    if thread {
                        app.thread_view.as_ref().unwrap().read_scroll
                    } else {
                        app.read_scroll
                    },
                    7
                );
            }
        }
    }
}

#[test]
fn receiving_new_history_does_not_move_the_viewport_when_reading_older_messages() {
    let mut app = App::demo(false);
    app.demo = false;
    app.messages = (0..100).map(message).collect();
    app.selected = 3;
    let mut ui = UiState::default();
    let mut terminal = Terminal::new(TestBackend::new(120, 35)).unwrap();
    terminal.draw(|f| ui::draw(f, &app, &mut ui)).unwrap();
    let before: Vec<_> = ui
        .hits
        .iter()
        .filter_map(|h| {
            if let Action::Message(index) = h.action {
                Some((index, h.rect))
            } else {
                None
            }
        })
        .collect();
    app.receive(Reply::History {
        id: app.generation,
        channel: "D0".into(),
        thread: None,
        append: false,
        result: Ok(History {
            messages: (0..101).map(message).collect(),
            cursor: "older".into(),
        }),
    });
    terminal.draw(|f| ui::draw(f, &app, &mut ui)).unwrap();
    let after: Vec<_> = ui
        .hits
        .iter()
        .filter_map(|h| {
            if let Action::Message(index) = h.action {
                Some((index, h.rect))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(app.selected, 3);
    assert_eq!(before, after);
}
