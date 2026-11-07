use crate::app::{App, Focus, Mode, safe_text};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use unicode_width::UnicodeWidthStr;

const BG: Color = Color::Rgb(19, 22, 32);
const PANEL: Color = Color::Rgb(25, 29, 42);
const FG: Color = Color::Rgb(220, 226, 240);
const MUTED: Color = Color::Rgb(132, 145, 168);
const ACCENT: Color = Color::Rgb(184, 162, 255);
const SELECTED: Color = Color::Rgb(45, 44, 67);
const GREEN: Color = Color::Rgb(133, 218, 177);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Channel(usize),
    Attachment(usize, usize),
    Read,
    Message(usize),
    SwitchResult(usize),
    Compose,
    Send,
    Thread,
    Media,
    Switch,
    Help,
    Refresh,
    Back,
    Close,
    Pause,
}
pub struct Hit {
    pub action: Action,
    pub rect: Rect,
}
#[derive(Default)]
pub struct UiState {
    pub hits: Vec<Hit>,
    pub message_area: Rect,
    pub sidebar_area: Rect,
    pub media_area: Rect,
    pub read_scroll_max: usize,
    channels: ListState,
    messages: ListState,
    switcher: ListState,
}

fn border(title: impl Into<Line<'static>>, active: bool) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(if active {
            ACCENT
        } else {
            Color::Rgb(55, 63, 82)
        }))
}

fn button(frame: &mut Frame, ui: &mut UiState, area: Rect, label: &str, action: Action) {
    if area.is_empty() {
        return;
    }
    frame.render_widget(
        Paragraph::new(label).style(Style::default().fg(ACCENT).bg(SELECTED)),
        area,
    );
    ui.hits.push(Hit { action, rect: area });
}

fn toolbar(frame: &mut Frame, ui: &mut UiState, area: Rect, buttons: &[(&str, Action)]) {
    let mut x = area.x;
    for (label, action) in buttons {
        let width = (label.width() as u16 + 2).min(area.right().saturating_sub(x));
        button(
            frame,
            ui,
            Rect::new(x, area.y, width, area.height.min(1)),
            &format!(" {label} "),
            *action,
        );
        x = x.saturating_add(width + 1);
    }
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width.saturating_sub(2));
    let height = height.min(area.height.saturating_sub(2));
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

pub fn display_text(app: &App, text: &str) -> String {
    let mut text = text.to_owned();
    for (id, name) in &app.users {
        text = text.replace(&format!("<@{id}>"), &format!("@{name}"));
    }
    safe_text(
        &text
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&"),
    )
}

