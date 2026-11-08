use ratatui::{Terminal, backend::TestBackend};
use slacker::{
    api::Message,
    app::{App, Focus, Mode, ThreadView},
    ui::{self, UiState},
};

#[test]
fn mention_lookup_handles_adjacent_unknown_and_incomplete_tokens() {
    let mut app = App::demo(false);
    app.users.insert("U123".into(), "李 Ada".into());
    for (input, expected) in [
        ("<@maya><@U123> <@missing>", "@Maya Chen@李 Ada <@missing>"),
        ("before <@unfinished", "before <@unfinished"),
        ("<@broken <@U123>>", "<@broken @李 Ada>"),
        ("&lt;@U123&gt; &amp; <@U123>", "<@U123> & @李 Ada"),
        (
            "ordinary text without mentions",
            "ordinary text without mentions",
        ),
    ] {
        assert_eq!(ui::display_text(&app, input), expected);
    }
}

#[test]
fn slack_links_render_labels_and_keep_destinations_readable() {
    let app = App::demo(false);
    for (input, expected) in [
        (
            "Could you look at <https://github.com/example/project/pull/42|this> one please?",
            "Could you look at this (https://github.com/example/project/pull/42) one please?",
        ),
        ("<https://example.com>", "https://example.com"),
        (
            "<http://example.com|http://example.com>",
            "http://example.com",
        ),
        ("<https://example.com|>", "https://example.com"),
        (
            "<mailto:team@example.com|Email us>",
            "Email us (mailto:team@example.com)",
        ),
        (
            "<https://example.com?a=1&amp;b=2|Docs &amp; help>",
            "Docs & help (https://example.com?a=1&b=2)",
        ),
        (
            "<https://example.com/一|参考> / <https://example.org|More>",
            "参考 (https://example.com/一) / More (https://example.org)",
        ),
    ] {
        assert_eq!(ui::display_text(&app, input), expected, "input: {input}");
    }
}

#[test]
fn link_parsing_preserves_other_text_mentions_and_terminal_safety() {
    let app = App::demo(false);
    for text in [
        "ordinary https://example.com/path",
        "<not a link> <!here> <#C123|general>",
        "an incomplete <https://example.com",
        "<javascript:alert(1)|no>",
        "<https://bad url|no>",
    ] {
        assert_eq!(ui::display_text(&app, text), text);
    }
    assert_eq!(ui::display_text(&app, "<@maya>"), "@Maya Chen");
    assert_eq!(
        ui::display_text(&app, "&lt;https://example.com|literal&gt; &amp;"),
        "<https://example.com|literal> &"
    );
    assert_eq!(
        ui::display_text(&app, "<https://example.com|\x1b]52;c;secret\x07>"),
        "]52;c;secret (https://example.com)"
    );
    assert_eq!(
        ui::display_text(&app, "<broken <https://example.com|Docs>"),
        "<broken Docs (https://example.com)"
    );
}

#[test]
fn links_render_in_conversation_thread_and_full_message_views() {
    let mut app = App::demo(false);
    let message = Message {
        ts: "1704067200.000001".into(),
        user: "maya".into(),
        text: "Read <https://example.com/pr/42|this PR> please".into(),
        ..Default::default()
    };
    app.messages = vec![message.clone()];
    app.selected = 0;
    app.thread_view = Some(ThreadView {
        messages: vec![message],
        ..Default::default()
    });
    for focus in [Focus::Messages, Focus::Thread] {
        app.focus = focus;
        for mode in [Mode::Normal, Mode::Read] {
            app.mode = mode;
            let mut terminal = Terminal::new(TestBackend::new(220, 40)).unwrap();
            let mut ui = UiState::default();
            terminal.draw(|f| ui::draw(f, &app, &mut ui)).unwrap();
            let buffer = terminal.backend().buffer();
            let area = if mode == Mode::Read {
                buffer.area
            } else if focus == Focus::Thread {
                ui.thread_area
            } else {
                ui.message_area
            };
            let rows: Vec<String> = (area.y..area.bottom())
                .map(|y| {
                    (area.x..area.right())
                        .map(|x| buffer[(x, y)].symbol())
                        .collect()
                })
                .collect();
            let text = rows.join("\n");
            assert!(
                text.contains("this PR (https://example.com/pr/42)"),
                "{text}"
            );
            assert!(!text.contains("|this PR>"), "{text}");
        }
    }
}
