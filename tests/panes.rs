use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend};
use slacker::{
    app::{App, Mode},
    ui::{self, UiState},
};
fn mouse(kind: MouseEventKind, x: u16, y: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    }
}
#[test]
fn sending_keeps_the_composer_ready_for_the_next_message() {
    let mut app = App::demo(false);
    app.mode = Mode::Compose;
    app.editor.insert("First message");
    app.send();
    assert!(app.mode == Mode::Compose);
    assert!(app.editor.text.is_empty());
}
#[test]
fn opening_replies_preserves_the_channel_feed() {
    let mut app = App::demo(false);
    let before: Vec<_> = app.messages.iter().map(|m| m.ts.clone()).collect();
    app.open_thread();
    assert!(app.thread.is_some());
    assert_eq!(
        app.messages
            .iter()
            .map(|m| m.ts.clone())
            .collect::<Vec<_>>(),
        before
    );
}
#[test]
fn dragging_the_sidebar_divider_changes_its_width() {
    let mut app = App::demo(false);
    let mut ui = UiState::default();
    let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
    terminal.draw(|f| ui::draw(f, &app, &mut ui)).unwrap();
    let before = ui.sidebar_area.width;
    let x = ui.sidebar_area.right() - 1;
    let y = ui.sidebar_area.y + 2;
    ui::mouse(
        &mut app,
        &mut ui,
        mouse(MouseEventKind::Down(MouseButton::Left), x, y),
    );
    ui::mouse(
        &mut app,
        &mut ui,
        mouse(MouseEventKind::Drag(MouseButton::Left), x + 9, y),
    );
    ui::mouse(
        &mut app,
        &mut ui,
        mouse(MouseEventKind::Up(MouseButton::Left), x + 9, y),
    );
    terminal.draw(|f| ui::draw(f, &app, &mut ui)).unwrap();
    assert!(ui.sidebar_area.width > before);
}

#[test]
fn channel_and_thread_composers_are_independent_and_thread_can_be_resized() {
    use slacker::{app::Focus, ui::Action};
    let mut app = App::demo(false);
    app.editor.insert("Channel draft");
    app.open_thread();
    let mut ui = UiState::default();
    let mut terminal = Terminal::new(TestBackend::new(160, 45)).unwrap();
    terminal.draw(|f| ui::draw(f, &app, &mut ui)).unwrap();
    assert!(ui.thread_area.x > ui.message_area.right());
    assert!(ui.hits.iter().any(|hit| hit.action == Action::Compose));
    let rect = ui
        .hits
        .iter()
        .find(|hit| hit.action == Action::ThreadCompose)
        .unwrap()
        .rect;
    ui::mouse(
        &mut app,
        &mut ui,
        mouse(MouseEventKind::Down(MouseButton::Left), rect.x, rect.y),
    );
    assert!(app.focus == Focus::Thread);
    app.active_editor_mut().insert("Thread reply");
    app.send();
    assert!(app.mode == Mode::Compose && app.focus == Focus::Thread);
    assert_eq!(app.editor.text, "Channel draft");
    assert!(
        app.thread_view
            .as_ref()
            .unwrap()
            .messages
            .iter()
            .any(|m| m.text == "Thread reply")
    );
    let before = ui.thread_area.width;
    let divider = ui.thread_divider;
    ui::mouse(
        &mut app,
        &mut ui,
        mouse(
            MouseEventKind::Down(MouseButton::Left),
            divider.x,
            divider.y + 1,
        ),
    );
    ui::mouse(
        &mut app,
        &mut ui,
        mouse(
            MouseEventKind::Drag(MouseButton::Left),
            divider.x - 8,
            divider.y + 1,
        ),
    );
    ui::mouse(
        &mut app,
        &mut ui,
        mouse(
            MouseEventKind::Up(MouseButton::Left),
            divider.x - 8,
            divider.y + 1,
        ),
    );
    terminal.draw(|f| ui::draw(f, &app, &mut ui)).unwrap();
    assert!(ui.thread_area.width > before);
    let rect = ui
        .hits
        .iter()
        .find(|hit| hit.action == Action::CloseThread)
        .unwrap()
        .rect;
    ui::mouse(
        &mut app,
        &mut ui,
        mouse(MouseEventKind::Down(MouseButton::Left), rect.x, rect.y),
    );
    assert!(app.thread_view.is_none());
    assert_eq!(app.editor.text, "Channel draft");
}

