use crate::app::{App, Focus, Mode, safe_text};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Wrap,
    },
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const BG: Color = Color::Rgb(25, 17, 30);
const PANEL: Color = Color::Rgb(35, 19, 41);
const FG: Color = Color::Rgb(250, 246, 252);
const MUTED: Color = Color::Rgb(183, 165, 191);
const ACCENT: Color = Color::Rgb(215, 185, 230);
const SELECTED: Color = Color::Rgb(69, 43, 77);
const GREEN: Color = Color::Rgb(133, 218, 177);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Channel(usize),
    Attachment(usize, usize),
    Reaction(usize, usize),
    ThreadAt(usize),
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
#[derive(Clone, PartialEq, Eq)]
pub struct InlineImage {
    pub name: String,
    pub rect: Rect,
    pub id: u32,
}
#[derive(Default)]
pub struct UiState {
    pub hits: Vec<Hit>,
    pub inline_images: Vec<InlineImage>,
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
        .border_type(BorderType::Rounded)
        .title(title)
        .border_style(Style::default().fg(if active {
            ACCENT
        } else {
            Color::Rgb(66, 45, 74)
        }))
}

fn button(frame: &mut Frame, ui: &mut UiState, area: Rect, label: &str, action: Action) {
    if area.is_empty() {
        return;
    }
    frame.render_widget(
        Paragraph::new(label).style(Style::default().fg(MUTED)),
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

fn scrollbar(frame: &mut Frame, area: Rect, length: usize, position: usize, viewport: usize) {
    if length <= viewport || area.is_empty() {
        return;
    }
    let mut state = ScrollbarState::new(length)
        .position(position)
        .viewport_content_length(viewport);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(None)
            .thumb_symbol("┃")
            .thumb_style(Style::default().fg(Color::Rgb(103, 72, 117))),
        area,
        &mut state,
    );
}

fn ellipsize(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let mut result = String::new();
    let mut used = 0;
    for character in text.chars() {
        let size = character.width().unwrap_or(0);
        if used + size > width - 1 {
            break;
        }
        result.push(character);
        used += size;
    }
    result.push('…');
    result
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
    ui.inline_images.clear();
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
                ("Thread", Action::Thread),
                ("Media", Action::Media),
            ],
        );
        scrollbar(
            frame,
            chunks[0],
            ui.read_scroll_max + chunks[0].height as usize,
            scroll,
            chunks[0].height as usize,
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
                "  slacker  ",
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
    let sidebar_width = if area.width >= 85 {
        (area.width / 3).clamp(28, 42)
    } else if area.width >= 60 {
        23
    } else {
        17
    };
    let cols =
        Layout::horizontal([Constraint::Length(sidebar_width), Constraint::Min(15)]).split(rows[1]);
    ui.sidebar_area = cols[0];
    let sidebar = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(if app.focus == Focus::Channels {
            Color::Rgb(103, 72, 117)
        } else {
            Color::Rgb(57, 35, 65)
        }))
        .style(Style::default().bg(PANEL));
    let inner = sidebar.inner(cols[0]);
    frame.render_widget(sidebar, cols[0]);
    let mut items = Vec::new();
    let mut channel_rows = Vec::new();
    items.push(ListItem::new(""));
    channel_rows.push(None);
    for (is_dm, heading) in [(false, "#  Channels"), (true, "◉  Direct messages")] {
        if !app.conversations.iter().any(|c| c.is_im == is_dm) {
            continue;
        }
        if is_dm {
            items.push(ListItem::new(""));
            channel_rows.push(None);
        }
        items.push(ListItem::new(format!("  {heading}")).style(Style::default().fg(MUTED)));
        channel_rows.push(None);
        for (index, conversation) in app
            .conversations
            .iter()
            .enumerate()
            .filter(|(_, c)| c.is_im == is_dm)
        {
            let icon = if conversation.is_im {
                "•"
            } else if conversation.is_private {
                "🔒"
            } else {
                "#"
            };
            let label = format!("    {icon}  {}", safe_text(&conversation.name));
            let active = index == app.channel;
            items.push(
                ListItem::new(ellipsize(&label, inner.width as usize)).style(
                    Style::default()
                        .fg(if active { FG } else { MUTED })
                        .add_modifier(if active {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
            );
            channel_rows.push(Some(index));
        }
    }
    ui.channels
        .select(channel_rows.iter().position(|i| *i == Some(app.sidebar)));
    frame.render_stateful_widget(
        List::new(items).highlight_style(Style::default().bg(SELECTED).fg(FG)),
        inner,
        &mut ui.channels,
    );
    for (row, index) in channel_rows
        .iter()
        .skip(ui.channels.offset())
        .take(inner.height as usize)
        .enumerate()
    {
        if let Some(index) = index {
            ui.hits.push(Hit {
                rect: Rect::new(inner.x, inner.y + row as u16, inner.width, 1),
                action: Action::Channel(*index),
            });
        }
    }
    scrollbar(
        frame,
        cols[0],
        channel_rows.len(),
        ui.channels.offset(),
        inner.height as usize,
    );
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
    frame.render_widget(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(Color::Rgb(66, 45, 74))),
        main[0],
    );
    frame.render_widget(
        Paragraph::new(safe_text(&title))
            .style(Style::default().fg(FG).add_modifier(Modifier::BOLD)),
        Rect::new(main[0].x + 1, main[0].y, main[0].width.saturating_sub(2), 1),
    );
    let title_inner = main[0].inner(ratatui::layout::Margin {
        horizontal: 1,
        vertical: 1,
    });
    toolbar(
        frame,
        ui,
        title_inner,
        &[
            ("Refresh", Action::Refresh),
            ("Thread", Action::Thread),
            ("Media", Action::Media),
            ("Read", Action::Read),
            ("Back", Action::Back),
        ],
    );
    ui.message_area = main[1];
    let width = main[1].width.saturating_sub(4).max(1) as usize;
    let mut heights = Vec::new();
    let mut attachment_rows = Vec::new();
    let mut preview_rows = Vec::new();
    let mut reaction_rows = Vec::new();
    let mut reply_rows = Vec::new();
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
            let mut previews = Vec::new();
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
                if app.graphics
                    && main[1].height >= 10
                    && let Some(image) = app.demo_images.get(&file.name)
                {
                    let (columns, rows) = image.fit(
                        width.saturating_sub(2).min(48) as u16,
                        main[1].height.saturating_sub(7).min(10),
                    );
                    previews.push((
                        file_index,
                        file.name.clone(),
                        lines.len() as u16,
                        columns,
                        rows,
                    ));
                    for _ in 0..rows {
                        lines.push(Line::from(" "));
                    }
                }
            }
            let mut reactions = Vec::new();
            if !m.reactions.is_empty() {
                let row = lines.len() as u16;
                let mut spans = vec![Span::raw(" ")];
                let mut x = 1u16;
                for (index, reaction) in m.reactions.iter().enumerate() {
                    let label = format!(
                        " {} {} ",
                        crate::demo::emoji(&reaction.name),
                        reaction.count
                    );
                    let length = label.width() as u16;
                    reactions.push((index, row, x, length));
                    spans.push(Span::styled(label, Style::default().fg(FG).bg(SELECTED)));
                    spans.push(Span::raw(" "));
                    x = x.saturating_add(length + 1);
                }
                lines.push(Line::from(spans));
            }
            let reply_row = (m.reply_count > 0).then_some(lines.len() as u16);
            if m.reply_count > 0 {
                lines.push(Line::from(Span::styled(
                    format!(" ↳ {} replies", m.reply_count),
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
            preview_rows.push(previews);
            reaction_rows.push(reactions);
            reply_rows.push(reply_row);
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
            .highlight_style(Style::default().bg(Color::Rgb(37, 25, 43)))
            .style(Style::default().fg(FG)),
        main[1],
        &mut ui.messages,
    );
    let content_lines: usize = heights.iter().map(|h| *h as usize).sum();
    let scrolled_lines: usize = heights
        .iter()
        .take(ui.messages.offset())
        .map(|h| *h as usize)
        .sum();
    scrollbar(
        frame,
        main[1],
        content_lines,
        scrolled_lines,
        main[1].height as usize,
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
        for (file_index, name, row, columns, rows) in &preview_rows[index] {
            if row + rows <= visible && ui.inline_images.len() < crate::media::INLINE_SLOTS as usize
            {
                let rect = Rect::new(main[1].x + 1, y + row, *columns, *rows);
                ui.inline_images.push(InlineImage {
                    name: name.clone(),
                    rect,
                    id: crate::media::INLINE_BASE + ui.inline_images.len() as u32,
                });
                ui.hits.push(Hit {
                    rect,
                    action: Action::Attachment(index, *file_index),
                });
            }
        }
        for (reaction, row, x, width) in &reaction_rows[index] {
            if app.demo && *row < visible && *x < main[1].width {
                ui.hits.push(Hit {
                    rect: Rect::new(main[1].x + x, y + row, (*width).min(main[1].width - x), 1),
                    action: Action::Reaction(index, *reaction),
                });
            }
        }
        if let Some(row) = reply_rows[index]
            && row < visible
        {
            ui.hits.push(Hit {
                rect: Rect::new(main[1].x, y + row, main[1].width, 1),
                action: Action::ThreadAt(index),
            });
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
    let hint = if app.sending {
        "Sending…"
    } else if app.mode == Mode::Compose {
        "Enter to send · Alt+Enter for a new line"
    } else {
        "Click the message field to write"
    };
    frame.render_widget(
        Paragraph::new(format!(" {hint}")).style(Style::default().fg(MUTED)),
        main[2],
    );
    let conversation = app
        .conversations
        .get(app.channel)
        .map(|c| c.label())
        .unwrap_or_default();
    let title = if app.thread.is_some() {
        " Reply to thread ".to_owned()
    } else {
        format!(" Message {conversation} ")
    };
    let composer = border(safe_text(&title), app.mode == Mode::Compose);
    let mut composer_inner = composer.inner(main[3]);
    frame.render_widget(composer, main[3]);
    if !app.editor.text.trim().is_empty() {
        let send = Rect::new(
            composer_inner.right().saturating_sub(8),
            composer_inner.bottom().saturating_sub(1),
            8.min(composer_inner.width),
            1,
        );
        button(
            frame,
            ui,
            send,
            if app.sending { "Sending" } else { " Send → " },
            Action::Send,
        );
        composer_inner.height = composer_inner.height.saturating_sub(1);
    }
    ui.hits.push(Hit {
        rect: composer_inner,
        action: Action::Compose,
    });
    let text = if app.editor.text.is_empty() && app.mode != Mode::Compose {
        "Write a message…"
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
        Mode::Compose => "Typing",
        Mode::Switcher => "Find conversation",
        _ => "Browsing",
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
        ui.inline_images.clear();
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
        ui.inline_images.clear();
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
            Action::ThreadAt(index) => {
                app.selected = index;
                app.mode = Mode::Normal;
                app.open_thread();
            }
            Action::Reaction(message, reaction) => app.toggle_demo_reaction(message, reaction),
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
