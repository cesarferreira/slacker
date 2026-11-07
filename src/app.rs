use crate::{
    api::{Conversation, History, Message, Slack, Snapshot},
    media::{Prepared, Video},
};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::{
    collections::HashMap,
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

#[derive(Default, Clone)]
pub struct Editor {
    pub text: String,
    pub cursor: usize,
}
impl Editor {
    pub fn insert(&mut self, s: &str) {
        self.text.insert_str(self.cursor, s);
        self.cursor += s.len();
    }
    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let start = self.text[..self.cursor]
                .char_indices()
                .last()
                .map_or(0, |(i, _)| i);
            self.text.drain(start..self.cursor);
            self.cursor = start;
        }
    }
    pub fn left(&mut self) {
        if self.cursor > 0 {
            self.cursor = self.text[..self.cursor]
                .char_indices()
                .last()
                .map_or(0, |(i, _)| i);
        }
    }
    pub fn right(&mut self) {
        if let Some(c) = self.text[self.cursor..].chars().next() {
            self.cursor += c.len_utf8();
        }
    }
    pub fn delete(&mut self) {
        if let Some(c) = self.text[self.cursor..].chars().next() {
            self.text.drain(self.cursor..self.cursor + c.len_utf8());
        }
    }
    pub fn key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete(),
            KeyCode::Left => self.left(),
            KeyCode::Right => self.right(),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.text.len(),
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.insert(&c.to_string())
            }
            _ => {}
        }
    }
}

pub fn safe_text(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect::<String>()
        .replace('\t', "    ")
}

pub fn fuzzy_score(needle: &str, haystack: &str) -> Option<usize> {
    let needle = needle.to_lowercase();
    let haystack = haystack.to_lowercase();
    let mut position = 0;
    let mut score = 0;
    for c in needle.chars() {
        let skip = haystack[position..].find(c)?;
        score += skip;
        position += skip + c.len_utf8();
    }
    Some(score)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Compose,
    Switcher,
    Help,
    Read,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Channels,
    Messages,
}

pub enum Job {
    Bootstrap,
    History {
        id: u64,
        channel: String,
        thread: Option<String>,
        cursor: String,
        oldest: Option<String>,
    },
    Send {
        channel: String,
        thread: Option<String>,
        text: String,
    },
}

pub enum Reply {
    Bootstrap(Result<Snapshot>),
    History {
        id: u64,
        append: bool,
        result: Result<History>,
    },
    Sent {
        channel: String,
        thread: Option<String>,
        text: String,
        result: Result<Message>,
    },
    Media {
        id: u64,
        result: Result<Prepared>,
    },
}

pub struct Worker {
    pub jobs: Sender<Job>,
    pub replies: Receiver<Reply>,
    pub events: Sender<Reply>,
    pub slack: Slack,
}
impl Worker {
    pub fn new(slack: Slack) -> Self {
        let (jobs, rx) = mpsc::channel();
        let (events, replies) = mpsc::channel();
        let client = slack.clone();
        let tx = events.clone();
        thread::spawn(move || {
            for job in rx {
                let reply = match job {
                    Job::Bootstrap => Reply::Bootstrap(client.bootstrap()),
                    Job::History {
                        id,
                        channel,
                        thread,
                        cursor,
                        oldest,
                    } => Reply::History {
                        id,
                        append: thread.is_some() || !cursor.is_empty(),
                        result: client.history(
                            &channel,
                            thread.as_deref(),
                            &cursor,
                            oldest.as_deref(),
                        ),
                    },
                    Job::Send {
                        channel,
                        thread,
                        text,
                    } => {
                        let result = client.send(&channel, thread.as_deref(), &text);
                        Reply::Sent {
                            channel,
                            thread,
                            text,
                            result,
                        }
                    }
                };
                if tx.send(reply).is_err() {
                    break;
                }
            }
        });
        Self {
            jobs,
            replies,
            events,
            slack,
        }
    }
}

