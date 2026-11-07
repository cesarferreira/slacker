use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use crossterm::{
    cursor::{MoveTo, RestorePosition, SavePosition},
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyEventKind,
    },
    execute,
};
use slacker::{
    api::Slack,
    app::{App, Job, Mode, Viewer, Worker},
    media::{self, Prepared},
    ui::{self, UiState},
};
use std::{
    io::{self, IsTerminal, Write},
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Parser)]
#[command(
    version,
    about = "Slack in your terminal, with Kitty images and video",
    after_help = "Run `slacker --demo` to explore without a Slack account.\nAuthentication: set SLACK_TOKEN to a Slack user OAuth token.\nPress ? in the app for keyboard and mouse controls."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    /// Explore a local demo without connecting to Slack
    #[arg(long)]
    demo: bool,
    /// Graphics mode; auto recognizes Kitty, Ghostty and WezTerm
    #[arg(long, value_enum, default_value = "auto", global = true)]
    graphics: Graphics,
    /// Refresh interval in seconds (0 disables automatic refresh)
    #[arg(long, default_value_t = 0)]
    refresh: u64,
    /// Start in a conversation by name or ID
    #[arg(long)]
    channel: Option<String>,
}
#[derive(Clone, Copy, ValueEnum)]
enum Graphics {
    Auto,
    Kitty,
    Off,
}
#[derive(Subcommand)]
enum Command {
    /// Preview a local image or video using Kitty graphics
    Media {
        path: PathBuf,
        /// Decode with FFmpeg even if the extension is unrecognized
        #[arg(long)]
        video: bool,
    },
    /// Print terminal, authentication and FFmpeg availability
    Doctor,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if cli.demo && cli.command.is_some() {
        bail!("--demo cannot be combined with a subcommand");
    }
    let graphics = match cli.graphics {
        Graphics::Auto => media::supported(),
        Graphics::Kitty => true,
        Graphics::Off => false,
    };
    if matches!(cli.command, Some(Command::Doctor)) {
        println!(
            "Kitty graphics: {}",
            if graphics {
                "enabled"
            } else {
                "unavailable / disabled"
            }
        );
        println!(
            "Slack token: {}",
            if token().is_some() {
                "set"
            } else {
                "missing (set SLACK_TOKEN)"
            }
        );
        let ffmpeg = std::process::Command::new("ffmpeg")
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        println!(
            "FFmpeg: {}",
            if ffmpeg {
                "available"
            } else {
                "missing (required for videos / GIFs)"
            }
        );
        println!("Mouse: enabled in TUI (hold Shift for terminal text selection)");
        return Ok(());
    }
    if !io::stdout().is_terminal() || !io::stdin().is_terminal() {
        bail!("The TUI needs an interactive terminal. Try slacker --help or slacker doctor.");
    }
    let mut app = if let Some(Command::Media { path, video }) = cli.command {
        if !graphics {
            bail!(
                "Media needs a Kitty-compatible terminal. Use --graphics kitty to force the protocol, or run slacker doctor."
            );
        }
        let video = video || media::video_path(&path);
        let prepared = Prepared::local(path, video)?;
        let mut app = App::new(None, graphics);
        app.viewer = Some(Viewer::new(prepared)?);
        app
    } else if cli.demo {
        App::demo(graphics)
    } else {
        let token = token().context("Set SLACK_TOKEN to a Slack user OAuth token, or run slacker --demo. See README for required scopes.")?;
        let worker = Worker::new(Slack::new(token)?);
        worker.jobs.send(Job::Bootstrap)?;
        App::new(Some(worker), graphics)
    };
    let standalone = app.viewer.is_some();
    let mut terminal = ratatui::try_init()?;
    let guard = TerminalGuard { graphics };
    execute!(io::stdout(), EnableMouseCapture, EnableBracketedPaste)?;
    let result = run(
        &mut terminal,
        &mut app,
        standalone,
        cli.refresh,
        cli.channel,
    );
    drop(app);
    drop(guard);
    result
}
fn token() -> Option<String> {
    std::env::var("SLACK_TOKEN")
        .ok()
        .filter(|s| !s.trim().is_empty())
}
struct TerminalGuard {
    graphics: bool,
}
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.graphics {
            let _ = media::clear(&mut io::stdout());
        }
        let _ = execute!(io::stdout(), DisableMouseCapture, DisableBracketedPaste);
        ratatui::restore();
    }
}