pub fn draw(frame: &mut Frame, app: &App, ui: &mut UiState) {
    let area = frame.area();
    ui.hits.clear();
    ui.media_area = Rect::default();
    frame.render_widget(Block::default().style(Style::default().bg(BG).fg(FG)), area);
    if area.width < 38 || area.height < 12 {
        frame.render_widget(
            Paragraph::new("Make the terminal at least 38 × 12.\nCtrl+C quits.")
                .style(Style::default().fg(ACCENT)),
            area,
        );
        return;
    }
    if let Some(viewer) = &app.viewer {
        let chunks = Layout::vertical([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(area);
        let name = &viewer.prepared.title;
        frame.render_widget(
            Paragraph::new(format!("  MEDIA / {}", safe_text(name)))
                .style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
            chunks[0],
        );
        ui.media_area = chunks[1].inner(ratatui::layout::Margin {
            horizontal: 1,
            vertical: 0,
        });
        if viewer.last.is_none() {
            frame.render_widget(
                Paragraph::new(viewer.note.as_str())
                    .centered()
                    .style(Style::default().fg(MUTED)),
                ui.media_area,
            );
        }
        toolbar(
            frame,
            ui,
            chunks[2],
            &[
                ("Esc Close", Action::Close),
                (
                    if viewer
                        .video
                        .as_ref()
                        .is_some_and(|v| v.paused.load(std::sync::atomic::Ordering::Relaxed))
                    {
                        "Space Resume"
                    } else {
                        "Space Pause"
                    },
                    Action::Pause,
                ),
            ],
        );
        return;
    }
    if app.mode == Mode::Read {
        let popup = area.inner(ratatui::layout::Margin {
            horizontal: 1,
            vertical: 1,
        });
        let block = border(" Message · j/k or wheel scroll · Esc closes ", true);
        let inner = block.inner(popup);
        frame.render_widget(block, popup);
        let chunks = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);
        let mut lines = Vec::new();
        let mut attachment_lines = Vec::new();
        if let Some(message) = app.messages.get(app.selected) {
            for line in display_text(app, &message.text).split('\n') {
                lines.extend(
                    textwrap::wrap(line, chunks[0].width.max(1) as usize)
                        .into_iter()
                        .map(|s| Line::from(s.into_owned())),
                );
            }
            for (index, file) in message.files.iter().enumerate() {
                if file.is_media() {
                    attachment_lines.push((index, lines.len()));
                }
                lines.push(Line::from(Span::styled(
                    format!(" ▧ {}", safe_text(&file.name)),
                    Style::default().fg(GREEN),
                )));
            }
        }
        ui.read_scroll_max = lines.len().saturating_sub(chunks[0].height as usize);
        let scroll = app.read_scroll.min(ui.read_scroll_max);
        frame.render_widget(
            Paragraph::new(lines)
                .scroll((scroll.min(u16::MAX as usize) as u16, 0))
                .style(Style::default().fg(FG)),
            chunks[0],
        );
        toolbar(
            frame,
            ui,
            chunks[1],
            &[
                ("Esc Close", Action::Close),
                ("t Thread", Action::Thread),
                ("v Media", Action::Media),
            ],
        );
        for (index, row) in attachment_lines {
            if row >= scroll && row < scroll + chunks[0].height as usize {
                ui.hits.push(Hit {
                    rect: Rect::new(
                        chunks[0].x,
                        chunks[0].y + (row - scroll) as u16,
                        chunks[0].width,
                        1,
                    ),
                    action: Action::Attachment(app.selected, index),
                });
            }
        }
        ui.message_area = chunks[0];
        return;
    }
    let rows = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(4),
        Constraint::Length(1),
    ])
    .split(area);
    let header = Layout::horizontal([Constraint::Min(10), Constraint::Length(28)]).split(rows[0]);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " ◆ slacker  ",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(safe_text(&app.workspace), Style::default().fg(MUTED)),
        ])),
        header[0],
    );
    toolbar(
        frame,
        ui,
        header[1],
        &[("Ctrl+K Jump", Action::Switch), ("? Help", Action::Help)],
    );
    let sidebar_width = if area.width >= 85 { 25 } else { 19 };
    let cols =
        Layout::horizontal([Constraint::Length(sidebar_width), Constraint::Min(15)]).split(rows[1]);
    ui.sidebar_area = cols[0];
    let sidebar = border(" Conversations ", app.focus == Focus::Channels);
    let inner = sidebar.inner(cols[0]);
    frame.render_widget(sidebar, cols[0]);
    let items: Vec<_> = app
        .conversations
        .iter()
        .enumerate()
        .map(|(i, c)| {
            ListItem::new(safe_text(&format!(
                " {}{}",
                if i == app.channel { "● " } else { "  " },
                c.label()
            )))
            .style(Style::default().fg(if i == app.channel { GREEN } else { MUTED }))
        })
        .collect();
    ui.channels.select(Some(app.sidebar));
    frame.render_stateful_widget(
        List::new(items)
            .style(Style::default().bg(PANEL))
            .highlight_style(Style::default().bg(SELECTED).fg(FG)),
        inner,
        &mut ui.channels,
    );
    for (row, index) in (ui.channels.offset()..app.conversations.len())
        .take(inner.height as usize)
        .enumerate()
    {
        ui.hits.push(Hit {
            rect: Rect::new(inner.x, inner.y + row as u16, inner.width, 1),
            action: Action::Channel(index),
        });
    }
    let main = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(5),
    ])
    .split(cols[1]);
    let title = app
        .conversations
        .get(app.channel)
        .map(|c| c.label())
        .unwrap_or_else(|| "Getting started".into());
    let title = if app.thread.is_some() {
        format!(" {title} / Thread ")
    } else {
        format!(" {title} ")
    };
    frame.render_widget(border(safe_text(&title), false), main[0]);
    let title_inner = main[0].inner(ratatui::layout::Margin {
        horizontal: 1,
        vertical: 1,
    });
    toolbar(
        frame,
        ui,
        title_inner,
        &[
            ("R Refresh", Action::Refresh),
            ("t Thread", Action::Thread),
            ("v Media", Action::Media),
            ("Back", Action::Back),
        ],
    );
    ui.message_area = main[1];
    let width = main[1].width.saturating_sub(4).max(1) as usize;
    let mut heights = Vec::new();
    let mut attachment_rows = Vec::new();
    let items: Vec<_> = app
        .messages
        .iter()
        .map(|m| {
            let author = app.users.get(&m.user).cloned().unwrap_or_else(|| {
                if m.username.is_empty() {
                    m.user.clone()
                } else {
                    m.username.clone()
                }
            });
            let author = if author.is_empty() {
                "Slack".into()
            } else {
                author
            };
            let time =
                m.ts.split('.')
                    .next()
                    .and_then(|s| s.parse::<u64>().ok())
                    .map(|n| format!("{:02}:{:02} UTC", (n / 3600) % 24, (n / 60) % 60))
                    .unwrap_or_default();
            let mut lines = vec![Line::from(vec![
                Span::styled(
                    format!(" {} ", safe_text(&author)),
                    Style::default()
                        .fg(if m.user == app.me { GREEN } else { ACCENT })
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(time, Style::default().fg(MUTED)),
            ])];
            let text = display_text(app, &m.text);
            for paragraph in text.split('\n') {
                for line in textwrap::wrap(paragraph, width) {
                    lines.push(Line::from(format!(" {line}")));
                }
            }
            let mut file_rows = Vec::new();
            for (file_index, file) in m.files.iter().enumerate() {
                if file.is_media() {
                    file_rows.push((file_index, lines.len() as u16));
                }
                lines.push(Line::from(Span::styled(
                    format!(
                        " {} {}",
                        if file.is_video() {
                            "▶"
                        } else if file.is_media() {
                            "▧"
                        } else {
                            "↳"
                        },
                        safe_text(&file.name)
                    ),
                    Style::default().fg(GREEN),
                )));
            }
            if m.reply_count > 0 {
                lines.push(Line::from(Span::styled(
                    format!(" ↳ {} replies · t to open", m.reply_count),
                    Style::default().fg(ACCENT),
                )));
            }
            lines.push(Line::from(""));
            // Keep a single long message navigable without forcing other rows offscreen.
            let max_lines = main[1].height.max(1) as usize;
            if lines.len() > max_lines {
                lines.truncate(max_lines);
                if let Some(last) = lines.last_mut() {
                    *last = Line::from(Span::styled(
                        " … press o to read the full message",
                        Style::default().fg(MUTED),
                    ));
                }
            }
            attachment_rows.push(file_rows);
            heights.push(lines.len() as u16);
            ListItem::new(Text::from(lines))
        })
        .collect();
    ui.messages.select(if app.messages.is_empty() {
        None
    } else {
        Some(app.selected)
    });
    frame.render_stateful_widget(
        List::new(items)
            .highlight_style(Style::default().bg(SELECTED))
            .style(Style::default().fg(FG)),
        main[1],
        &mut ui.messages,
    );
    let mut y = main[1].y;
    for (index, height) in heights.iter().enumerate().skip(ui.messages.offset()) {
        let visible = (*height).min(main[1].bottom().saturating_sub(y));
        if visible == 0 {
            break;
        }
        ui.hits.push(Hit {
            rect: Rect::new(main[1].x, y, main[1].width, visible),
            action: Action::Message(index),
        });
        for (file_index, row) in &attachment_rows[index] {
            if *row < visible {
                ui.hits.push(Hit {
                    rect: Rect::new(main[1].x, y + row, main[1].width, 1),
                    action: Action::Attachment(index, *file_index),
                });
            }
        }
        y += visible;
    }
    if app.messages.is_empty() {
        frame.render_widget(
            Paragraph::new(if app.loading {
                "Loading conversation…"
            } else {
                "No messages to display."
            })
            .style(Style::default().fg(MUTED))
            .centered(),
            main[1],
        );
    }
    toolbar(
        frame,
        ui,
        main[2],
        &[
            ("i Compose", Action::Compose),
            ("Send ↵", Action::Send),
            ("o Read", Action::Read),
        ],
    );
    let composer = border(
        if app.thread.is_some() {
            " Reply to thread · Enter sends · Alt+Enter newline "
        } else {
            " Message · Enter sends · Alt+Enter newline "
        },
        app.mode == Mode::Compose,
    );
    let composer_inner = composer.inner(main[3]);
    frame.render_widget(composer, main[3]);
    ui.hits.push(Hit {
        rect: composer_inner,
        action: Action::Compose,
    });
    let text = if app.editor.text.is_empty() && app.mode != Mode::Compose {
        "Press i or click here to write a message…"
    } else {
        &app.editor.text
    };
    let before = &app.editor.text[..app.editor.cursor];
    let line_count = before.lines().count().max(1) + usize::from(before.ends_with('\n'));
    let scroll = line_count.saturating_sub(composer_inner.height as usize) as u16;
    let cursor_column = before.rsplit('\n').next().unwrap_or("").width() as u16;
    let horizontal = cursor_column.saturating_sub(composer_inner.width.saturating_sub(1));
    frame.render_widget(
        Paragraph::new(text)
            .scroll((scroll, horizontal))
            .style(Style::default().fg(if app.mode == Mode::Compose { FG } else { MUTED })),
        composer_inner,
    );
    if app.mode == Mode::Compose && !composer_inner.is_empty() {
        let x = before.rsplit('\n').next().unwrap_or("").width() as u16;
        let y = (line_count as u16).saturating_sub(1).saturating_sub(scroll);
        frame.set_cursor_position((
            composer_inner.x
                + x.saturating_sub(horizontal)
                    .min(composer_inner.width.saturating_sub(1)),
            composer_inner.y + y.min(composer_inner.height.saturating_sub(1)),
        ));
    }
    let mode = match app.mode {
        Mode::Compose => "INSERT",
        Mode::Switcher => "JUMP",
        _ => "NORMAL",
    };
    frame.render_widget(
        Paragraph::new(format!(" {mode} │ {}", safe_text(&app.status)))
            .style(Style::default().fg(MUTED).bg(PANEL)),
        rows[2],
    );
    if app.mode == Mode::Help {
        let popup = centered(area, 76, 24);
        frame.render_widget(Clear, popup);
        ui.hits.clear();
        let block = border(" Keyboard & mouse ", true).style(Style::default().bg(PANEL));
        let inner = block.inner(popup);
        frame.render_widget(block, popup);
        let help = "NAVIGATE\n j / k, ↑ / ↓    Select a message or conversation\n h / l, Tab      Focus conversations / messages\n Enter           Open conversation or selected thread\n Ctrl+K          Find a conversation (fuzzy search)\n g / G           First / last item\n PgUp / PgDown   More history / move five messages\n\nWRITE\n i               Compose   ·   Esc returns to navigation\n Enter           Send      ·   Alt+Enter adds a newline\n\nREAD & MEDIA\n o               Read full message (j/k or wheel scroll)\n t               Open selected message's thread\n v               View image / video attachment\n Space           Pause / resume video (silent playback)\n Esc             Close media, thread, or popup\n R               Refresh / retry connection\n q / Ctrl+C      Quit (q types normally while composing)\n\nMOUSE  Click channels, messages, composer, and buttons.\n       Wheel scrolls the list under the pointer.";
        frame.render_widget(
            Paragraph::new(help)
                .wrap(Wrap { trim: false })
                .style(Style::default().fg(FG)),
            inner,
        );
        button(
            frame,
            ui,
            Rect::new(popup.right().saturating_sub(10), popup.y, 9, 1),
            " × Close ",
            Action::Close,
        );
    } else if app.mode == Mode::Switcher {
        let popup = centered(area, 60, 18);
        frame.render_widget(Clear, popup);
        ui.hits.clear();
        let block =
            border(" Jump to conversation · Esc closes ", true).style(Style::default().bg(PANEL));
        let inner = block.inner(popup);
        frame.render_widget(block, popup);
        let parts = Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).split(inner);
        frame.render_widget(
            Paragraph::new(format!(" > {}", app.query.text)).style(Style::default().fg(ACCENT)),
            parts[0],
        );
        let matched = app.filtered();
        let items: Vec<_> = matched
            .iter()
            .map(|i| ListItem::new(safe_text(&app.conversations[*i].label())))
            .collect();
        ui.switcher.select(Some(app.switch_selected));
        frame.render_stateful_widget(
            List::new(items).highlight_style(Style::default().bg(SELECTED).fg(ACCENT)),
            parts[1],
            &mut ui.switcher,
        );
        for (row, index) in matched
            .iter()
            .skip(ui.switcher.offset())
            .take(parts[1].height as usize)
            .enumerate()
        {
            ui.hits.push(Hit {
                rect: Rect::new(parts[1].x, parts[1].y + row as u16, parts[1].width, 1),
                action: Action::SwitchResult(*index),
            });
        }
        if matched.is_empty() {
            frame.render_widget(
                Paragraph::new("No matching conversations").style(Style::default().fg(MUTED)),
                parts[1],
            );
        }
        button(
            frame,
            ui,
            Rect::new(popup.right().saturating_sub(10), popup.y, 9, 1),
            " × Close ",
            Action::Close,
        );
    }
}