pub struct Viewer {
    pub prepared: Prepared,
    pub video: Option<Video>,
    pub last: Option<crate::media::Pixels>,
    pub dirty: bool,
    pub note: String,
}
impl Viewer {
    pub fn new(mut prepared: Prepared) -> Result<Self> {
        let video = if prepared.video {
            Some(Video::start(&prepared.path)?)
        } else {
            None
        };
        let last = prepared.image.take();
        Ok(Self {
            prepared,
            video,
            last,
            dirty: true,
            note: "Decoding media…".into(),
        })
    }
}

pub struct App {
    pub notifications: crate::notifications::Notifications,
    pub demo_images: HashMap<String, crate::media::Pixels>,
    demo_saved: HashMap<(String, Option<String>), Vec<Message>>,
    pub workspace: String,
    pub me: String,
    pub users: HashMap<String, String>,
    pub conversations: Vec<Conversation>,
    pub channel: usize,
    pub sidebar: usize,
    pub messages: Vec<Message>,
    pub selected: usize,
    pub read_scroll: usize,
    pub thread: Option<String>,
    pub thread_latest: Option<String>,
    pub mode: Mode,
    pub focus: Focus,
    pub editor: Editor,
    pub query: Editor,
    pub switch_selected: usize,
    pub status: String,
    pub loading: bool,
    pub sending: bool,
    pub cursor: String,
    pub generation: u64,
    pub media_generation: u64,
    pub viewer: Option<Viewer>,
    pub graphics: bool,
    pub demo: bool,
    pub quit: bool,
    pub drafts: HashMap<(String, Option<String>), Editor>,
    pub worker: Option<Worker>,
}