fn run(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    standalone: bool,
    refresh: u64,
    mut initial_channel: Option<String>,
) -> Result<()> {
    let mut ui = UiState::default();
    let mut last_refresh = Instant::now();
    let mut had_viewer = false;
    loop {
        app.poll();
        if !app.conversations.is_empty() {
            if let Some(name) = initial_channel.take() {
                if let Some(i) = app
                    .conversations
                    .iter()
                    .position(|c| c.name == name.trim_start_matches('#') || c.id == name)
                {
                    app.loading = false;
                    app.open_channel(i);
                } else {
                    app.status = format!("Conversation {name} not found");
                }
            }
        }
        if refresh > 0 && last_refresh.elapsed() >= Duration::from_secs(refresh.max(60)) {
            if app.mode == Mode::Normal && app.viewer.is_none() && !app.loading && !app.sending {
                app.refresh(false);
            }
            last_refresh = Instant::now();
        }
        if had_viewer && app.viewer.is_none() {
            media::clear(&mut io::stdout())?;
            terminal.clear()?;
            if standalone {
                break;
            }
        }
        had_viewer = app.viewer.is_some();
        if let Some(viewer) = &mut app.viewer {
            if let Some(video) = &mut viewer.video {
                if let Some(pixels) = video.poll() {
                    viewer.last = Some(pixels);
                    viewer.dirty = true;
                }
                if video.ended() && viewer.last.is_none() {
                    viewer.note =
                        "FFmpeg could not decode this video. Esc closes the viewer.".into();
                }
            }
        }
        terminal.draw(|frame| ui::draw(frame, app, &mut ui))?;
        if app.mode == Mode::Read {
            app.read_scroll = app.read_scroll.min(ui.read_scroll_max);
        }
        if let Some(viewer) = &mut app.viewer {
            if viewer.dirty && !ui.media_area.is_empty() {
                if let Some(pixels) = &viewer.last {
                    let (columns, rows) = pixels.fit(ui.media_area.width, ui.media_area.height);
                    let x = ui.media_area.x + (ui.media_area.width - columns) / 2;
                    let y = ui.media_area.y + (ui.media_area.height - rows) / 2;
                    let mut out = io::stdout().lock();
                    execute!(out, SavePosition, MoveTo(x, y))?;
                    media::transmit(
                        &mut out,
                        &pixels.rgb,
                        pixels.width,
                        pixels.height,
                        columns,
                        rows,
                    )?;
                    execute!(out, RestorePosition)?;
                    out.flush()?;
                    viewer.dirty = false;
                }
            }
        }
        if app.quit {
            break;
        }
        if event::poll(Duration::from_millis(30))? {
            match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => app.key(key),
                Event::Mouse(mouse) => ui::mouse(app, &mut ui, mouse),
                Event::Paste(text) => match app.mode {
                    Mode::Compose if !app.sending => {
                        app.editor.insert(&slacker::app::safe_text(&text))
                    }
                    Mode::Switcher => app
                        .query
                        .insert(&slacker::app::safe_text(&text).replace('\n', " ")),
                    _ => {}
                },
                Event::Resize(_, _) => {
                    if let Some(viewer) = &mut app.viewer {
                        media::clear(&mut io::stdout())?;
                        viewer.dirty = true;
                    }
                    terminal.clear()?;
                }
                _ => {}
            }
        }
    }
    Ok(())
}