pub fn mouse(app: &mut App, ui: &mut UiState, event: MouseEvent) {
    let point = Position::new(event.column, event.row);
    if matches!(
        event.kind,
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
    ) {
        let delta = if event.kind == MouseEventKind::ScrollUp {
            -1
        } else {
            1
        };
        if app.viewer.is_some() || app.mode == Mode::Help {
            return;
        }
        if app.mode == Mode::Read {
            app.read_scroll = app
                .read_scroll
                .min(ui.read_scroll_max)
                .saturating_add_signed(delta)
                .min(ui.read_scroll_max);
        } else if app.mode == Mode::Switcher {
            app.switch_selected = app
                .switch_selected
                .saturating_add_signed(delta)
                .min(app.filtered().len().saturating_sub(1));
        } else if ui.sidebar_area.contains(point) {
            app.focus = Focus::Channels;
            app.move_selection(delta);
        } else if ui.message_area.contains(point) {
            app.focus = Focus::Messages;
            app.move_selection(delta);
        }
        return;
    }
    if event.kind != MouseEventKind::Down(MouseButton::Left) {
        return;
    }
    if let Some(hit) = ui.hits.iter().rev().find(|h| h.rect.contains(point)) {
        match hit.action {
            Action::Channel(i) | Action::SwitchResult(i) => {
                app.loading = false;
                app.open_channel(i);
            }
            Action::Message(i) => {
                app.selected = i;
                app.focus = Focus::Messages;
            }
            Action::Read => {
                app.read_scroll = 0;
                app.mode = Mode::Read;
            }
            Action::Attachment(message, index) => {
                app.selected = message;
                app.open_attachment(index);
            }
            Action::Compose => app.mode = Mode::Compose,
            Action::Send => app.send(),
            Action::Thread => {
                app.mode = Mode::Normal;
                app.open_thread();
            }
            Action::Media => app.open_media(),
            Action::Switch => app.key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL)),
            Action::Help => app.mode = Mode::Help,
            Action::Refresh => app.key(KeyEvent::new(KeyCode::Char('R'), KeyModifiers::NONE)),
            Action::Back => app.close_thread(),
            Action::Close => {
                app.viewer = None;
                app.mode = Mode::Normal;
            }
            Action::Pause => {
                if let Some(video) = app.viewer.as_ref().and_then(|v| v.video.as_ref()) {
                    video.toggle();
                }
            }
        }
    }
}