#[test]
fn people_stay_visible_even_with_hundreds_of_channels() {
    use slacker::api::Conversation;
    let mut app = App::demo(false);
    let dm = app.conversations.iter().find(|c| c.is_im).unwrap().clone();
    app.conversations = (0..200)
        .map(|i| Conversation {
            id: format!("C{i}"),
            name: format!("channel-{i}"),
            is_member: true,
            ..Default::default()
        })
        .collect();
    app.person = app.conversations.len();
    app.conversations.push(dm);
    let mut ui = UiState::default();
    let mut terminal = Terminal::new(TestBackend::new(110, 35)).unwrap();
    terminal.draw(|f| ui::draw(f, &app, &mut ui)).unwrap();
    assert!(ui.visible_people.iter().any(|id| id == "maya"));
    let screen = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(screen.contains("Maya Chen"));
}

#[test]
fn scrolling_the_channel_cannot_redirect_a_thread_reply() {
    use slacker::app::Focus;
    let mut app = App::demo(false);
    app.editor.insert("Unsent channel draft");
    app.open_thread();
    app.active_editor_mut().insert("Thread-only reply");
    let mut ui = UiState::default();
    let mut terminal = Terminal::new(TestBackend::new(160, 45)).unwrap();
    terminal.draw(|f| ui::draw(f, &app, &mut ui)).unwrap();
    let rect = ui.message_area;
    ui::mouse(
        &mut app,
        &mut ui,
        mouse(MouseEventKind::ScrollUp, rect.x + 2, rect.y + 2),
    );
    assert!(app.focus == Focus::Thread);
    app.send();
    assert_eq!(app.editor.text, "Unsent channel draft");
    assert!(!app.messages.iter().any(|m| m.text == "Thread-only reply"));
    assert!(
        app.thread_view
            .as_ref()
            .unwrap()
            .messages
            .iter()
            .any(|m| m.text == "Thread-only reply")
    );
}

#[test]
fn people_can_scroll_without_taking_focus_from_a_reply_draft() {
    use slacker::{api::Conversation, app::Focus};
    let mut app = App::demo(false);
    app.conversations.retain(|c| !c.is_im);
    for index in 0..20 {
        app.conversations.push(Conversation {
            id: format!("DM{index}"),
            name: format!("Colleague {index}"),
            user: format!("user{index}"),
            is_im: true,
            ..Default::default()
        });
    }
    app.open_thread();
    app.active_editor_mut().insert("Still writing here");
    let mut ui = UiState::default();
    let mut terminal = Terminal::new(TestBackend::new(160, 45)).unwrap();
    terminal.draw(|f| ui::draw(f, &app, &mut ui)).unwrap();
    assert_eq!(ui.visible_people[0], "user0");
    let rect = ui.people_area;
    ui::mouse(
        &mut app,
        &mut ui,
        mouse(MouseEventKind::ScrollDown, rect.x + 2, rect.y + 3),
    );
    terminal.draw(|f| ui::draw(f, &app, &mut ui)).unwrap();
    assert!(app.focus == Focus::Thread);
    assert_eq!(app.active_editor().text, "Still writing here");
    assert_eq!(ui.visible_people[0], "user1");
}

#[test]
fn previewing_channel_media_preserves_thread_typing_destination() {
    use slacker::{app::Focus, ui::Action};
    let mut app = App::demo(true);
    app.open_thread();
    app.active_editor_mut()
        .insert("Private thread continuation");
    let mut ui = UiState::default();
    let mut terminal = Terminal::new(TestBackend::new(160, 45)).unwrap();
    terminal.draw(|f| ui::draw(f, &app, &mut ui)).unwrap();
    let hit = ui
        .hits
        .iter()
        .find(|h| matches!(h.action, Action::Attachment(..)))
        .unwrap();
    let rect = hit.rect;
    ui::mouse(
        &mut app,
        &mut ui,
        mouse(MouseEventKind::Down(MouseButton::Left), rect.x, rect.y),
    );
    assert!(app.focus == Focus::Thread);
    assert_eq!(app.active_editor().text, "Private thread continuation");
}
