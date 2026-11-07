use slacker::{api::Message, notifications::Notifications};
fn message(ts: &str, user: &str) -> Message {
    Message {
        ts: ts.into(),
        user: user.into(),
        ..Default::default()
    }
}
#[test]
fn initial_history_and_pagination_do_not_notify() {
    let mut state = Notifications::default();
    state.focus(false);
    state.observe("C1", &[message("100.000001", "other")], "me", false);
    state.observe("C1", &[message("90.000001", "other")], "me", true);
    assert_eq!(state.unread, 0);
    assert!(!state.take_bell());
}
#[test]
fn new_background_messages_increment_once_and_own_messages_stay_quiet() {
    let mut state = Notifications::default();
    state.observe("C1", &[message("100.000001", "other")], "me", false);
    state.focus(false);
    let messages = [
        message("100.000001", "other"),
        message("101.000001", "other"),
        message("102.000001", "me"),
    ];
    state.observe("C1", &messages, "me", false);
    assert_eq!(state.unread, 1);
    assert!(state.take_bell());
    state.observe("C1", &messages, "me", false);
    assert_eq!(state.unread, 1);
    assert!(!state.take_bell());
    state.focus(true);
    assert_eq!(state.unread, 0);
}
#[test]
fn focused_updates_and_first_visit_to_another_channel_are_quiet() {
    let mut state = Notifications::default();
    state.observe("C1", &[message("100.000001", "other")], "me", false);
    state.observe("C1", &[message("101.000001", "other")], "me", false);
    state.focus(false);
    state.observe("C2", &[message("200.000001", "other")], "me", false);
    assert_eq!(state.unread, 0);
    assert!(!state.take_bell());
}

#[test]
fn accepted_channel_refresh_drives_unread_title_but_thread_pages_do_not() {
    use slacker::{
        api::History,
        app::{App, Reply},
    };
    let mut app = App::demo(false);
    app.demo = false;
    app.receive(Reply::History {
        id: app.generation,
        channel: app.current_id().unwrap().into(),
        thread: None,
        append: false,
        result: Ok(History {
            messages: vec![message("100.000001", "other")],
            cursor: String::new(),
        }),
    });
    app.notifications.focus(false);
    app.receive(Reply::History {
        id: app.generation,
        channel: app.current_id().unwrap().into(),
        thread: None,
        append: false,
        result: Ok(History {
            messages: vec![message("101.000001", "other")],
            cursor: String::new(),
        }),
    });
    assert_eq!(app.notifications.unread, 1);
    assert!(
        app.notifications
            .title("Workspace", "#general")
            .starts_with("(1) slacker")
    );
    app.open_thread();
    app.receive(Reply::History {
        id: app.thread_view.as_ref().unwrap().generation,
        channel: app.current_id().unwrap().into(),
        thread: app.thread.clone(),
        append: true,
        result: Ok(History {
            messages: vec![message("200.000001", "other")],
            cursor: String::new(),
        }),
    });
    assert_eq!(app.notifications.unread, 1);
}
