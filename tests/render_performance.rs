use ratatui::{Terminal, backend::TestBackend};
use slacker::{
    api::Message,
    app::App,
    ui::{self, UiState},
};
use std::time::{Duration, Instant};

// Run explicitly in debug builds: cargo test --test render_performance -- --ignored --nocapture
// A deliberately generous budget detects multi-second redraws, not small timing variations.
#[test]
#[ignore = "manual performance regression for large cached workspaces"]
fn cached_history_redraw_does_not_scan_the_entire_workspace_for_every_text_fragment() {
    let mut app = App::demo(false);
    for i in 0..4000 {
        app.users
            .insert(format!("U{i:08}"), format!("Colleague {i}"));
    }
    app.messages = (0..500).map(|i| Message {
        ts: format!("170000{i:04}.000001"),
        user: "maya".into(),
        text: format!("<@U00000001> Please review <https://github.com/example/repo/pull/{i}|this change> when you have time."),
        ..Default::default()
    }).collect();
    app.selected = app.messages.len() - 1;
    let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
    let mut ui = UiState::default();
    let start = Instant::now();
    terminal.draw(|f| ui::draw(f, &app, &mut ui)).unwrap();
    let elapsed = start.elapsed();
    eprintln!("500-message frame with 4,000 workspace users: {elapsed:?}");
    app.users.retain(|id, _| id == "maya" || id == "U00000001");
    let small_start = Instant::now();
    terminal.draw(|f| ui::draw(f, &app, &mut ui)).unwrap();
    eprintln!(
        "Same 500-message frame with 2 users: {:?}",
        small_start.elapsed()
    );
    assert!(
        elapsed < Duration::from_millis(200),
        "cached history blocks input while redrawing: {elapsed:?}"
    );
}
