use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend, style::Modifier};
use serde_json::json;
use slacker::{
    api::{Message, decode_messages},
    app::{App, Focus, Mode, ThreadView},
    ui::{self, Action, UiState},
};

#[test]
fn every_wrapped_link_segment_opens_the_original_url_without_disturbing_drafts() {
    let url = format!(
        "https://example-workspace.slack.com/archives/C123EXAMPLE/p1700000000000001?{}",
        "long=query&".repeat(10)
    );
    let message = Message {
        text: format!("Read <{url}|this incident update> please"),
        ..Default::default()
    };
    for thread in [false, true] {
        for reader in [false, true] {
            let mut app = App::demo(false);
            app.messages = vec![message.clone()];
            app.selected = 0;
            app.editor.insert("keep my channel draft");
            if thread {
                app.thread = Some("root".into());
                app.thread_view = Some(ThreadView {
                    messages: vec![message.clone()],
                    ..Default::default()
                });
                app.thread_view
                    .as_mut()
                    .unwrap()
                    .editor
                    .insert("keep my thread draft");
                app.focus = Focus::Thread;
            } else {
                app.focus = Focus::Messages;
            }
            app.mode = if reader { Mode::Read } else { Mode::Compose };
            let mut ui = UiState::default();
            let mut terminal =
                Terminal::new(TestBackend::new(if thread { 180 } else { 95 }, 40)).unwrap();
            terminal.draw(|f| ui::draw(f, &app, &mut ui)).unwrap();
            let hits: Vec<_> = ui
                .hits
                .iter()
                .filter_map(|hit| {
                    if let Action::Link(index) = hit.action {
                        (ui.link_targets[index] == url).then_some(hit.rect)
                    } else {
                        None
                    }
                })
                .collect();
            assert!(
                hits.len() >= 2,
                "long link must have a target on every wrapped line"
            );
            let focus = app.focus;
            let mode = app.mode;
            for hit in hits {
                assert!(
                    terminal.backend().buffer()[(hit.x, hit.y)]
                        .modifier
                        .contains(Modifier::UNDERLINED)
                );
                ui::mouse(
                    &mut app,
                    &mut ui,
                    MouseEvent {
                        kind: MouseEventKind::Down(MouseButton::Left),
                        column: hit.x,
                        row: hit.y,
                        modifiers: KeyModifiers::NONE,
                    },
                );
                assert_eq!(ui.pending_url.take().as_deref(), Some(url.as_str()));
                assert!(app.focus == focus && app.mode == mode);
                assert_eq!(app.editor.text, "keep my channel draft");
                if thread {
                    assert_eq!(
                        app.thread_view.as_ref().unwrap().editor.text,
                        "keep my thread draft"
                    );
                }
            }
        }
    }
}

#[test]
fn bare_urls_are_clickable_but_unsafe_schemes_are_not() {
    let mut app = App::demo(false);
    app.messages = vec![Message { text: "See https://example.com/path?q=one&amp;b=two. <javascript:alert(1)|bad> <file:///etc/passwd|bad>".into(), ..Default::default() }];
    app.selected = 0;
    let mut ui = UiState::default();
    let mut terminal = Terminal::new(TestBackend::new(140, 35)).unwrap();
    terminal.draw(|f| ui::draw(f, &app, &mut ui)).unwrap();
    assert!(
        ui.link_targets
            .iter()
            .any(|u| u == "https://example.com/path?q=one&b=two")
    );
    assert!(
        ui.link_targets
            .iter()
            .all(|u| !u.starts_with("javascript:") && !u.starts_with("file:"))
    );
    assert!(slacker::links::valid_url("https://example.com/\x1b]52").is_none());
}

