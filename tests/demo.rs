use ratatui::{Terminal, backend::TestBackend};
use slacker::{
    app::{App, Mode},
    ui::{self, Action, UiState},
};
#[test]
fn demo_has_real_offline_media_and_reactions() {
    let mut app = App::demo(true);
    assert!(
        app.messages
            .iter()
            .any(|m| m.files.iter().any(|f| f.mimetype.starts_with("image/")))
    );
    assert!(app.messages.iter().any(|m| !m.reactions.is_empty()));
    app.open_channel(1);
    assert!(
        app.messages
            .iter()
            .any(|m| m.files.iter().any(|f| f.is_video()))
    );
    app.open_channel(3);
    app.selected = app
        .messages
        .iter()
        .position(|m| m.files.iter().any(|f| f.mimetype.starts_with("image/")))
        .unwrap();
    app.open_media();
    assert!(app.viewer.as_ref().is_some_and(|v| v.last.is_some()));
}
#[test]
fn demo_thread_opens_the_actual_selected_parent_and_has_replies() {
    let mut app = App::demo(false);
    app.selected = app.messages.iter().position(|m| m.reply_count > 0).unwrap();
    let parent = app.messages[app.selected].clone();
    app.open_thread();
    let view = app.thread_view.as_ref().unwrap();
    assert_eq!(view.messages[0].ts, parent.ts);
    assert_eq!(view.messages[0].text, parent.text);
    assert!(view.messages.len() >= 3);
}
#[test]
fn composer_is_a_single_obvious_field_with_send_only_when_needed() {
    let mut app = App::demo(false);
    let mut ui = UiState::default();
    let mut terminal = Terminal::new(TestBackend::new(110, 35)).unwrap();
    terminal.draw(|f| ui::draw(f, &app, &mut ui)).unwrap();
    let screen = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(!screen.contains("i Compose"));
    assert!(!screen.contains("o Read"));
    assert!(screen.contains("Message #general"));
    assert!(!ui.hits.iter().any(|h| h.action == Action::Send));
    app.mode = Mode::Compose;
    app.editor.insert("Hello team");
    terminal.draw(|f| ui::draw(f, &app, &mut ui)).unwrap();
    assert!(ui.hits.iter().any(|h| h.action == Action::Send));
}

#[test]
fn inline_images_reactions_and_reply_links_are_clickable() {
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    let mut app = App::demo(true);
    let mut ui = UiState::default();
    let mut terminal = Terminal::new(TestBackend::new(110, 40)).unwrap();
    terminal.draw(|f| ui::draw(f, &app, &mut ui)).unwrap();
    assert!(!ui.inline_images.is_empty());
    let click = |app: &mut App, ui: &mut UiState, rect: ratatui::layout::Rect| {
        ui::mouse(
            app,
            ui,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: rect.x,
                row: rect.y,
                modifiers: KeyModifiers::NONE,
            },
        )
    };
    let (rect, message, reaction) = ui
        .hits
        .iter()
        .find_map(|hit| match hit.action {
            Action::Reaction(m, r) => Some((hit.rect, m, r)),
            _ => None,
        })
        .unwrap();
    let before = app.messages[message].reactions[reaction].count;
    click(&mut app, &mut ui, rect);
    assert_eq!(app.messages[message].reactions[reaction].count, before + 1);
    click(&mut app, &mut ui, rect);
    assert_eq!(app.messages[message].reactions[reaction].count, before);
    let (rect, index) = ui
        .hits
        .iter()
        .find_map(|hit| match hit.action {
            Action::ThreadAt(i) => Some((hit.rect, i)),
            _ => None,
        })
        .unwrap();
    let parent = app.messages[index].ts.clone();
    click(&mut app, &mut ui, rect);
    assert_eq!(app.thread.as_deref(), Some(parent.as_str()));
    assert_eq!(app.thread_view.as_ref().unwrap().messages.len(), 3);
}

#[test]
fn demo_replies_survive_closing_and_reopening_the_thread() {
    let mut app = App::demo(false);
    app.open_thread();
    app.active_editor_mut().insert("A real local reply");
    app.send();
    app.close_thread();
    assert_eq!(app.messages[app.selected].reply_count, 3);
    app.open_thread();
    assert!(
        app.thread_view
            .as_ref()
            .unwrap()
            .messages
            .iter()
            .any(|m| m.text == "A real local reply")
    );
}

#[test]
fn parent_reactions_stay_in_sync_after_a_thread_has_been_cached() {
    let mut app = App::demo(false);
    app.open_thread();
    app.close_thread();
    app.toggle_demo_reaction(app.selected, 0);
    let count = app.messages[app.selected].reactions[0].count;
    app.open_thread();
    assert_eq!(
        app.thread_view.as_ref().unwrap().messages[0].reactions[0].count,
        count
    );
    app.close_thread();
    assert_eq!(app.messages[app.selected].reactions[0].count, count);
}
