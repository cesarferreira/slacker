use crate::{
    api::{Conversation, History, Message, Person, Presence, Slack, Snapshot},
    media::{Prepared, Video},
};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::{
    collections::HashMap,
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant},
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
    Thread,
    People,
}

pub enum Job {
    Bootstrap,
    Counts,
    Presence {
        users: Vec<String>,
    },
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
    Counts(Result<HashMap<String, usize>>),
    Presence {
        users: Vec<String>,
        result: Result<HashMap<String, Presence>>,
    },
    History {
        id: u64,
        channel: String,
        thread: Option<String>,
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
                    Job::Counts => {
                        let client = client.clone();
                        let tx = tx.clone();
                        thread::spawn(move || {
                            let _ = tx.send(Reply::Counts(client.unread_counts()));
                        });
                        continue;
                    }
                    Job::Presence { users } => {
                        let client = client.clone();
                        let tx = tx.clone();
                        thread::spawn(move || {
                            let result = client.presences(&users);
                            let _ = tx.send(Reply::Presence { users, result });
                        });
                        continue;
                    }
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
                        channel,
                        thread,
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

#[derive(Default)]
pub struct ThreadView {
    pub messages: Vec<Message>,
    pub selected: usize,
    pub editor: Editor,
    pub cursor: String,
    pub latest: Option<String>,
    pub generation: u64,
    pub loading: bool,
    pub sending: bool,
    pub read_scroll: usize,
}

pub struct App {
    pub notifications: crate::notifications::Notifications,
    pub demo_images: HashMap<String, crate::media::Pixels>,
    demo_saved: HashMap<(String, Option<String>), Vec<Message>>,
    pub workspace: String,
    pub me: String,
    pub users: HashMap<String, String>,
    pub people: HashMap<String, Person>,
    pub presences: HashMap<String, Presence>,
    pub presence_loading: bool,
    pub unread_counts: HashMap<String, usize>,
    unread_loading: bool,
    unread_checked: Option<Instant>,
    presence_checked: HashMap<String, Instant>,
    request_generation: u64,
    pending_sends: HashMap<(String, Option<String>), String>,
    sent_messages: HashMap<(String, Option<String>), Vec<Message>>,
    pub conversations: Vec<Conversation>,
    pub channel: usize,
    pub sidebar: usize,
    pub person: usize,
    pub sidebar_width: Option<u16>,
    pub thread_width: Option<u16>,
    pub messages: Vec<Message>,
    pub selected: usize,
    pub read_scroll: usize,
    pub thread: Option<String>,
    pub thread_view: Option<ThreadView>,
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
            people: HashMap::new(),
            presences: HashMap::new(),
            presence_loading: false,
            unread_counts: HashMap::new(),
            unread_loading: false,
            unread_checked: None,
            presence_checked: HashMap::new(),
            request_generation: 0,
            pending_sends: HashMap::new(),
            sent_messages: HashMap::new(),
            conversations: Vec::new(),
            channel: 0,
            sidebar: 0,
            person: 0,
            sidebar_width: None,
            thread_width: None,
            messages: Vec::new(),
            selected: 0,
            read_scroll: 0,
            thread: None,
            thread_view: None,
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
        app.people = app
            .users
            .iter()
            .map(|(id, name)| {
                (
                    id.clone(),
                    Person {
                        display_name: name.clone(),
                        status_emoji: if id == "maya" {
                            ":coffee:".into()
                        } else {
                            String::new()
                        },
                        status_text: if id == "maya" {
                            "Pairing after lunch".into()
                        } else {
                            String::new()
                        },
                        is_bot: false,
                    },
                )
            })
            .collect();
        app.presences = HashMap::from([
            ("maya".into(), Presence::Active),
            ("leo".into(), Presence::Away),
            ("you".into(), Presence::DoNotDisturb),
        ]);
        app.conversations = [
            ("general", false),
            ("engineering", false),
            ("design", false),
            ("random", false),
            ("Maya Chen", true),
            ("Leo Rivera", true),
            ("You", true),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (name, is_im))| Conversation {
            id: format!("D{i}"),
            name: name.into(),
            user: if is_im {
                match name {
                    "Maya Chen" => "maya",
                    "Leo Rivera" => "leo",
                    _ => "you",
                }
                .into()
            } else {
                String::new()
            },
            is_im,
            is_member: true,
            ..Default::default()
        })
        .collect();
        if graphics {
            app.demo_images = crate::demo::previews();
        }
        app.person = app.conversations.iter().position(|c| c.is_im).unwrap_or(0);
        app.unread_counts = HashMap::from([("D1".into(), 2), ("D3".into(), 1)]);
        app.load_demo();
        app
    }
    fn load_demo(&mut self) {
        let channel = self.current_id().unwrap_or("").to_owned();
        let previous = self.messages.get(self.selected).map(|m| m.ts.clone());
        self.messages = self
            .demo_saved
            .get(&(channel, None))
            .cloned()
            .unwrap_or_else(|| {
                crate::demo::messages_for(
                    self.conversations
                        .get(self.channel)
                        .map_or("general", |c| c.name.as_str()),
                )
            });
        self.selected = previous
            .and_then(|ts| self.messages.iter().position(|m| m.ts == ts))
            .or_else(|| {
                self.messages
                    .iter()
                    .position(|m| m.files.iter().any(|f| f.is_media()))
            })
            .unwrap_or_else(|| self.messages.len().saturating_sub(1));
        self.status = "Demo · messages stay on this device".into();
    }
    fn save_demo_messages(&mut self) {
        if !self.demo {
            return;
        }
        if let Some(channel) = self.current_id().map(str::to_owned) {
            if let (Some(ts), Some(view)) = (&self.thread, &mut self.thread_view) {
                if let Some(parent) = self.messages.iter_mut().find(|m| m.ts == *ts) {
                    parent.reply_count = view.messages.len().saturating_sub(1);
                    if let Some(root) = view.messages.first() {
                        parent.reactions = root.reactions.clone();
                    }
                }
                self.demo_saved
                    .insert((channel.clone(), Some(ts.clone())), view.messages.clone());
            }
            self.demo_saved
                .insert((channel.clone(), None), self.messages.clone());
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
    }
    pub fn active_editor(&self) -> &Editor {
        if self.focus == Focus::Thread
            && let Some(view) = &self.thread_view
        {
            return &view.editor;
        }
        &self.editor
    }
    pub fn active_editor_mut(&mut self) -> &mut Editor {
        if self.focus == Focus::Thread
            && let Some(view) = &mut self.thread_view
        {
            return &mut view.editor;
        }
        &mut self.editor
    }
    pub fn active_messages(&self) -> (&[Message], usize) {
        if self.focus == Focus::Thread
            && let Some(view) = &self.thread_view
        {
            return (&view.messages, view.selected);
        }
        (&self.messages, self.selected)
    }
    pub fn active_sending(&self) -> bool {
        if self.focus == Focus::Thread
            && let Some(view) = &self.thread_view
        {
            return view.sending;
        }
        self.sending
    }
    pub fn focus_channel(&mut self) {
        self.focus = Focus::Messages;
    }
    pub fn focus_thread(&mut self) {
        if self.thread_view.is_some() {
            self.focus = Focus::Thread;
        }
    }
    pub fn start_composing(&mut self, thread: bool) {
        if thread {
            self.focus_thread();
        } else {
            self.focus_channel();
        }
        self.mode = Mode::Compose;
    }
    fn next_request(&mut self) -> u64 {
        self.request_generation = self.request_generation.max(self.generation) + 1;
        self.request_generation
    }
    pub fn refresh_unreads(&mut self) {
        if self.demo
            || self.unread_loading
            || self.conversations.is_empty()
            || self
                .unread_checked
                .is_some_and(|checked| checked.elapsed() < Duration::from_secs(60))
        {
            return;
        }
        if let Some(worker) = &self.worker
            && worker.jobs.send(Job::Counts).is_ok()
        {
            self.unread_loading = true;
            self.unread_checked = Some(Instant::now());
        }
    }
    pub fn refresh_presence(&mut self, users: &[String]) {
        if self.demo || self.presence_loading {
            return;
        }
        let Some(worker) = &self.worker else {
            return;
        };
        let mut requested = Vec::new();
        let now = Instant::now();
        for user in users {
            if requested.len() == 10 {
                break;
            }
            if !user.is_empty()
                && !requested.contains(user)
                && self
                    .presence_checked
                    .get(user)
                    .is_none_or(|checked| now.duration_since(*checked) >= Duration::from_secs(60))
            {
                requested.push(user.clone());
            }
        }
        if requested.is_empty() {
            return;
        }
        if worker
            .jobs
            .send(Job::Presence {
                users: requested.clone(),
            })
            .is_ok()
        {
            for user in requested {
                self.presence_checked.insert(user, now);
            }
            self.presence_loading = true;
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
                .insert((id.clone(), None), std::mem::take(&mut self.editor));
            if let (Some(root), Some(view)) = (&self.thread, &mut self.thread_view) {
                self.drafts
                    .insert((id, Some(root.clone())), std::mem::take(&mut view.editor));
            }
        }
    }
    pub fn open_channel(&mut self, index: usize) {
        if index >= self.conversations.len() {
            return;
        }
        self.save_draft();
        self.generation = self.next_request();
        self.media_generation += 1;
        self.channel = index;
        if self.conversations[index].is_im {
            self.person = index;
        } else {
            self.sidebar = index;
        }
        self.thread = None;
        self.thread_view = None;
        let key = (self.current_id().unwrap_or("").to_owned(), None);
        self.editor = self.drafts.remove(&key).unwrap_or_default();
        self.sending = self.pending_sends.contains_key(&key);
        self.mode = Mode::Normal;
        self.focus = Focus::Messages;
        self.messages.clear();
        self.selected = 0;
        self.read_scroll = 0;
        self.loading = false;
        self.cursor.clear();
        self.refresh_channel(false);
    }
    pub fn refresh(&mut self, older: bool) {
        if self.focus == Focus::Thread && self.thread_view.is_some() {
            self.refresh_thread(older);
        } else {
            self.refresh_channel(older);
        }
    }
    pub fn refresh_channel(&mut self, older: bool) {
        if self.demo {
            if !self.messages.is_empty() {
                self.save_demo_messages();
            }
            self.load_demo();
            return;
        }
        if self.loading {
            return;
        }
        let Some(channel) = self.current_id().map(str::to_owned) else {
            return;
        };
        if self.worker.is_none() {
            return;
        }
        self.generation = self.next_request();
        let job = Job::History {
            id: self.generation,
            channel,
            thread: None,
            oldest: None,
            cursor: if older {
                self.cursor.clone()
            } else {
                String::new()
            },
        };
        if self.worker.as_ref().unwrap().jobs.send(job).is_ok() {
            self.loading = true;
            self.status = "Loading messages…".into();
        }
    }
    pub fn refresh_thread(&mut self, older: bool) {
        if self.demo {
            return;
        }
        let (Some(channel), Some(root), Some(view), Some(_)) = (
            self.current_id().map(str::to_owned),
            self.thread.clone(),
            self.thread_view.as_ref(),
            self.worker.as_ref(),
        ) else {
            return;
        };
        if view.loading {
            return;
        }
        let cursor = if older {
            view.cursor.clone()
        } else {
            String::new()
        };
        let oldest = if older { None } else { view.latest.clone() };
        let id = self.next_request();
        if self
            .worker
            .as_ref()
            .unwrap()
            .jobs
            .send(Job::History {
                id,
                channel,
                thread: Some(root),
                cursor,
                oldest,
            })
            .is_ok()
        {
            let view = self.thread_view.as_mut().unwrap();
            view.generation = id;
            view.loading = true;
        }
    }
    pub fn open_thread(&mut self) {
        let Some(parent) = self.messages.get(self.selected).cloned() else {
            return;
        };
        if self.thread.as_deref() == Some(&parent.ts) {
            self.start_composing(true);
            return;
        }
        self.close_thread();
        self.save_demo_messages();
        let channel = self.current_id().unwrap_or("").to_owned();
        let key = (channel, Some(parent.ts.clone()));
        let mut messages = if self.demo {
            self.demo_saved
                .get(&key)
                .cloned()
                .unwrap_or_else(|| crate::demo::thread(parent.clone()))
        } else {
            vec![parent.clone()]
        };
        if let Some(root) = messages.first_mut() {
            *root = parent.clone();
        }
        self.thread = Some(parent.ts);
        let generation = self.next_request();
        self.thread_view = Some(ThreadView {
            selected: 0,
            messages,
            editor: self.drafts.remove(&key).unwrap_or_default(),
            generation,
            sending: self.pending_sends.contains_key(&key),
            ..Default::default()
        });
        self.start_composing(true);
        self.refresh_thread(false);
    }
    pub fn close_thread(&mut self) {
        self.save_demo_messages();
        if let (Some(channel), Some(root), Some(view)) = (
            self.current_id().map(str::to_owned),
            self.thread.take(),
            self.thread_view.take(),
        ) {
            self.drafts.insert((channel, Some(root)), view.editor);
        }
        self.mode = Mode::Compose;
        self.focus = Focus::Messages;
    }
    pub fn send(&mut self) {
        if self.active_sending() || self.active_editor().text.trim().is_empty() {
            return;
        }
        let thread = if self.focus == Focus::Thread {
            self.thread.clone()
        } else {
            None
        };
        let text = self.active_editor().text.clone();
        if self.demo {
            let message = Message {
                ts: format!("demo-{}", self.next_request()),
                user: self.me.clone(),
                text,
                ..Default::default()
            };
            *self.active_editor_mut() = Editor::default();
            if let Some(view) = self.thread_view.as_mut().filter(|_| thread.is_some()) {
                view.messages.push(message);
                view.selected = view.messages.len() - 1;
            } else {
                self.messages.push(message);
                self.selected = self.messages.len() - 1;
            }
            self.mode = Mode::Compose;
            self.status = "Message sent · demo only".into();
            self.save_demo_messages();
            return;
        }
        if let (Some(channel), Some(worker)) = (self.current_id().map(str::to_owned), &self.worker)
            && worker
                .jobs
                .send(Job::Send {
                    channel: channel.clone(),
                    thread: thread.clone(),
                    text: text.clone(),
                })
                .is_ok()
        {
            self.pending_sends.insert((channel, thread.clone()), text);
            if let Some(view) = self.thread_view.as_mut().filter(|_| thread.is_some()) {
                view.sending = true;
            } else {
                self.sending = true;
            }
            self.status = "Sending…".into();
        }
    }
    pub fn open_media(&mut self) {
        let (messages, selected) = self.active_messages();
        let index = messages
            .get(selected)
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
        let (messages, selected) = self.active_messages();
        let file = messages
            .get(selected)
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
        let in_thread = self.focus == Focus::Thread;
        let messages = if in_thread {
            &mut self.thread_view.as_mut().unwrap().messages
        } else {
            &mut self.messages
        };
        if let Some(reaction) = messages
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
            if !in_thread
                && let Some(view) = &mut self.thread_view
                && let Some(parent) = self.messages.get(message)
                && self.thread.as_deref() == Some(&parent.ts)
                && let Some(root) = view.messages.first_mut()
            {
                *root = parent.clone();
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
                        self.people = data.people;
                        self.conversations = data.conversations;
                        self.sidebar = self
                            .conversations
                            .iter()
                            .position(|c| !c.is_im)
                            .unwrap_or(0);
                        self.person = self.conversations.iter().position(|c| c.is_im).unwrap_or(0);
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
            Reply::Counts(result) => {
                self.unread_loading = false;
                if let Ok(counts) = result {
                    self.unread_counts = counts;
                }
            }
            Reply::Presence { users, result } => {
                self.presence_loading = false;
                let values = result.unwrap_or_default();
                for user in users {
                    self.presences
                        .insert(user.clone(), values.get(&user).copied().unwrap_or_default());
                }
            }
            Reply::History {
                id,
                channel,
                thread,
                append,
                result,
            } => {
                if self.current_id() != Some(channel.as_str()) {
                    return;
                }
                if let Some(root) = &thread {
                    if self.thread.as_ref() != Some(root) {
                        return;
                    }
                    let Some(view) = &mut self.thread_view else {
                        return;
                    };
                    if id != view.generation {
                        return;
                    }
                    view.loading = false;
                } else {
                    if id != self.generation {
                        return;
                    }
                    self.loading = false;
                }
                match result {
                    Ok(mut history) => {
                        let latest = history.messages.last().map(|m| m.ts.clone());
                        if thread.is_none() {
                            self.notifications.observe(
                                &channel,
                                &history.messages,
                                &self.me,
                                append,
                            );
                        }
                        if let Some(sent) = self.sent_messages.get_mut(&(channel, thread.clone())) {
                            sent.retain(|message| {
                                !history.messages.iter().any(|m| m.ts == message.ts)
                            });
                            history.messages.extend(sent.iter().cloned());
                        }
                        let (messages, selected, cursor) = if thread.is_some() {
                            let view = self.thread_view.as_mut().unwrap();
                            if latest.is_some() {
                                view.latest = latest;
                            }
                            (&mut view.messages, &mut view.selected, &mut view.cursor)
                        } else {
                            (&mut self.messages, &mut self.selected, &mut self.cursor)
                        };
                        let selected_ts = messages.get(*selected).map(|m| m.ts.clone());
                        if append || thread.is_some() {
                            messages.extend(history.messages);
                        } else {
                            let reading = (self.mode == Mode::Read)
                                .then(|| messages.get(*selected).cloned())
                                .flatten();
                            *messages = history.messages;
                            if let Some(reading) = reading
                                && !messages.iter().any(|m| m.ts == reading.ts)
                            {
                                messages.push(reading);
                            }
                        }
                        messages.sort_by(|a, b| a.ts.cmp(&b.ts));
                        messages.dedup_by(|a, b| a.ts == b.ts);
                        *cursor = history.cursor;
                        *selected = selected_ts
                            .and_then(|ts| messages.iter().position(|m| m.ts == ts))
                            .unwrap_or_else(|| messages.len().saturating_sub(1));
                        self.status = "Updated • R refresh • ? shortcuts".into();
                    }
                    Err(e) => self.status = format!("{e:#}"),
                }
            }
            Reply::Sent {
                channel,
                thread,
                text,
                result,
            } => {
                self.pending_sends
                    .remove(&(channel.clone(), thread.clone()));
                let same_channel = self.current_id() == Some(channel.as_str());
                let same_thread = same_channel && thread.is_some() && self.thread == thread;
                if same_channel && thread.is_none() {
                    self.sending = false;
                }
                if same_thread && let Some(view) = &mut self.thread_view {
                    view.sending = false;
                }
                match result {
                    Ok(mut message) => {
                        if message.user.is_empty() {
                            message.user = self.me.clone();
                        }
                        self.sent_messages
                            .entry((channel.clone(), thread.clone()))
                            .or_default()
                            .push(message.clone());
                        let target = if same_thread {
                            self.thread_view.as_mut().map(|view| {
                                (&mut view.editor, &mut view.messages, &mut view.selected)
                            })
                        } else if same_channel && thread.is_none() {
                            Some((&mut self.editor, &mut self.messages, &mut self.selected))
                        } else {
                            None
                        };
                        if let Some((editor, messages, selected)) = target {
                            if editor.text == text {
                                *editor = Editor::default();
                            }
                            if !messages.iter().any(|m| m.ts == message.ts) {
                                messages.push(message);
                                messages.sort_by(|a, b| a.ts.cmp(&b.ts));
                            }
                            *selected = messages.len().saturating_sub(1);
                        } else {
                            let key = (channel, thread);
                            if self.drafts.get(&key).is_some_and(|d| d.text == text) {
                                self.drafts.remove(&key);
                            }
                            if let Some(messages) = self.demo_saved.get_mut(&key)
                                && !messages.iter().any(|m| m.ts == message.ts)
                            {
                                messages.push(message);
                            }
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
        if matches!(self.focus, Focus::Channels | Focus::People) {
            let is_im = self.focus == Focus::People;
            let indices: Vec<_> = self
                .conversations
                .iter()
                .enumerate()
                .filter(|(_, c)| c.is_im == is_im)
                .map(|(i, _)| i)
                .collect();
            let selected = if is_im {
                &mut self.person
            } else {
                &mut self.sidebar
            };
            if let Some(position) = indices.iter().position(|i| i == selected) {
                *selected = indices[position
                    .saturating_add_signed(delta)
                    .min(indices.len().saturating_sub(1))];
            } else if let Some(first) = indices.first() {
                *selected = *first;
            }
            return;
        }
        let (selected, len) = if self.focus == Focus::Thread
            && let Some(view) = &mut self.thread_view
        {
            (&mut view.selected, view.messages.len())
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
            let read_scroll = if self.focus == Focus::Thread
                && let Some(view) = &mut self.thread_view
            {
                &mut view.read_scroll
            } else {
                &mut self.read_scroll
            };
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => self.mode = Mode::Normal,
                KeyCode::Char('j') | KeyCode::Down => *read_scroll = read_scroll.saturating_add(1),
                KeyCode::Char('k') | KeyCode::Up => *read_scroll = read_scroll.saturating_sub(1),
                KeyCode::PageDown => *read_scroll = read_scroll.saturating_add(10),
                KeyCode::PageUp => *read_scroll = read_scroll.saturating_sub(10),
                KeyCode::Home | KeyCode::Char('g') => *read_scroll = 0,
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
                    self.active_editor_mut().insert("\n")
                }
                KeyCode::Enter => self.send(),
                _ if !self.active_sending() => self.active_editor_mut().key(key),
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
                    if self.focus == Focus::Thread
                        && let Some(view) = &mut self.thread_view
                    {
                        view.read_scroll = 0;
                    } else {
                        self.read_scroll = 0;
                    }
                    self.mode = Mode::Read;
                }
                KeyCode::Char('i') => self.start_composing(self.focus == Focus::Thread),
                KeyCode::Char('j') | KeyCode::Down => self.move_selection(1),
                KeyCode::Char('k') | KeyCode::Up => self.move_selection(-1),
                KeyCode::Char('h') => self.focus = Focus::Channels,
                KeyCode::Char('l') => self.focus = Focus::Messages,
                KeyCode::Tab | KeyCode::BackTab => {
                    self.focus = match self.focus {
                        Focus::Channels => Focus::People,
                        Focus::People => Focus::Messages,
                        Focus::Messages if self.thread_view.is_some() => Focus::Thread,
                        _ => Focus::Channels,
                    }
                }
                KeyCode::Enter if self.focus == Focus::People => {
                    self.open_channel(self.person);
                }
                KeyCode::Enter if self.focus == Focus::Channels => {
                    self.loading = false;
                    self.open_channel(self.sidebar);
                }
                KeyCode::Enter | KeyCode::Char('t') => self.open_thread(),
                KeyCode::Char('v') => self.open_media(),
                KeyCode::Char('g') | KeyCode::Home => {
                    self.move_selection(isize::MIN);
                }
                KeyCode::Char('G') | KeyCode::End => {
                    self.move_selection(isize::MAX);
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
                    let cursor = if self.focus == Focus::Thread {
                        self.thread_view.as_ref().map_or("", |v| v.cursor.as_str())
                    } else {
                        self.cursor.as_str()
                    };
                    if !cursor.is_empty() {
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
