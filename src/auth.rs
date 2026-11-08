use crate::api::Slack;
use aes::cipher::{BlockModeDecrypt, KeyIvInit, block_padding::Pkcs7};
use anyhow::{Context, Result, anyhow, bail};
use rusty_leveldb::{DB, LdbIterator, Options};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

#[derive(Clone)]
pub struct Workspace {
    pub name: String,
    pub team_id: String,
    pub domain: String,
    pub profile: PathBuf,
}

pub fn parse_workspaces(bytes: &[u8], profile: &Path) -> Result<Vec<Workspace>> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).context("Could not read Slack workspace metadata")?;
    let workspaces = value["workspaces"]
        .as_object()
        .context("Slack has no signed-in workspaces")?;
    let mut result = Vec::new();
    for (key, value) in workspaces {
        let id = value["id"].as_str().unwrap_or(key);
        let domain = value["domain"].as_str().unwrap_or("");
        if id.is_empty()
            || !id.chars().all(|c| c.is_ascii_alphanumeric())
            || domain.is_empty()
            || !domain
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            continue;
        }
        let name = value["name"].as_str().unwrap_or(domain);
        result.push(Workspace {
            name: crate::app::safe_text(name).replace('\n', " "),
            team_id: id.into(),
            domain: domain.into(),
            profile: profile.into(),
        });
    }
    result.sort_by_key(|w| w.name.to_lowercase());
    Ok(result)
}

pub fn discover() -> Vec<Workspace> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    if cfg!(target_os = "macos") {
        paths.push(home.join("Library/Application Support/Slack"));
        paths.push(home.join(
            "Library/Containers/com.tinyspeck.slackmacgap/Data/Library/Application Support/Slack",
        ));
    } else if cfg!(target_os = "windows") {
        if let Some(roaming) = std::env::var_os("APPDATA") {
            paths.push(PathBuf::from(roaming).join("Slack"));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            paths.push(PathBuf::from(local).join(
                "Packages/com.tinyspeck.slackdesktop_8yrtsj140pw4g/LocalCache/Roaming/Slack",
            ));
        }
    } else {
        if let Some(config) = dirs::config_dir() {
            paths.push(config.join("Slack"));
        }
        paths.push(home.join(".var/app/com.slack.Slack/config/Slack"));
        paths.push(home.join("snap/slack/current/.config/Slack"));
    }
    let mut workspaces = Vec::new();
    for profile in paths {
        if let Ok(bytes) = fs::read(profile.join("storage/root-state.json"))
            && let Ok(mut found) = parse_workspaces(&bytes, &profile)
        {
            workspaces.append(&mut found);
        }
    }
    workspaces.sort_by_key(|w| w.name.to_lowercase());
    workspaces
}

pub fn decode_local_value(bytes: &[u8]) -> Result<String> {
    match bytes.first() {
        Some(0) => {
            let bytes = &bytes[1..];
            if !bytes.len().is_multiple_of(2) {
                bail!("Invalid Slack local storage encoding");
            }
            let units: Vec<u16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect();
            String::from_utf16(&units).map_err(|_| anyhow!("Invalid Slack local storage text"))
        }
        Some(1) => Ok(bytes[1..].iter().map(|b| char::from(*b)).collect()),
        _ => String::from_utf8(bytes.to_vec())
            .map_err(|_| anyhow!("Invalid Slack local storage text")),
    }
}

pub fn read_tokens(profile: &Path) -> Result<HashMap<String, String>> {
    let source = profile.join("Local Storage/leveldb");
    let snapshot = tempfile::tempdir()?;
    for entry in fs::read_dir(&source)
        .context("Slack's local session store is missing. Sign in to Slack desktop first.")?
    {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            fs::copy(entry.path(), snapshot.path().join(entry.file_name()))
                .context("Could not snapshot Slack's local session store")?;
        }
    }
    let options = Options {
        create_if_missing: false,
        ..Default::default()
    };
    let mut db = DB::open(snapshot.path(), options).map_err(|_| {
        anyhow!("Could not read Slack's local session store. Open Slack and retry.")
    })?;
    let mut iterator = db
        .new_iter()
        .map_err(|_| anyhow!("Could not read Slack's workspace sessions"))?;
    let mut tokens = HashMap::new();
    while let Some((key, value)) = iterator.next() {
        if !key.ends_with(b"localConfig_v2") {
            continue;
        }
        let Ok(text) = decode_local_value(&value) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        if let Some(teams) = value["teams"].as_object() {
            for (id, team) in teams {
                if let Some(token) = team["token"]
                    .as_str()
                    .filter(|token| token.starts_with("xoxc-"))
                {
                    tokens.insert(id.clone(), token.to_owned());
                }
            }
        }
    }
    Ok(tokens)
}

