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
const CHAT_BG: Color = Color::Rgb(43, 51, 65);
const CHAT_FG: Color = Color::Rgb(200, 211, 239);
const CHAT_MUTED: Color = Color::Rgb(165, 175, 201);
const CHAT_CHIP: Color = Color::Rgb(57, 67, 85);

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
    ThreadMessage(usize),
    ThreadAttachment(usize, usize),
    ThreadReaction(usize, usize),
    ThreadCompose,
    ThreadSend,
    ThreadRead,
    ThreadRefresh,
    ThreadMedia,
    CloseThread,
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
#[derive(Clone, Copy)]
enum Divider {
    Sidebar,
    Thread,
}
#[derive(Default)]
pub struct UiState {
    pub hits: Vec<Hit>,
    pub inline_images: Vec<InlineImage>,
    pub message_area: Rect,
    pub thread_area: Rect,
    pub people_area: Rect,
    pub sidebar_divider: Rect,
    pub thread_divider: Rect,
    pub visible_people: Vec<String>,
    pub visible_avatars: Vec<String>,
    bounds: Rect,
    dragging: Option<Divider>,
    manual_sidebar_scroll: bool,
    manual_people_scroll: bool,
    pub sidebar_area: Rect,
    pub media_area: Rect,
    pub read_scroll_max: usize,
    channels: ListState,
    messages: ListState,
    thread_messages: ListState,
    people: ListState,
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

fn draw_sidebar(frame: &mut Frame, app: &App, ui: &mut UiState, area: Rect) {
    use crate::api::Presence;
    ui.sidebar_area = area;
    ui.sidebar_divider = Rect::new(area.right().saturating_sub(1), area.y, 1, area.height);
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(Color::Rgb(79, 53, 90)))
        .style(Style::default().bg(PANEL));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let channel_ids: Vec<_> = app
        .conversations
        .iter()
        .enumerate()
        .filter(|(_, c)| !c.is_im)
        .map(|(i, _)| i)
        .collect();
    let people_ids: Vec<_> = app
        .conversations
        .iter()
        .enumerate()
        .filter(|(_, c)| c.is_im)
        .map(|(i, _)| i)
        .collect();
    let people_height = if people_ids.is_empty() {
        0
    } else {
        (inner.height / 3)
            .clamp(5, 12)
            .min(inner.height.saturating_sub(3))
    };
    let channels = Rect::new(inner.x, inner.y, inner.width, inner.height - people_height);
    let people = Rect::new(inner.x, channels.bottom(), inner.width, people_height);
    frame.render_widget(
        Paragraph::new(" #  Channels").style(Style::default().fg(ACCENT)),
        Rect::new(
            channels.x,
            channels.y + u16::from(channels.height > 1),
            channels.width,
            1.min(channels.height),
        ),
    );
    let body = Rect::new(
        channels.x,
        channels.y + 2.min(channels.height),
        channels.width,
        channels.height.saturating_sub(2),
    );
    let items: Vec<_> = channel_ids
        .iter()
        .map(|index| {
            let c = &app.conversations[*index];
            let unread = app.unread_counts.get(&c.id).copied().unwrap_or(0) > 0;
            let active = *index == app.channel;
            let icon = if c.is_private {
                "🔒"
            } else if c.is_shared {
                "◆"
            } else {
                "#"
            };
            let prefix = format!("  {icon} ");
            let width = body
                .width
                .saturating_sub(prefix.width() as u16 + if unread { 3 } else { 1 });
            let name = ellipsize(&safe_text(&c.name), width as usize);
            let mut spans = vec![
                Span::styled(
                    prefix,
                    Style::default().fg(if c.is_shared {
                        Color::Rgb(235, 192, 119)
                    } else {
                        MUTED
                    }),
                ),
                Span::styled(
                    name,
                    Style::default()
                        .fg(if unread || active { FG } else { MUTED })
                        .add_modifier(if unread || active {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
            ];
            if unread {
                spans.push(Span::styled(
                    " ●",
                    Style::default().fg(Color::Rgb(112, 195, 211)),
                ));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();
    let selected = if app.focus == Focus::Channels {
        Some(app.sidebar)
    } else if app.conversations.get(app.channel).is_some_and(|c| !c.is_im) {
        Some(app.channel)
    } else {
        None
    };
    let manual_offset = ui.channels.offset();
    ui.channels.select(if ui.manual_sidebar_scroll {
        None
    } else {
        selected.and_then(|i| channel_ids.iter().position(|index| *index == i))
    });
    if ui.manual_sidebar_scroll {
        *ui.channels.offset_mut() = manual_offset;
    }
    frame.render_stateful_widget(
        List::new(items).highlight_style(Style::default().bg(SELECTED)),
        body,
        &mut ui.channels,
    );
    for (row, index) in channel_ids
        .iter()
        .skip(ui.channels.offset())
        .take(body.height as usize)
        .enumerate()
    {
        ui.hits.push(Hit {
            rect: Rect::new(body.x, body.y + row as u16, body.width, 1),
            action: Action::Channel(*index),
        });
    }
    scrollbar(
        frame,
        channels,
        channel_ids.len(),
        ui.channels.offset(),
        body.height as usize,
    );
    ui.people_area = people;
    if people_height == 0 {
        return;
    }
    frame.render_widget(
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(Color::Rgb(66, 45, 74))),
        people,
    );
    frame.render_widget(
        Paragraph::new(" ◉  People").style(Style::default().fg(ACCENT)),
        Rect::new(
            people.x,
            people.y + 1.min(people.height),
            people.width,
            1.min(people.height),
        ),
    );
    let body = Rect::new(
        people.x,
        people.y + 2.min(people.height),
        people.width,
        people.height.saturating_sub(2),
    );
    let items: Vec<_> = people_ids
        .iter()
        .map(|index| {
            let c = &app.conversations[*index];
            let (symbol, color) = match app.presences.get(&c.user).copied().unwrap_or_default() {
                Presence::Active => ("●", GREEN),
                Presence::Away => ("○", MUTED),
                Presence::Unknown => ("·", Color::Rgb(112, 94, 122)),
                Presence::DoNotDisturb => ("⊘", Color::Rgb(235, 192, 119)),
            };
            let person = app.people.get(&c.user);
            let name = person
                .filter(|p| !p.display_name.is_empty())
                .map_or(c.name.as_str(), |p| p.display_name.as_str());
            let status = person
                .filter(|p| !p.status_emoji.is_empty())
                .map(|p| {
                    let text = &p.status_emoji;
                    if text.starts_with(':') {
                        {
                            let emoji = crate::demo::emoji(text.trim_matches(':'));
                            if emoji.starts_with(':') {
                                "💬".to_string()
                            } else {
                                emoji
                            }
                        }
                    } else {
                        safe_text(text)
                    }
                })
                .unwrap_or_default();
            let unread = app.unread_counts.get(&c.id).copied().unwrap_or(0) > 0;
            let suffix = format!(
                "{}{}",
                if status.is_empty() {
                    String::new()
                } else {
                    format!(" {status}")
                },
                if unread { " ●" } else { "" }
            );
            let name = ellipsize(
                &safe_text(name),
                body.width.saturating_sub(4 + suffix.width() as u16) as usize,
            );
            ListItem::new(Line::from(vec![
                Span::styled(format!(" {symbol} "), Style::default().fg(color)),
                Span::styled(
                    name,
                    Style::default()
                        .fg(if unread || *index == app.channel {
                            FG
                        } else {
                            MUTED
                        })
                        .add_modifier(if unread || *index == app.channel {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
                Span::styled(suffix, Style::default().fg(ACCENT)),
            ]))
        })
        .collect();
    let selected = if app.focus == Focus::People {
        Some(app.person)
    } else if app.conversations.get(app.channel).is_some_and(|c| c.is_im) {
        Some(app.channel)
    } else {
        None
    };
    let manual_offset = ui.people.offset();
    ui.people.select(if ui.manual_people_scroll {
        None
    } else {
        selected.and_then(|i| people_ids.iter().position(|index| *index == i))
    });
    if ui.manual_people_scroll {
        *ui.people.offset_mut() = manual_offset;
    }
    frame.render_stateful_widget(
        List::new(items).highlight_style(Style::default().bg(SELECTED)),
        body,
        &mut ui.people,
    );
    for (row, index) in people_ids
        .iter()
        .skip(ui.people.offset())
        .take(body.height as usize)
        .enumerate()
    {
        ui.hits.push(Hit {
            rect: Rect::new(body.x, body.y + row as u16, body.width, 1),
            action: Action::Channel(*index),
        });
        let id = app.conversations[*index].user.clone();
        if !id.is_empty() {
            ui.visible_people.push(id);
        }
    }
    scrollbar(
        frame,
        people,
        people_ids.len(),
        ui.people.offset(),
        body.height as usize,
    );
}

fn thread_action(action: Action) -> Action {
    match action {
        Action::Message(i) | Action::ThreadAt(i) => Action::ThreadMessage(i),
        Action::Attachment(m, a) => Action::ThreadAttachment(m, a),
        Action::Reaction(m, r) => Action::ThreadReaction(m, r),
        Action::Compose => Action::ThreadCompose,
        Action::Send => Action::ThreadSend,
        Action::Read => Action::ThreadRead,
        Action::Refresh => Action::ThreadRefresh,
        Action::Media => Action::ThreadMedia,
        other => other,
    }
}

fn message_time(ts: &str) -> Option<chrono::DateTime<chrono::Local>> {
    chrono::DateTime::from_timestamp(ts.split('.').next()?.parse().ok()?, 0)
        .map(|time| time.with_timezone(&chrono::Local))
}

fn date_label(date: chrono::NaiveDate) -> String {
    let today = chrono::Local::now().date_naive();
    if date == today {
        "Today".into()
    } else if Some(date) == today.pred_opt() {
        "Yesterday".into()
    } else {
        date.format("%A, %B %-d, %Y").to_string()
    }
}

fn draw_feed(frame: &mut Frame, app: &App, ui: &mut UiState, area: Rect, thread: bool) {
    let hit_start = ui.hits.len();
    let (messages, selected, editor, loading, sending) = if thread {
        let Some(view) = app.thread_view.as_ref() else {
            return;
        };
        (
            &view.messages,
            view.selected,
            &view.editor,
            view.loading,
            view.sending,
        )
    } else {
        (
            &app.messages,
            app.selected,
            &app.editor,
            app.loading,
            app.sending,
        )
    };
    let composing = app.mode == Mode::Compose
        && if thread {
            app.focus == Focus::Thread
        } else {
            app.focus == Focus::Messages
        };
    let main = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(5),
    ])
    .split(area);
    let title = app
        .conversations
        .get(app.channel)
        .map(|c| c.label())
        .unwrap_or_else(|| "Getting started".into());
    let title = if thread {
        " Thread ".to_owned()
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
    if thread {
        toolbar(
            frame,
            ui,
            title_inner,
            &[
                ("Refresh", Action::Refresh),
                ("Media", Action::Media),
                ("Read", Action::Read),
            ],
        );
        button(
            frame,
            ui,
            Rect::new(area.right().saturating_sub(9), area.y, 9.min(area.width), 1),
            "× Close",
            Action::CloseThread,
        );
    } else {
        toolbar(
            frame,
            ui,
            title_inner,
            &[
                ("Refresh", Action::Refresh),
                ("Thread", Action::Thread),
                ("Media", Action::Media),
                ("Read", Action::Read),
            ],
        );
    }
    if thread {
        ui.thread_area = main[1];
    } else {
        ui.message_area = main[1];
    }
    let gutter = if main[1].width >= 24 { 7 } else { 1 };
    let width = main[1].width.saturating_sub(gutter + 2).max(1) as usize;
    let mut heights = Vec::new();
    let mut attachment_rows = Vec::new();
    let mut preview_rows = Vec::new();
    let mut reaction_rows = Vec::new();
    let mut reply_rows = Vec::new();
    let mut date_rows = Vec::new();
    let mut previous_date = None;
    let items: Vec<_> = messages
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
            let local = message_time(&m.ts);
            let date = local.map(|time| time.date_naive());
            let separator = date.filter(|date| Some(*date) != previous_date);
            previous_date = date;
            let time = local
                .map(|time| time.format("%-I:%M %p").to_string())
                .unwrap_or_default();
            let mut lines = vec![Line::from(vec![
                Span::styled(
                    format!("{}  ", safe_text(&author)),
                    Style::default().fg(CHAT_MUTED).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    time,
                    Style::default()
                        .fg(CHAT_MUTED)
                        .add_modifier(Modifier::ITALIC),
                ),
            ])];
            let text = display_text(app, &m.text);
            for paragraph in text.split('\n') {
                for line in textwrap::wrap(paragraph, width) {
                    lines.push(Line::from(line.into_owned()));
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
                        "{} {}",
                        if file.is_video() {
                            "▶"
                        } else if file.is_media() {
                            "▧"
                        } else {
                            "↳"
                        },
                        safe_text(&file.name)
                    ),
                    Style::default()
                        .fg(CHAT_MUTED)
                        .add_modifier(Modifier::UNDERLINED),
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
            let reply_row = (m.reply_count > 0).then_some(lines.len() as u16);
            if m.reply_count > 0 {
                lines.push(Line::from(Span::styled(
                    format!(
                        "[{} {} →]",
                        m.reply_count,
                        if m.reply_count == 1 {
                            "reply"
                        } else {
                            "replies"
                        }
                    ),
                    Style::default()
                        .fg(CHAT_MUTED)
                        .add_modifier(Modifier::ITALIC),
                )));
            }
            let mut reactions = Vec::new();
            if !m.reactions.is_empty() {
                let row = lines.len() as u16;
                let mut spans = vec![];
                let mut x = 0u16;
                for (index, reaction) in m.reactions.iter().enumerate() {
                    let label = format!(
                        " {} {} ",
                        crate::demo::emoji(&reaction.name),
                        reaction.count
                    );
                    let length = label.width() as u16;
                    reactions.push((index, row, x, length));
                    spans.push(Span::styled(
                        label,
                        Style::default().fg(CHAT_MUTED).bg(CHAT_CHIP),
                    ));
                    spans.push(Span::raw(" "));
                    x = x.saturating_add(length + 1);
                }
                lines.push(Line::from(spans));
            }
            lines.push(Line::from(""));
            for line in &mut lines {
                line.spans.insert(0, Span::raw(" ".repeat(gutter as usize)));
            }
            let date_height = u16::from(separator.is_some());
            if let Some(date) = separator {
                lines.insert(
                    0,
                    Line::from(Span::styled(
                        format!("─  {}  ─", date_label(date)),
                        Style::default().fg(CHAT_MUTED).add_modifier(Modifier::BOLD),
                    ))
                    .centered(),
                );
            }
            // Keep a single long message navigable without forcing other rows offscreen.
            let max_lines = main[1].height.max(1) as usize;
            if lines.len() > max_lines {
                lines.truncate(max_lines);
                if let Some(last) = lines.last_mut() {
                    *last = Line::from(Span::styled(
                        format!(
                            "{}… press o to read the full message",
                            " ".repeat(gutter as usize)
                        ),
                        Style::default().fg(CHAT_MUTED),
                    ));
                }
            }
            attachment_rows.push(
                file_rows
                    .into_iter()
                    .map(|(i, r)| (i, r + date_height))
                    .collect::<Vec<_>>(),
            );
            preview_rows.push(
                previews
                    .into_iter()
                    .map(|(i, n, r, c, h)| (i, n, r + date_height, c, h))
                    .collect::<Vec<_>>(),
            );
            reaction_rows.push(
                reactions
                    .into_iter()
                    .map(|(i, r, x, w)| (i, r + date_height, x + gutter, w))
                    .collect::<Vec<_>>(),
            );
            reply_rows.push(reply_row.map(|r| r + date_height));
            date_rows.push(date_height);
            heights.push(lines.len() as u16);
            ListItem::new(Text::from(lines))
        })
        .collect();
    let state = if thread {
        &mut ui.thread_messages
    } else {
        &mut ui.messages
    };
    state.select(if messages.is_empty() {
        None
    } else {
        Some(selected)
    });
    frame.render_stateful_widget(
        List::new(items)
            .highlight_style(Style::default().bg(Color::Rgb(47, 57, 73)))
            .style(Style::default().fg(CHAT_FG).bg(CHAT_BG)),
        main[1],
        state,
    );
    let offset = state.offset();
    let content_lines: usize = heights.iter().map(|h| *h as usize).sum();
    let scrolled_lines: usize = heights.iter().take(offset).map(|h| *h as usize).sum();
    scrollbar(
        frame,
        main[1],
        content_lines,
        scrolled_lines,
        main[1].height as usize,
    );
    let mut y = main[1].y;
    for (index, height) in heights.iter().enumerate().skip(offset) {
        let visible = (*height).min(main[1].bottom().saturating_sub(y));
        if visible == 0 {
            break;
        }
        ui.hits.push(Hit {
            rect: Rect::new(main[1].x, y, main[1].width, visible),
            action: Action::Message(index),
        });
        if gutter > 1 && visible > date_rows[index] {
            ui.visible_avatars.push(messages[index].user.clone());
            let author = app
                .users
                .get(&messages[index].user)
                .map(String::as_str)
                .unwrap_or(&messages[index].username);
            let initials: String = author
                .split_whitespace()
                .take(2)
                .filter_map(|part| part.chars().next())
                .flat_map(char::to_uppercase)
                .collect();
            let avatar = Rect::new(
                main[1].x + 1,
                y + date_rows[index],
                4,
                2.min(visible - date_rows[index]),
            );
            frame.render_widget(
                Paragraph::new(format!(
                    " {}",
                    if initials.is_empty() { "?" } else { &initials }
                ))
                .style(
                    Style::default()
                        .fg(CHAT_MUTED)
                        .bg(CHAT_CHIP)
                        .add_modifier(Modifier::BOLD),
                ),
                avatar,
            );
            if let Some(pixels) = app.avatars.get(&messages[index].user) {
                if app.graphics
                    && avatar.height == 2
                    && ui.inline_images.len() < crate::media::INLINE_SLOTS as usize
                {
                    frame.render_widget(ratatui::widgets::Clear, avatar);
                    ui.inline_images.push(InlineImage {
                        name: format!("avatar:{}", messages[index].user),
                        rect: avatar,
                        id: crate::media::INLINE_BASE + ui.inline_images.len() as u32,
                    });
                } else {
                    // Half-block pixels keep profile pictures available without Kitty graphics.
                    let image =
                        image::RgbImage::from_raw(pixels.width, pixels.height, pixels.rgb.clone());
                    if let Some(image) = image {
                        let image = image::imageops::resize(
                            &image,
                            4,
                            4,
                            image::imageops::FilterType::Triangle,
                        );
                        for row in 0..avatar.height {
                            for col in 0..4 {
                                let top = image.get_pixel(col, u32::from(row) * 2).0;
                                let bottom = image.get_pixel(col, u32::from(row) * 2 + 1).0;
                                frame.buffer_mut()[(avatar.x + col as u16, avatar.y + row)]
                                    .set_symbol("▀")
                                    .set_fg(Color::Rgb(top[0], top[1], top[2]))
                                    .set_bg(Color::Rgb(bottom[0], bottom[1], bottom[2]));
                            }
                        }
                    }
                }
            }
        }
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
                let rect = Rect::new(main[1].x + gutter, y + row, *columns, *rows);
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
    if messages.is_empty() {
        frame.render_widget(
            Paragraph::new(if loading {
                "Loading conversation…"
            } else {
                "No messages to display."
            })
            .style(Style::default().fg(MUTED))
            .centered(),
            main[1],
        );
    }
    let hint = if sending {
        "Sending…"
    } else if composing {
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
    let title = if thread {
        " Reply to thread ".to_owned()
    } else {
        format!(" Message {conversation} ")
    };
    let composer = border(safe_text(&title), composing);
    let mut composer_inner = composer.inner(main[3]);
    frame.render_widget(composer, main[3]);
    if !editor.text.trim().is_empty() {
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
            if sending { "Sending" } else { " Send → " },
            Action::Send,
        );
        composer_inner.height = composer_inner.height.saturating_sub(1);
    }
    ui.hits.push(Hit {
        rect: composer_inner,
        action: Action::Compose,
    });
    let text = if editor.text.is_empty() && !composing {
        "Write a message…"
    } else {
        &editor.text
    };
    let before = &editor.text[..editor.cursor];
    let line_count = before.lines().count().max(1) + usize::from(before.ends_with('\n'));
    let scroll = line_count.saturating_sub(composer_inner.height as usize) as u16;
    let cursor_column = before.rsplit('\n').next().unwrap_or("").width() as u16;
    let horizontal = cursor_column.saturating_sub(composer_inner.width.saturating_sub(1));
    frame.render_widget(
        Paragraph::new(text)
            .scroll((scroll, horizontal))
            .style(Style::default().fg(if composing { FG } else { MUTED })),
        composer_inner,
    );
    if composing && !composer_inner.is_empty() {
        let x = before.rsplit('\n').next().unwrap_or("").width() as u16;
        let y = (line_count as u16).saturating_sub(1).saturating_sub(scroll);
        frame.set_cursor_position((
            composer_inner.x
                + x.saturating_sub(horizontal)
                    .min(composer_inner.width.saturating_sub(1)),
            composer_inner.y + y.min(composer_inner.height.saturating_sub(1)),
        ));
    }
    if thread {
        for hit in &mut ui.hits[hit_start..] {
            hit.action = thread_action(hit.action);
        }
    }
}

pub fn draw(frame: &mut Frame, app: &App, ui: &mut UiState) {
    let area = frame.area();
    ui.bounds = area;
    ui.thread_area = Rect::default();
    ui.thread_divider = Rect::default();
    ui.sidebar_divider = Rect::default();
    ui.visible_people.clear();
    ui.visible_avatars.clear();
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
        let (messages, selected) = app.active_messages();
        if let Some(message) = messages.get(selected) {
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
        let active_scroll = if app.focus == Focus::Thread {
            app.thread_view.as_ref().map_or(0, |view| view.read_scroll)
        } else {
            app.read_scroll
        };
        let scroll = active_scroll.min(ui.read_scroll_max);
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
                    action: if app.focus == Focus::Thread {
                        Action::ThreadAttachment(selected, index)
                    } else {
                        Action::Attachment(selected, index)
                    },
                });
            }
        }
        if app.focus == Focus::Thread {
            for hit in &mut ui.hits {
                hit.action = thread_action(hit.action);
            }
            ui.thread_area = chunks[0];
        } else {
            ui.message_area = chunks[0];
        }
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
    let min_feed = if area.width >= 70 { 24 } else { 12 };
    let min_sidebar = if area.width >= 70 { 16 } else { 12 };
    let reserve = if app.thread_view.is_some() {
        min_feed * 2 + 1
    } else {
        min_feed
    };
    let preferred = app.sidebar_width.unwrap_or((area.width / 4).clamp(22, 34));
    let sidebar_width = preferred.clamp(
        min_sidebar,
        area.width.saturating_sub(reserve).max(min_sidebar),
    );
    let cols = [
        Rect::new(rows[1].x, rows[1].y, sidebar_width, rows[1].height),
        Rect::new(
            rows[1].x + sidebar_width,
            rows[1].y,
            rows[1].width - sidebar_width,
            rows[1].height,
        ),
    ];
    draw_sidebar(frame, app, ui, cols[0]);
    let content = cols[1];
    if app.thread_view.is_some() && content.width >= 25 {
        let minimum = if content.width >= 49 { 24 } else { 12 };
        let width = app.thread_width.unwrap_or(content.width / 2).clamp(
            minimum,
            content.width.saturating_sub(minimum + 1).max(minimum),
        );
        let divider = Rect::new(content.right() - width - 1, content.y, 1, content.height);
        let channel = Rect::new(content.x, content.y, divider.x - content.x, content.height);
        let replies = Rect::new(divider.right(), content.y, width, content.height);
        ui.thread_divider = divider;
        frame.render_widget(
            Block::default()
                .borders(Borders::LEFT)
                .border_style(Style::default().fg(ACCENT)),
            divider,
        );
        draw_feed(frame, app, ui, channel, false);
        draw_feed(frame, app, ui, replies, true);
    } else {
        draw_feed(frame, app, ui, content, false);
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
        let help = "NAVIGATE\n j / k, ↑ / ↓    Select a message or conversation\n h / l, Tab      Cycle sidebar / channel / thread panes\n Enter           Open conversation or selected thread\n Ctrl+K          Find a conversation (fuzzy search)\n g / G           First / last item\n PgUp / PgDown   More history / move five messages\n\nWRITE\n i               Compose   ·   Esc returns to navigation\n Enter           Send      ·   Alt+Enter adds a newline\n\nREAD & MEDIA\n o               Read full message (j/k or wheel scroll)\n t               Open thread in the right pane\n v               View image / video attachment\n Space           Pause / resume video (silent playback)\n Esc             Close media, thread, or popup\n R               Refresh / retry connection\n q / Ctrl+C      Quit (q types normally while composing)\n\nMOUSE  Click channels, messages, composer, and buttons.\n       Wheel scrolls under the pointer; typing focus stays put.\n       Drag pane dividers to resize.\nPEOPLE ● Online · ○ Away · ⊘ Do not disturb · · Unknown";
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

pub fn key(app: &mut App, ui: &mut UiState, event: KeyEvent) {
    let switching = app.mode == Mode::Switcher && event.code == KeyCode::Enter;
    app.key(event);
    if switching {
        ui.manual_sidebar_scroll = false;
        ui.manual_people_scroll = false;
    }
    if app.mode == Mode::Normal {
        if app.focus == Focus::Channels {
            ui.manual_sidebar_scroll = false;
        }
        if app.focus == Focus::People {
            ui.manual_people_scroll = false;
        }
    }
}

pub fn mouse(app: &mut App, ui: &mut UiState, event: MouseEvent) {
    let point = Position::new(event.column, event.row);
    let modal =
        app.viewer.is_some() || matches!(app.mode, Mode::Help | Mode::Switcher | Mode::Read);
    if event.kind == MouseEventKind::Up(MouseButton::Left) {
        ui.dragging = None;
        return;
    }
    if !modal
        && event.kind == MouseEventKind::Drag(MouseButton::Left)
        && let Some(divider) = ui.dragging
    {
        match divider {
            Divider::Sidebar => {
                app.sidebar_width = Some(event.column.saturating_sub(ui.bounds.x).saturating_add(1))
            }
            Divider::Thread => {
                app.thread_width = Some(
                    ui.bounds
                        .right()
                        .saturating_sub(event.column.saturating_add(1)),
                )
            }
        }
        return;
    }
    if !modal && event.kind == MouseEventKind::Down(MouseButton::Left) {
        if ui.sidebar_divider.contains(point) {
            ui.dragging = Some(Divider::Sidebar);
            return;
        }
        if ui.thread_divider.contains(point) {
            ui.dragging = Some(Divider::Thread);
            return;
        }
    }
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
        let compose_focus = (app.mode == Mode::Compose).then_some(app.focus);
        if app.mode == Mode::Read {
            let scroll = if app.focus == Focus::Thread {
                if let Some(view) = &mut app.thread_view {
                    &mut view.read_scroll
                } else {
                    &mut app.read_scroll
                }
            } else {
                &mut app.read_scroll
            };
            *scroll = (*scroll)
                .min(ui.read_scroll_max)
                .saturating_add_signed(delta)
                .min(ui.read_scroll_max);
        } else if app.mode == Mode::Switcher {
            app.switch_selected = app
                .switch_selected
                .saturating_add_signed(delta)
                .min(app.filtered().len().saturating_sub(1));
        } else if ui.thread_area.contains(point) {
            app.focus_thread();
            app.move_selection(delta);
        } else if ui.people_area.contains(point) {
            if compose_focus.is_some() {
                ui.manual_people_scroll = true;
                let maximum = app
                    .conversations
                    .iter()
                    .filter(|c| c.is_im)
                    .count()
                    .saturating_sub(ui.people_area.height.saturating_sub(2) as usize);
                *ui.people.offset_mut() =
                    ui.people.offset().saturating_add_signed(delta).min(maximum);
            } else {
                app.focus = Focus::People;
                app.move_selection(delta);
            }
        } else if ui.sidebar_area.contains(point) {
            if compose_focus.is_some() {
                ui.manual_sidebar_scroll = true;
                let maximum = app
                    .conversations
                    .iter()
                    .filter(|c| !c.is_im)
                    .count()
                    .saturating_sub(
                        ui.sidebar_area
                            .height
                            .saturating_sub(ui.people_area.height + 2)
                            as usize,
                    );
                *ui.channels.offset_mut() = ui
                    .channels
                    .offset()
                    .saturating_add_signed(delta)
                    .min(maximum);
            } else {
                app.focus = Focus::Channels;
                app.move_selection(delta);
            }
        } else if ui.message_area.contains(point) {
            app.focus_channel();
            app.move_selection(delta);
        }
        if let Some(focus) = compose_focus {
            app.focus = focus;
        }
        return;
    }
    if event.kind != MouseEventKind::Down(MouseButton::Left) {
        return;
    }
    if let Some(hit) = ui.hits.iter().rev().find(|h| h.rect.contains(point)) {
        let preview_focus = (app.mode == Mode::Compose
            && matches!(
                hit.action,
                Action::Attachment(..)
                    | Action::ThreadAttachment(..)
                    | Action::Media
                    | Action::ThreadMedia
            ))
        .then_some(app.focus);
        match hit.action {
            Action::Channel(index) | Action::SwitchResult(index) => {
                ui.manual_sidebar_scroll = false;
                ui.manual_people_scroll = false;
                app.open_channel(index);
            }
            Action::Message(index) => {
                app.focus_channel();
                app.selected = index;
                app.mode = Mode::Normal;
            }
            Action::ThreadMessage(index) => {
                app.focus_thread();
                if let Some(view) = &mut app.thread_view {
                    view.selected = index;
                }
                app.mode = Mode::Normal;
            }
            Action::ThreadAt(index) => {
                app.focus_channel();
                app.selected = index;
                app.open_thread();
            }
            Action::Reaction(message, reaction) | Action::ThreadReaction(message, reaction) => {
                let focus = app.focus;
                if matches!(hit.action, Action::ThreadReaction(..)) {
                    app.focus_thread();
                } else {
                    app.focus_channel();
                }
                app.toggle_demo_reaction(message, reaction);
                app.focus = focus;
            }
            Action::Read | Action::ThreadRead => {
                if hit.action == Action::ThreadRead {
                    app.focus_thread();
                    if let Some(view) = &mut app.thread_view {
                        view.read_scroll = 0;
                    }
                } else {
                    app.focus_channel();
                    app.read_scroll = 0;
                }
                app.mode = Mode::Read;
            }
            Action::Attachment(message, index) => {
                app.focus_channel();
                app.selected = message;
                app.open_attachment(index);
            }
            Action::ThreadAttachment(message, index) => {
                app.focus_thread();
                if let Some(view) = &mut app.thread_view {
                    view.selected = message;
                }
                app.open_attachment(index);
            }
            Action::Compose => app.start_composing(false),
            Action::ThreadCompose => app.start_composing(true),
            Action::Send => {
                app.start_composing(false);
                app.send();
            }
            Action::ThreadSend => {
                app.start_composing(true);
                app.send();
            }
            Action::Thread => {
                app.focus_channel();
                app.open_thread();
            }
            Action::Media => {
                app.focus_channel();
                app.open_media();
            }
            Action::ThreadMedia => {
                app.focus_thread();
                app.open_media();
            }
            Action::Switch => app.key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL)),
            Action::Help => app.mode = Mode::Help,
            Action::Refresh => {
                app.refresh_channel(false);
            }
            Action::ThreadRefresh => app.refresh_thread(false),
            Action::Back | Action::CloseThread => app.close_thread(),
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
        if let Some(focus) = preview_focus {
            app.focus = focus;
        }
    }
}
