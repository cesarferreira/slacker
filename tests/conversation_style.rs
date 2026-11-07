use ratatui::{Terminal, backend::TestBackend, style::Modifier};
use slacker::{
    api::Message,
    app::App,
    ui::{self, UiState},
};

fn conversation(ts: &str) -> App {
    let mut app = App::demo(false);
    app.messages = vec![Message {
        ts: ts.into(),
        user: "maya".into(),
        text: "A message with room to breathe".into(),
        reply_count: 2,
        ..Default::default()
    }];
    app.selected = 0;
    app
}

#[test]
fn conversation_has_avatar_gutter_aligned_body_and_italic_time() {
    let app = conversation("1704067200.000001");
    let mut terminal = Terminal::new(TestBackend::new(120, 35)).unwrap();
    let mut ui = UiState::default();
    terminal.draw(|f| ui::draw(f, &app, &mut ui)).unwrap();
    let buffer = terminal.backend().buffer();
    let mut author = None;
    let mut body = None;
    for y in ui.message_area.y..ui.message_area.bottom() {
        let line: String = (ui.message_area.x..ui.message_area.right())
            .map(|x| buffer[(x, y)].symbol())
            .collect();
        if let Some(x) = line.find("Maya Chen") {
            author = Some((x, y));
        }
        if let Some(x) = line.find("A message") {
            body = Some(x);
        }
    }
    let (x, y) = author.unwrap();
    assert!(x >= 7, "author needs space for the profile picture");
    assert_eq!(body, Some(x), "body should align with author");
    let gutter: String = (ui.message_area.x..ui.message_area.x + x as u16)
        .map(|x| buffer[(x, y)].symbol())
        .collect();
    assert!(
        gutter.contains("MC"),
        "missing-photo fallback uses initials"
    );
    assert!(
        (ui.message_area.x..ui.message_area.right())
            .any(|x| buffer[(x, y)].modifier.contains(Modifier::ITALIC))
    );
}

#[test]
fn timestamps_use_system_timezone_instead_of_utc() {
    // Use a subprocess: changing TZ in a multithreaded test process is unsafe.
    if std::env::var_os("SLACKER_TIME_TEST").is_none() {
        for (zone, ts, time, date) in [
            (
                "Asia/Kolkata",
                "1704067200",
                "5:30 AM",
                "Monday, January 1, 2024",
            ),
            (
                "America/New_York",
                "1704067200",
                "7:00 PM",
                "Sunday, December 31, 2023",
            ),
            (
                "America/New_York",
                "1719792000",
                "8:00 PM",
                "Sunday, June 30, 2024",
            ),
        ] {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "timestamps_use_system_timezone_instead_of_utc",
                    "--nocapture",
                ])
                .env("SLACKER_TIME_TEST", "1")
                .env("TZ", zone)
                .env("SLACKER_TEST_TS", ts)
                .env("SLACKER_TEST_TIME", time)
                .env("SLACKER_TEST_DATE", date)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{zone}: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        return;
    }
    let app = conversation(&std::env::var("SLACKER_TEST_TS").unwrap());
    let mut terminal = Terminal::new(TestBackend::new(120, 35)).unwrap();
    terminal
        .draw(|f| ui::draw(f, &app, &mut UiState::default()))
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(
        text.contains(&std::env::var("SLACKER_TEST_TIME").unwrap()),
        "expected local time, got {text}"
    );
    assert!(
        text.contains(&std::env::var("SLACKER_TEST_DATE").unwrap()),
        "date should use local timezone: {text}"
    );
    assert!(!text.contains("UTC"));
}

#[test]
fn current_day_has_a_centered_today_separator() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let app = conversation(&now.to_string());
    let mut terminal = Terminal::new(TestBackend::new(120, 35)).unwrap();
    let mut ui = UiState::default();
    terminal.draw(|f| ui::draw(f, &app, &mut ui)).unwrap();
    let buffer = terminal.backend().buffer();
    let line: String = (ui.message_area.x..ui.message_area.right())
        .map(|x| buffer[(x, ui.message_area.y)].symbol())
        .collect();
    assert!(line.contains("Today"), "missing date separator: {line}");
    let label = line.find("Today").unwrap();
    assert!((label as i32 - ui.message_area.width as i32 / 2).abs() < 6);
}

#[test]
fn downloaded_profile_photo_is_placed_beside_message() {
    let mut app = conversation("1704067200.000001");
    app.graphics = true;
    app.avatars.insert(
        "maya".into(),
        slacker::media::Pixels {
            width: 2,
            height: 2,
            rgb: vec![180; 12],
        },
    );
    let mut terminal = Terminal::new(TestBackend::new(120, 35)).unwrap();
    let mut ui = UiState::default();
    terminal.draw(|f| ui::draw(f, &app, &mut ui)).unwrap();
    let photo = ui
        .inline_images
        .iter()
        .find(|image| image.name == "avatar:maya")
        .expect("cached profile picture should be rendered");
    assert_eq!(photo.rect.x, ui.message_area.x + 1);
    assert_eq!((photo.rect.width, photo.rect.height), (4, 2));
}