#[test]
fn slack_github_unfurl_renders_a_card_with_clickable_title_website_and_stars() {
    let mut app = App::demo(false);
    app.messages = decode_messages(json!({"ok":true,"messages":[{
        "ts":"1700000000.000001", "text":"hey! <https://github.com/The-PR-Agent/pr-agent>",
        "attachments":[{
            "service_name":"GitHub", "title":"The-PR-Agent/pr-agent",
            "title_link":"https://github.com/The-PR-Agent/pr-agent",
            "text":"PR Agent: The Original Open-Source PR Reviewer.",
            "fields":[
                {"title":"Website","value":"<https://www.pr-agent.ai>","short":true},
                {"title":"Stars","value":"15379","short":true}
            ]
        }]
    }]}))
    .unwrap();
    app.selected = 0;
    assert_eq!(app.messages[0].attachments.len(), 1);
    for read in [false, true] {
        app.mode = if read { Mode::Read } else { Mode::Normal };
        let mut ui = UiState::default();
        let mut terminal = Terminal::new(TestBackend::new(150, 35)).unwrap();
        terminal.draw(|f| ui::draw(f, &app, &mut ui)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        for expected in [
            "The-PR-Agent/pr-agent",
            "Original Open-Source",
            "Website",
            "Stars",
            "15379",
            "│",
        ] {
            assert!(text.contains(expected), "missing {expected}: {text}");
        }
        assert!(
            ui.link_targets
                .iter()
                .any(|u| u == "https://www.pr-agent.ai")
        );
        assert!(
            ui.link_targets
                .iter()
                .filter(|u| *u == "https://github.com/The-PR-Agent/pr-agent")
                .count()
                >= 2,
            "body and preview title must both be clickable"
        );
    }
    let dir = tempfile::tempdir().unwrap();
    let mut cache = slacker::cache::Cache::open(&dir.path().join("history.sqlite3"), "a").unwrap();
    cache
        .save_feed(
            "C1",
            None,
            &slacker::cache::Feed {
                messages: app.messages.clone(),
                ..Default::default()
            },
        )
        .unwrap();
    let restored = cache.load_feed("C1", None).unwrap().unwrap();
    assert_eq!(restored.messages[0].attachments[0].fields[1].value, "15379");
}

#[test]
fn unicode_links_and_preview_cards_keep_hits_inside_the_visible_viewport() {
    let mut app = App::demo(false);
    app.messages = decode_messages(json!({"ok":true,"messages":[{
        "ts":"1700000000.000001", "text": "世界 🦀 <https://example.com/a-long-path/with-more-parts|参考资料>\nhttps://example.org/one/two/three",
        "attachments":[{"title":"A long preview title", "title_link":"https://example.com", "text":"description ".repeat(40), "fields":[{"title":"Website", "value":"https://example.org"}]}]
    }]})).unwrap();
    app.selected = 0;
    let mut ui = UiState::default();
    for (width, height) in [(75, 22), (110, 35), (55, 18)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        for read in [false, true] {
            app.mode = if read { Mode::Read } else { Mode::Normal };
            app.read_scroll = if read { 2 } else { 0 };
            terminal.draw(|f| ui::draw(f, &app, &mut ui)).unwrap();
            let mut hits = 0;
            for hit in &ui.hits {
                if let Action::Link(index) = hit.action {
                    hits += 1;
                    assert!(hit.rect.right() <= width && hit.rect.bottom() <= height);
                    assert!(slacker::links::valid_url(&ui.link_targets[index]).is_some());
                    for x in hit.rect.x..hit.rect.right() {
                        // Ratatui leaves continuation cells of a wide glyph blank;
                        // its visible leading cell carries the glyph's underline.
                        if x > 0
                            && unicode_width::UnicodeWidthStr::width(
                                terminal.backend().buffer()[(x - 1, hit.rect.y)].symbol(),
                            ) > 1
                        {
                            continue;
                        }
                        assert!(
                            terminal.backend().buffer()[(x, hit.rect.y)]
                                .modifier
                                .contains(Modifier::UNDERLINED),
                            "click target must match visible underlined cells at {x},{} (width={width},height={height},read={read},url={},rect={:?},row={})",
                            hit.rect.y,
                            ui.link_targets[index],
                            hit.rect,
                            (0..width)
                                .map(|x| terminal.backend().buffer()[(x, hit.rect.y)].symbol())
                                .collect::<String>()
                        );
                    }
                }
            }
            assert!(hits > 0);
        }
    }
}

#[test]
fn nullable_slack_preview_metadata_does_not_break_message_history() {
    let messages = decode_messages(json!({"ok":true,"messages":[{
        "ts":"1", "text":"message", "attachments":[{"title":null,"title_link":null,"text":null,"fields":null,"service_name":null}]
    }, {"ts":"2","text":"another","attachments":null}]})).unwrap();
    assert_eq!(messages.len(), 2);
    assert!(messages[1].attachments.is_empty());
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn browser_opener_passes_a_single_literal_url_to_the_system_handler() {
    const URL: &str = "https://example.com/?q=one&other=$(whoami)";
    if let Some(capture) = std::env::var_os("SLACKER_OPEN_CAPTURE") {
        slacker::links::open(URL).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let expected = if cfg!(target_os = "macos") {
            format!("--\n{URL}\n")
        } else {
            format!("{URL}\n")
        };
        loop {
            if std::fs::read_to_string(&capture).ok().as_deref() == Some(expected.as_str()) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "system opener did not receive literal URL"
            );
            std::thread::yield_now();
        }
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let opener = dir.path().join(if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    });
    std::fs::write(
        &opener,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$SLACKER_OPEN_CAPTURE\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&opener, std::fs::Permissions::from_mode(0o700)).unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "browser_opener_passes_a_single_literal_url_to_the_system_handler",
            "--nocapture",
        ])
        .env("PATH", dir.path())
        .env("SLACKER_OPEN_CAPTURE", dir.path().join("captured"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