impl App {
    pub fn new(worker: Option<Worker>, graphics: bool) -> Self {
        Self {
            notifications: crate::notifications::Notifications::default(),
            demo_images: HashMap::new(),
            demo_saved: HashMap::new(),
            workspace: "slacker".into(),
            me: "you".into(),
            users: HashMap::new(),
            conversations: Vec::new(),
            channel: 0,
            sidebar: 0,
            messages: Vec::new(),
            selected: 0,
            read_scroll: 0,
            thread: None,
            thread_latest: None,
            mode: Mode::Normal,
            focus: Focus::Messages,
            editor: Editor::default(),
            query: Editor::default(),
            switch_selected: 0,
            status: "Connecting to Slack…".into(),
            loading: worker.is_some(),
            sending: false,
            cursor: String::new(),
            generation: 0,
            media_generation: 0,
            viewer: None,
            graphics,
            demo: worker.is_none(),
            quit: false,
            drafts: HashMap::new(),
            worker,
        }
    }
    pub fn demo(graphics: bool) -> Self {
        let mut app = Self::new(None, graphics);
        app.workspace = "slacker / demo".into();
        app.users = HashMap::from([
            ("maya".into(), "Maya Chen".into()),
            ("leo".into(), "Leo Rivera".into()),
            ("you".into(), "You".into()),
        ]);
        app.conversations = [
            ("general", false),
            ("engineering", false),
            ("design", false),
            ("random", false),
            ("Maya Chen", true),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (name, is_im))| Conversation {
            id: format!("D{i}"),
            name: name.into(),
            is_im,
            is_member: true,
            ..Default::default()
        })
        .collect();
        if graphics {
            app.demo_images = crate::demo::previews();
        }
        app.load_demo();
        app
    }
    fn load_demo(&mut self) {
        let channel = self.current_id().unwrap_or("").to_owned();
        let key = (channel.clone(), self.thread.clone());
        self.messages = self.demo_saved.get(&key).cloned().unwrap_or_else(|| {
            if let Some(ts) = &self.thread {
                self.demo_saved
                    .get(&(channel, None))
                    .and_then(|messages| messages.iter().find(|m| m.ts == *ts))
                    .cloned()
                    .map(crate::demo::thread)
                    .unwrap_or_default()
            } else {
                crate::demo::messages()
            }
        });
        self.selected = self.messages.len().saturating_sub(1);
        self.status = "Demo · messages stay on this device".into();
    }
    fn save_demo_messages(&mut self) {
        if !self.demo {
            return;
        }
        if let Some(channel) = self.current_id().map(str::to_owned) {
            self.demo_saved.insert(
                (channel.clone(), self.thread.clone()),
                self.messages.clone(),
            );
            if self.thread.is_none() {
                for parent in &self.messages {
                    if let Some(thread) = self
                        .demo_saved
                        .get_mut(&(channel.clone(), Some(parent.ts.clone())))
                        && let Some(root) = thread.first_mut()
                    {
                        *root = parent.clone();
                    }
                }
            }
            if let Some(ts) = &self.thread
                && let Some(messages) = self.demo_saved.get_mut(&(channel, None))
                && let Some(parent) = messages.iter_mut().find(|m| m.ts == *ts)
            {
                parent.reply_count = self.messages.len().saturating_sub(1);
                if let Some(root) = self.messages.first() {
                    parent.reactions = root.reactions.clone();
                }
            }
        }
    }
    pub fn current_id(&self) -> Option<&str> {
        self.conversations.get(self.channel).map(|c| c.id.as_str())
    }
    pub fn filtered(&self) -> Vec<usize> {
        let mut matched: Vec<_> = self
            .conversations
            .iter()
            .enumerate()
            .filter_map(|(i, c)| fuzzy_score(&self.query.text, &c.name).map(|score| (score, i)))
            .collect();
        matched.sort_by_key(|(score, i)| (*score, *i));
        matched.into_iter().map(|(_, i)| i).collect()
    }
    fn save_draft(&mut self) {
        self.save_demo_messages();
        if let Some(id) = self.current_id().map(str::to_owned) {
            self.drafts
                .insert((id, self.thread.clone()), std::mem::take(&mut self.editor));
        }
    }
    fn restore_draft(&mut self) {
        if let Some(id) = self.current_id().map(str::to_owned) {
            self.editor = self
                .drafts
                .remove(&(id, self.thread.clone()))
                .unwrap_or_default();
        }
    }
    pub fn open_channel(&mut self, index: usize) {
        if index >= self.conversations.len() {
            return;
        }
        self.generation += 1;
        self.media_generation += 1;
        self.loading = false;
        self.save_draft();
        self.channel = index;
        self.sidebar = index;
        self.thread = None;
        self.thread_latest = None;
        self.restore_draft();
        self.mode = Mode::Normal;
        self.focus = Focus::Messages;
        self.messages.clear();
        self.selected = 0;
        self.cursor.clear();
        self.refresh(false);
    }
    pub fn refresh(&mut self, older: bool) {
        if self.demo {
            self.load_demo();
            return;
        }
        if self.loading {
            return;
        }
        if let (Some(id), Some(worker)) = (self.current_id().map(str::to_owned), &self.worker) {
            self.generation += 1;
            let _ = worker.jobs.send(Job::History {
                id: self.generation,
                channel: id.to_owned(),
                thread: self.thread.clone(),
                oldest: if self.thread.is_some() {
                    self.thread_latest.clone()
                } else {
                    None
                },
                cursor: if older && self.thread.is_none() {
                    self.cursor.clone()
                } else {
                    String::new()
                },
            });
            self.loading = true;
            self.status = "Loading messages…".into();
        }
    }
    pub fn open_thread(&mut self) {
        if self.thread.is_some() {
            return;
        }
        if let Some(message) = self.messages.get(self.selected) {
            let ts = message.ts.clone();
            self.save_draft();
            self.thread = Some(ts);
            self.thread_latest = None;
            self.restore_draft();
            self.loading = false;
            self.messages.clear();
            self.cursor.clear();
            self.selected = 0;
            if self.demo {
                self.load_demo();
            } else {
                self.refresh(false);
            }
        }
    }
    pub fn close_thread(&mut self) {
        if self.thread.is_some() {
            self.open_channel(self.channel);
        }
    }
    pub fn send(&mut self) {
        if self.sending || self.editor.text.trim().is_empty() {
            return;
        }
        if self.demo {
            self.messages.push(Message {
                ts: format!("demo-{}", self.messages.len()),
                user: self.me.clone(),
                text: std::mem::take(&mut self.editor.text),
                ..Default::default()
            });
            self.editor.cursor = 0;
            self.selected = self.messages.len() - 1;
            self.mode = Mode::Normal;
            self.status = "Message sent · demo only".into();
            self.save_demo_messages();
            return;
        }
        if let (Some(channel), Some(worker)) = (self.current_id().map(str::to_owned), &self.worker)
        {
            let _ = worker.jobs.send(Job::Send {
                channel: channel.to_owned(),
                thread: self.thread.clone(),
                text: self.editor.text.clone(),
            });
            self.sending = true;
            self.status = "Sending…".into();
        }
    }
    pub fn open_media(&mut self) {
        let index = self
            .messages
            .get(self.selected)
            .and_then(|m| m.files.iter().position(|f| f.is_media()))
            .unwrap_or(0);
        self.open_attachment(index);
    }
    pub fn open_attachment(&mut self, index: usize) {
        if !self.graphics {
            self.status =
                "Kitty graphics unavailable; use a compatible terminal or --graphics kitty".into();
            return;
        }
        let file = self
            .messages
            .get(self.selected)
            .and_then(|m| m.files.get(index).filter(|f| f.is_media()))
            .cloned();
        if self.demo {
            if let Some(file) = file {
                match crate::demo::prepare(&file).and_then(Viewer::new) {
                    Ok(viewer) => self.viewer = Some(viewer),
                    Err(error) => self.status = format!("{error:#}"),
                }
            }
            return;
        }
        if let (Some(file), Some(worker)) = (file, &self.worker) {
            self.media_generation += 1;
            let id = self.media_generation;
            let client = worker.slack.clone();
            let tx = worker.events.clone();
            self.status = format!("Loading {}…", safe_text(&file.name));
            thread::spawn(move || {
                let result = (|| {
                    let temp = client.download(&file)?;
                    let mut prepared = Prepared::local(temp.path().to_path_buf(), file.is_video())?;
                    prepared.title = file.name.clone();
                    prepared.temporary = Some(temp);
                    Ok(prepared)
                })();
                let _ = tx.send(Reply::Media { id, result });
            });
        } else {
            self.status = "No image or video attached to the selected message".into();
        }
    }
    pub fn toggle_demo_reaction(&mut self, message: usize, reaction: usize) {
        if !self.demo {
            return;
        }
        if let Some(reaction) = self
            .messages
            .get_mut(message)
            .and_then(|m| m.reactions.get_mut(reaction))
        {
            if let Some(index) = reaction.users.iter().position(|user| user == &self.me) {
                reaction.users.remove(index);
                reaction.count = reaction.count.saturating_sub(1);
            } else {
                reaction.users.push(self.me.clone());
                reaction.count += 1;
            }
            self.save_demo_messages();
        }
    }
    pub fn receive(&mut self, reply: Reply) {
        match reply {
            Reply::Bootstrap(result) => {
                self.loading = false;
                match result {
                    Ok(data) => {
                        self.workspace = data.workspace;
                        self.me = data.me;
                        self.users = data.users;
                        self.conversations = data.conversations;
                        if self.conversations.is_empty() {
                            self.status =
                                "No conversations. Check Slack scopes and channel membership."
                                    .into();
                        } else {
                            self.open_channel(0);
                        }
                    }
                    Err(e) => self.status = format!("{e:#} • R retries connection"),
                }
            }
            Reply::History { id, append, result } if id == self.generation => {
                self.loading = false;
                match result {
                    Ok(history) => {
                        if self.thread.is_none()
                            && let Some(channel) = self.current_id().map(str::to_owned)
                        {
                            self.notifications.observe(
                                &channel,
                                &history.messages,
                                &self.me,
                                append,
                            );
                        }
                        let selected_ts = self.messages.get(self.selected).map(|m| m.ts.clone());
                        if self.thread.is_some()
                            && let Some(last) = history.messages.last()
                        {
                            self.thread_latest = Some(last.ts.clone());
                        }
                        if append || self.thread.is_some() {
                            self.messages.extend(history.messages);
                            self.messages.sort_by(|a, b| a.ts.cmp(&b.ts));
                            self.messages.dedup_by(|a, b| a.ts == b.ts);
                        } else {
                            let reading = (self.mode == Mode::Read)
                                .then(|| self.messages.get(self.selected).cloned())
                                .flatten();
                            self.messages = history.messages;
                            if let Some(reading) = reading
                                && !self.messages.iter().any(|message| message.ts == reading.ts)
                            {
                                self.messages.push(reading);
                                self.messages.sort_by(|a, b| a.ts.cmp(&b.ts));
                            }
                        }
                        self.cursor = history.cursor;
                        self.selected = selected_ts
                            .and_then(|ts| self.messages.iter().position(|m| m.ts == ts))
                            .unwrap_or_else(|| self.messages.len().saturating_sub(1));
                        self.status = if self.cursor.is_empty() {
                            "Updated • R refresh • ? shortcuts".into()
                        } else {
                            "Updated • PgUp loads more • R refresh".into()
                        };
                    }
                    Err(e) => self.status = format!("{e:#}"),
                }
            }
            Reply::History { .. } => {}
            Reply::Sent {
                channel,
                thread,
                text,
                result,
            } => {
                self.sending = false;
                match result {
                    Ok(mut message) => {
                        if self.current_id() == Some(&channel) && self.thread == thread {
                            if self.editor.text == text {
                                self.editor = Editor::default();
                            }
                            self.mode = Mode::Normal;
                            self.loading = false;
                            self.refresh(false);
                            if message.user.is_empty() {
                                message.user = self.me.clone();
                            }
                            if !self.messages.iter().any(|m| m.ts == message.ts) {
                                self.messages.push(message);
                            }
                            self.messages.sort_by(|a, b| a.ts.cmp(&b.ts));
                            self.selected = self.messages.len().saturating_sub(1);
                        } else if self
                            .drafts
                            .get(&(channel.clone(), thread.clone()))
                            .is_some_and(|d| d.text == text)
                        {
                            self.drafts.remove(&(channel, thread));
                        }
                        self.status = "Message sent".into();
                    }
                    Err(e) => {
                        self.status = format!("{e:#} • draft kept; check Slack before retrying")
                    }
                }
            }
            Reply::Media { id, result } if id == self.media_generation => {
                match result.and_then(Viewer::new) {
                    Ok(viewer) => self.viewer = Some(viewer),
                    Err(e) => self.status = format!("{e:#}"),
                }
            }
            Reply::Media { .. } => {}
        }
    }
    pub fn poll(&mut self) {
        let mut replies = Vec::new();
        if let Some(worker) = &self.worker {
            while let Ok(reply) = worker.replies.try_recv() {
                replies.push(reply);
            }
        }
        for reply in replies {
            self.receive(reply);
        }
    }
    pub fn move_selection(&mut self, delta: isize) {
        let (selected, len) = if self.focus == Focus::Channels {
            (&mut self.sidebar, self.conversations.len())
        } else {
            (&mut self.selected, self.messages.len())
        };
        *selected = selected
            .saturating_add_signed(delta)
            .min(len.saturating_sub(1));
    }
    pub fn key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        if self.viewer.is_some() {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.viewer = None;
                }
                KeyCode::Char(' ') => {
                    if let Some(video) = self.viewer.as_ref().and_then(|v| v.video.as_ref()) {
                        video.toggle();
                    }
                }
                _ => {}
            }
            return;
        }
        if self.mode == Mode::Read {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => self.mode = Mode::Normal,
                KeyCode::Char('j') | KeyCode::Down => {
                    self.read_scroll = self.read_scroll.saturating_add(1)
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.read_scroll = self.read_scroll.saturating_sub(1)
                }
                KeyCode::PageDown => self.read_scroll = self.read_scroll.saturating_add(10),
                KeyCode::PageUp => self.read_scroll = self.read_scroll.saturating_sub(10),
                KeyCode::Home | KeyCode::Char('g') => self.read_scroll = 0,
                KeyCode::Char('v') => self.open_media(),
                KeyCode::Char('t') => {
                    self.mode = Mode::Normal;
                    self.open_thread();
                }
                _ => {}
            }
            return;
        }
        if self.mode == Mode::Help {
            if matches!(
                key.code,
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')
            ) {
                self.mode = Mode::Normal;
            }
            return;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('k') {
            self.mode = Mode::Switcher;
            self.query = Editor::default();
            self.switch_selected = 0;
            return;
        }
        match self.mode {
            Mode::Compose => match key.code {
                KeyCode::Esc => self.mode = Mode::Normal,
                KeyCode::Enter if key.modifiers.contains(KeyModifiers::ALT) => {
                    self.editor.insert("\n")
                }
                KeyCode::Enter => self.send(),
                _ if !self.sending => self.editor.key(key),
                _ => {}
            },
            Mode::Switcher => match key.code {
                KeyCode::Esc => self.mode = Mode::Normal,
                KeyCode::Enter => {
                    if let Some(index) = self.filtered().get(self.switch_selected).copied() {
                        self.loading = false;
                        self.open_channel(index);
                    }
                }
                KeyCode::Up => self.switch_selected = self.switch_selected.saturating_sub(1),
                KeyCode::Down => {
                    self.switch_selected =
                        (self.switch_selected + 1).min(self.filtered().len().saturating_sub(1))
                }
                _ => {
                    self.query.key(key);
                    self.switch_selected = 0;
                }
            },
            Mode::Normal => match key.code {
                KeyCode::Char('q') => self.quit = true,
                KeyCode::Char('?') | KeyCode::F(1) => self.mode = Mode::Help,
                KeyCode::Char('o') => {
                    self.read_scroll = 0;
                    self.mode = Mode::Read;
                }
                KeyCode::Char('i') => self.mode = Mode::Compose,
                KeyCode::Char('j') | KeyCode::Down => self.move_selection(1),
                KeyCode::Char('k') | KeyCode::Up => self.move_selection(-1),
                KeyCode::Char('h') => self.focus = Focus::Channels,
                KeyCode::Char('l') => self.focus = Focus::Messages,
                KeyCode::Tab | KeyCode::BackTab => {
                    self.focus = if self.focus == Focus::Channels {
                        Focus::Messages
                    } else {
                        Focus::Channels
                    }
                }
                KeyCode::Enter if self.focus == Focus::Channels => {
                    self.loading = false;
                    self.open_channel(self.sidebar);
                }
                KeyCode::Enter | KeyCode::Char('t') => self.open_thread(),
                KeyCode::Char('v') => self.open_media(),
                KeyCode::Char('g') | KeyCode::Home => {
                    if self.focus == Focus::Channels {
                        self.sidebar = 0;
                    } else {
                        self.selected = 0;
                    }
                }
                KeyCode::Char('G') | KeyCode::End => {
                    if self.focus == Focus::Channels {
                        self.sidebar = self.conversations.len().saturating_sub(1);
                    } else {
                        self.selected = self.messages.len().saturating_sub(1);
                    }
                }
                KeyCode::Char('R') => {
                    if self.conversations.is_empty() {
                        if let Some(worker) = &self.worker {
                            let _ = worker.jobs.send(Job::Bootstrap);
                            self.loading = true;
                            self.status = "Connecting…".into();
                        }
                    } else {
                        self.refresh(false);
                    }
                }
                KeyCode::PageUp => {
                    if !self.cursor.is_empty() {
                        self.refresh(true);
                    } else {
                        self.move_selection(-5);
                    }
                }
                KeyCode::PageDown => self.move_selection(5),
                KeyCode::Esc => self.close_thread(),
                _ => {}
            },
            Mode::Help | Mode::Read => {}
        }
    }
}