#[test]
fn profile_photos_load_asynchronously_and_are_reused() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/photo.png", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(std::time::Duration::from_secs(3)))
            .unwrap();
        let mut request = [0; 4096];
        let n = socket.read(&mut request).unwrap();
        let request = String::from_utf8_lossy(&request[..n]).to_lowercase();
        assert!(!request.contains("authorization:"));
        assert!(!request.contains("cookie:"));
        let image = image::RgbImage::from_pixel(4, 4, image::Rgb([200, 100, 50]));
        let mut png = std::io::Cursor::new(Vec::new());
        image.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let png = png.into_inner();
        write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            png.len()
        )
        .unwrap();
        socket.write_all(&png).unwrap();
    });
    let slack =
        slacker::api::Slack::with_base_url("secret".into(), "http://127.0.0.1:1/".into()).unwrap();
    let mut app = App::new(Some(slacker::app::Worker::new(slack)), true);
    app.people.insert(
        "maya".into(),
        slacker::api::Person {
            avatar_url: url,
            ..Default::default()
        },
    );
    app.refresh_avatars(&["maya".into(), "maya".into()]);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !app.avatars.contains_key("maya") && std::time::Instant::now() < deadline {
        app.poll();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(
        app.avatars.contains_key("maya"),
        "photo should arrive without blocking UI"
    );
    assert_eq!(&app.avatars["maya"].rgb[..3], &[200, 100, 50]);
    server.join().unwrap();
    app.refresh_avatars(&["maya".into()]);
    assert!(app.avatars.contains_key("maya"));
}

#[test]
fn photos_without_graphics_use_pixel_cells_and_keep_text_clear() {
    let mut app = conversation("1704067200");
    app.avatars.insert(
        "maya".into(),
        slacker::media::Pixels {
            width: 2,
            height: 2,
            rgb: vec![180; 12],
        },
    );
    let mut terminal = Terminal::new(TestBackend::new(120, 35)).unwrap();
    let mut ui = UiState::default();
    terminal.draw(|f| ui::draw(f, &app, &mut ui)).unwrap();
    assert!(ui.inline_images.is_empty());
    let pixel = &terminal.backend().buffer()[(ui.message_area.x + 1, ui.message_area.y + 1)];
    assert_eq!(pixel.symbol(), "▀");
    assert_eq!(pixel.fg, ratatui::style::Color::Rgb(180, 180, 180));
}

#[test]
fn date_separators_group_messages_and_threads_keep_clickable_replies() {
    let mut app = conversation("1704067200");
    let mut second = app.messages[0].clone();
    second.ts = "1704067260".into();
    app.messages.push(second);
    let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
    let mut ui = UiState::default();
    terminal.draw(|f| ui::draw(f, &app, &mut ui)).unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert_eq!(
        text.matches("January 1, 2024").count() + text.matches("December 31, 2023").count(),
        1
    );
    let hit = ui
        .hits
        .iter()
        .find(|hit| hit.action == ui::Action::ThreadAt(0))
        .unwrap();
    let row: String = (hit.rect.x..hit.rect.right())
        .map(|x| terminal.backend().buffer()[(x, hit.rect.y)].symbol())
        .collect();
    assert!(row.contains("[2 replies →]"));
    app.thread_view = Some(slacker::app::ThreadView {
        messages: app.messages.clone(),
        ..Default::default()
    });
    terminal.draw(|f| ui::draw(f, &app, &mut ui)).unwrap();
    assert!(!ui.thread_area.is_empty());
    assert!(
        ui.hits
            .iter()
            .any(|hit| hit.action == ui::Action::ThreadMessage(0))
    );
    assert_eq!(
        ui.visible_avatars
            .iter()
            .filter(|user| *user == "maya")
            .count(),
        4
    );
}

#[test]
fn avatar_requests_are_deduplicated_bounded_and_not_retried_each_frame() {
    use slacker::app::{Job, Reply, Worker};
    let (jobs, requests) = std::sync::mpsc::channel();
    let (events, replies) = std::sync::mpsc::channel();
    let slack =
        slacker::api::Slack::with_base_url("secret".into(), "http://127.0.0.1:1/".into()).unwrap();
    let mut app = App::new(
        Some(Worker {
            jobs,
            replies,
            events,
            slack,
        }),
        false,
    );
    let mut users = Vec::new();
    for i in 0..11 {
        let id = format!("U{i}");
        app.people.insert(
            id.clone(),
            slacker::api::Person {
                avatar_url: format!("https://avatars.slack-edge.com/{id}.png"),
                ..Default::default()
            },
        );
        users.extend([id.clone(), id]);
    }
    app.refresh_avatars(&users);
    let Job::Avatars(first) = requests.try_recv().unwrap() else {
        panic!("wrong job")
    };
    assert_eq!(first.len(), 8);
    assert_ne!(first[0].0, first[1].0);
    app.refresh_avatars(&users);
    assert!(
        requests.try_recv().is_err(),
        "only one photo batch may be in flight"
    );
    app.receive(Reply::Avatars(Vec::new()));
    app.refresh_avatars(&users);
    let Job::Avatars(second) = requests.try_recv().unwrap() else {
        panic!("wrong job")
    };
    assert_eq!(second.len(), 3);
    assert!(second.iter().all(|user| !first.contains(user)));
    app.receive(Reply::Avatars(Vec::new()));
    app.refresh_avatars(&users);
    assert!(
        requests.try_recv().is_err(),
        "failed photos must not hammer the server every frame"
    );
}