pub fn read_cookie(profile: &Path) -> Result<(String, Vec<u8>)> {
    let source = if profile.join("Cookies").is_file() {
        profile.join("Cookies")
    } else {
        profile.join("Network/Cookies")
    };
    let snapshot = tempfile::tempdir()?;
    let target = snapshot.path().join("Cookies");
    let source =
        rusqlite::Connection::open_with_flags(&source, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .context(
                "Slack's session cookie is missing or locked. Sign in to Slack desktop first.",
            )?;
    let mut db = rusqlite::Connection::open(&target)
        .context("Could not create a private Slack session snapshot")?;
    {
        let backup = rusqlite::backup::Backup::new(&source, &mut db)
            .context("Could not snapshot Slack's session cookie")?;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if matches!(backup.step(128)?, rusqlite::backup::StepResult::Done) {
                break;
            }
            if std::time::Instant::now() >= deadline {
                bail!("Slack's session database is busy. Retry slacker auth.");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    db.query_row("SELECT value, encrypted_value FROM cookies WHERE host_key IN ('.slack.com','slack.com') AND name='d' ORDER BY length(encrypted_value) DESC LIMIT 1",[],|row|Ok((row.get(0)?,row.get(1)?)))
        .context("Slack has no signed-in session cookie. Sign in to Slack desktop and retry.")
}

pub fn decrypt_cookie(encrypted: &[u8], password: &[u8], rounds: u32) -> Result<String> {
    if encrypted.len() < 19 || !(encrypted.starts_with(b"v10") || encrypted.starts_with(b"v11")) {
        bail!("Unsupported Slack session encryption");
    }
    let mut key = [0u8; 16];
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(password, b"saltysalt", rounds, &mut key);
    let plaintext = cbc::Decryptor::<aes::Aes128>::new(&key.into(), &[b' '; 16].into())
        .decrypt_padded_vec::<Pkcs7>(&encrypted[3..])
        .map_err(|_| anyhow!("Could not unlock Slack's session with this storage key"))?;
    let mut value = plaintext.as_slice();
    for domain in [".slack.com", "slack.com"] {
        let hash = Sha256::digest(domain.as_bytes());
        if value.starts_with(&hash) {
            value = &value[hash.len()..];
            break;
        }
    }
    if value.is_empty() || !value.iter().all(|b| (0x21..=0x7e).contains(b)) {
        bail!("Could not unlock Slack's session with this storage key");
    }
    String::from_utf8(value.to_vec()).map_err(|_| anyhow!("Invalid Slack session cookie"))
}

#[cfg(target_os = "macos")]
fn desktop_cookie(profile: &Path) -> Result<String> {
    let (plain, encrypted) = read_cookie(profile)?;
    if !plain.is_empty() {
        return Ok(plain);
    }
    let store = profile.to_string_lossy().contains("/Containers/");
    let mut accounts = vec![
        if store {
            "Slack App Store Key"
        } else {
            "Slack Key"
        },
        "",
    ];
    accounts.dedup();
    for account in accounts {
        let mut command = Command::new("/usr/bin/security");
        command.args(["find-generic-password", "-w", "-s", "Slack Safe Storage"]);
        if !account.is_empty() {
            command.args(["-a", account]);
        }
        let output = command
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .context("Could not access macOS Keychain")?;
        if output.status.success() {
            let mut password = output.stdout;
            while password.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
                password.pop();
            }
            if let Ok(cookie) = decrypt_cookie(&encrypted, &password, 1003) {
                return Ok(cookie);
            }
        }
    }
    bail!(
        "Could not unlock Slack's session. Allow access to ‘Slack Safe Storage’ in macOS Keychain, then run slacker auth again."
    )
}

#[cfg(target_os = "linux")]
fn desktop_cookie(profile: &Path) -> Result<String> {
    let (plain, encrypted) = read_cookie(profile)?;
    if !plain.is_empty() {
        return Ok(plain);
    }
    if encrypted.starts_with(b"v10") {
        return decrypt_cookie(&encrypted, b"peanuts", 1);
    }
    let output = Command::new("secret-tool")
        .args(["lookup", "application", "Slack"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .context("Unlock your desktop keyring and install secret-tool to read Slack's session")?;
    if !output.status.success() {
        bail!("Could not read Slack Safe Storage from the desktop keyring");
    }
    let mut password = output.stdout;
    while password.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
        password.pop();
    }
    decrypt_cookie(&encrypted, &password, 1)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn desktop_cookie(_profile: &Path) -> Result<String> {
    bail!("Desktop sign-in is currently supported on macOS and Linux")
}

#[derive(Deserialize, Serialize)]
struct Preference {
    team_id: String,
    profile: PathBuf,
}
fn preference_path() -> Result<PathBuf> {
    Ok(dirs::config_dir()
        .context("Could not locate your configuration directory")?
        .join("slacker/account.json"))
}
fn save_preference(workspace: &Workspace) -> Result<()> {
    let path = preference_path()?;
    let directory = path.parent().context("Invalid configuration path")?;
    fs::create_dir_all(directory)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
    }
    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    serde_json::to_writer(
        &mut file,
        &Preference {
            team_id: workspace.team_id.clone(),
            profile: workspace.profile.clone(),
        },
    )?;
    file.flush()?;
    file.persist(&path)
        .map_err(|_| anyhow!("Could not save the selected Slack workspace"))?;
    Ok(())
}

fn sign_in_to_desktop() -> Result<()> {
    println!(
        "\nConnect to Slack\n\nSign in to the Slack desktop app, then return here.\nPress Enter to open Slack, or type q to cancel."
    );
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    if answer.trim().eq_ignore_ascii_case("q") {
        bail!("Sign-in canceled");
    }
    #[cfg(target_os = "macos")]
    let status = Command::new("open").args(["-a", "Slack"]).status();
    #[cfg(target_os = "linux")]
    let status = Command::new("xdg-open").arg("slack://signin").status();
    #[cfg(target_os = "windows")]
    let status = Command::new("cmd")
        .args(["/C", "start", "", "slack://signin"])
        .status();
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    let status: io::Result<std::process::ExitStatus> =
        Err(io::Error::other("Unsupported platform"));
    if !status.is_ok_and(|status| status.success()) {
        bail!("Install and sign in to Slack desktop, then run slacker auth");
    }
    println!("Once you are signed in to Slack, press Enter to continue.");
    answer.clear();
    io::stdin().read_line(&mut answer)?;
    Ok(())
}

pub fn connect(requested: Option<&str>, choose_again: bool) -> Result<Slack> {
    connect_inner(requested, choose_again, true)
}

/// Let the TUI restore cached history before the network validates the session.
pub fn connect_for_app(requested: Option<&str>) -> Result<Slack> {
    connect_inner(requested, false, false)
}

fn connect_inner(requested: Option<&str>, choose_again: bool, validate: bool) -> Result<Slack> {
    let mut workspaces = discover();
    if workspaces.is_empty() {
        if !io::stdin().is_terminal() {
            bail!("Sign in to Slack desktop, then run slacker in a terminal");
        }
        sign_in_to_desktop()?;
        workspaces = discover();
    }
    if workspaces.is_empty() {
        bail!(
            "No signed-in Slack desktop workspaces found. Finish signing in, then run slacker auth."
        );
    }
    let saved = preference_path()
        .ok()
        .and_then(|path| fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<Preference>(&bytes).ok());
    let workspace = if let Some(requested) = requested {
        workspaces
            .iter()
            .find(|w| {
                w.team_id == requested
                    || w.domain.eq_ignore_ascii_case(requested)
                    || w.name.eq_ignore_ascii_case(requested)
            })
            .cloned()
            .context("That workspace is not signed in to Slack desktop")?
    } else if !choose_again
        && let Some(saved) = saved
        && let Some(workspace) = workspaces
            .iter()
            .find(|w| w.team_id == saved.team_id && w.profile == saved.profile)
    {
        workspace.clone()
    } else {
        pick_workspace(&workspaces)?
    };
    println!("Connecting to {}…", workspace.name);
    let mut tokens = read_tokens(&workspace.profile)?;
    let token=tokens.remove(&workspace.team_id).context("This workspace has no active desktop session. Open it in Slack desktop, then retry slacker auth.")?;
    let cookie = desktop_cookie(&workspace.profile)?;
    let mut client = Slack::with_session(token, cookie, &workspace.domain)?;
    if validate {
        client.validate().context("Slack could not validate this desktop session. Open Slack desktop and run slacker auth again.")?;
    }
    save_preference(&workspace)?;
    if validate {
        println!("Connected to {}.", workspace.name);
    }
    Ok(client)
}

fn pick_workspace(workspaces: &[Workspace]) -> Result<Workspace> {
    use crossterm::{
        event::{
            self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind,
            MouseButton, MouseEventKind,
        },
        execute,
    };
    use ratatui::{
        layout::{Constraint, Layout, Rect},
        style::{Color, Modifier, Style},
        widgets::{Block, BorderType, List, ListItem, ListState, Paragraph},
    };
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = execute!(io::stdout(), DisableMouseCapture);
            ratatui::restore();
        }
    }
    let mut terminal = ratatui::try_init()?;
    let _restore = Restore;
    execute!(io::stdout(), EnableMouseCapture)?;
    let mut selected = 0usize;
    let mut state = ListState::default();
    let mut inner = Rect::default();
    loop {
        terminal.draw(|frame| {
            let area = frame.area();
            frame.render_widget(
                Block::default().style(
                    Style::default()
                        .bg(Color::Rgb(25, 17, 30))
                        .fg(Color::Rgb(250, 246, 252)),
                ),
                area,
            );
            let parts = Layout::vertical([
                Constraint::Length(5),
                Constraint::Min(1),
                Constraint::Length(2),
            ])
            .split(area);
            frame.render_widget(
                Paragraph::new(
                    "\n  Connect to Slack\n  Choose an account already signed in to Slack desktop.",
                ),
                parts[0],
            );
            let block = Block::bordered()
                .border_type(BorderType::Rounded)
                .title(" Your workspaces ")
                .border_style(Style::default().fg(Color::Rgb(183, 165, 191)));
            inner = block.inner(parts[1]);
            frame.render_widget(block, parts[1]);
            let items: Vec<_> = workspaces
                .iter()
                .map(|w| ListItem::new(format!(" {}\n {}.slack.com", w.name, w.domain)))
                .collect();
            state.select(Some(selected));
            frame.render_stateful_widget(
                List::new(items).highlight_style(
                    Style::default()
                        .bg(Color::Rgb(69, 43, 77))
                        .add_modifier(Modifier::BOLD),
                ),
                inner,
                &mut state,
            );
            frame.render_widget(
                Paragraph::new(" Enter to connect · ↑/↓ or click to choose · Esc to cancel"),
                parts[2],
            );
        })?;
        if !event::poll(Duration::from_millis(50))? {
            continue;
        }
        match event::read()? {
            Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
                KeyCode::Enter => return Ok(workspaces[selected].clone()),
                KeyCode::Esc | KeyCode::Char('q') => bail!("Sign-in canceled"),
                KeyCode::Char('c') if key.modifiers.contains(event::KeyModifiers::CONTROL) => {
                    bail!("Sign-in canceled")
                }
                KeyCode::Up | KeyCode::Char('k') => selected = selected.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => {
                    selected = (selected + 1).min(workspaces.len() - 1)
                }
                _ => {}
            },
            Event::Mouse(mouse)
                if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                    && inner.contains((mouse.column, mouse.row).into()) =>
            {
                selected =
                    (state.offset() + (mouse.row - inner.y) as usize / 2).min(workspaces.len() - 1);
            }
            _ => {}
        }
    }
}
