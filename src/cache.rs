//! Best-effort history cache. Only the cache thread touches SQLite in the app.
use crate::api::{Message, Snapshot};
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc::{self, Sender},
    thread::{self, JoinHandle},
    time::Duration,
};

pub const MAX_MESSAGES: usize = 500;
pub const MAX_FEEDS: usize = 100;
const MAX_AGE_SECONDS: i64 = 30 * 24 * 60 * 60;

#[derive(Clone, Default)]
pub struct Feed {
    pub messages: Vec<Message>,
    pub cursor: String,
    pub selected_ts: Option<String>,
    pub read_scroll: usize,
}

#[derive(Serialize, Deserialize)]
struct Position {
    cursor: String,
    selected_ts: Option<String>,
    read_scroll: usize,
}

pub struct Startup {
    pub snapshot: Snapshot,
    pub channel: String,
}

pub struct Cache {
    db: Connection,
    namespace: String,
}

impl Cache {
    pub fn default_path() -> Result<PathBuf> {
        Ok(dirs::cache_dir()
            .context("Could not locate the cache directory")?
            .join("slacker/history.sqlite3"))
    }

    pub fn open(path: &Path, namespace: &str) -> Result<Self> {
        let directory = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .context("Invalid cache directory")?;
        if fs::symlink_metadata(directory).is_ok_and(|m| m.file_type().is_symlink())
            || fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
        {
            bail!("Cache path must not be a symlink");
        }
        fs::create_dir_all(directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
            fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .mode(0o600)
                .open(path)?;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        }
        let db = Connection::open(path)?;
        db.busy_timeout(Duration::from_secs(2))?;
        db.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS accounts (
                 namespace TEXT PRIMARY KEY, snapshot TEXT NOT NULL,
                 channel TEXT NOT NULL DEFAULT '', updated_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS feeds (
                 namespace TEXT NOT NULL, channel TEXT NOT NULL, thread TEXT NOT NULL,
                 position TEXT NOT NULL, touched INTEGER NOT NULL,
                 PRIMARY KEY(namespace, channel, thread)
             );
             CREATE TABLE IF NOT EXISTS messages (
                 namespace TEXT NOT NULL, channel TEXT NOT NULL, thread TEXT NOT NULL,
                 ts TEXT NOT NULL, payload TEXT NOT NULL,
                 PRIMARY KEY(namespace, channel, thread, ts),
                 FOREIGN KEY(namespace, channel, thread) REFERENCES feeds(namespace, channel, thread)
                     ON DELETE CASCADE
             );"
        )?;
        // Retention is based on use, not message timestamps: old history can still be useful.
        db.execute(
            "DELETE FROM feeds WHERE touched < unixepoch() - ?1",
            [MAX_AGE_SECONDS],
        )?;
        db.execute(
            "DELETE FROM accounts WHERE updated_at < unixepoch() - ?1",
            [MAX_AGE_SECONDS],
        )?;
        Ok(Self {
            db,
            namespace: namespace.into(),
        })
    }

    pub fn load_startup(&self) -> Result<Option<Startup>> {
        let row: Option<(String, String)> = self
            .db
            .query_row(
                "SELECT snapshot, channel FROM accounts WHERE namespace = ?1",
                [&self.namespace],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        row.map(|(payload, channel)| {
            Ok(Startup {
                snapshot: serde_json::from_str(&payload)?,
                channel,
            })
        })
        .transpose()
    }

    pub fn save_snapshot(&mut self, snapshot: &Snapshot) -> Result<()> {
        self.db.execute(
            "INSERT INTO accounts(namespace, snapshot, updated_at) VALUES (?1, ?2, unixepoch())
             ON CONFLICT(namespace) DO UPDATE SET snapshot = excluded.snapshot, updated_at = excluded.updated_at",
            params![self.namespace, serde_json::to_string(snapshot)?],
        )?;
        Ok(())
    }

    pub fn select_channel(&mut self, channel: &str) -> Result<()> {
        self.db.execute(
            "UPDATE accounts SET channel = ?2, updated_at = unixepoch() WHERE namespace = ?1",
            params![self.namespace, channel],
        )?;
        Ok(())
    }

    pub fn load_feed(&self, channel: &str, thread: Option<&str>) -> Result<Option<Feed>> {
        let root = thread.unwrap_or("");
        let position: Option<String> = self
            .db
            .query_row(
                "SELECT position FROM feeds WHERE namespace = ?1 AND channel = ?2 AND thread = ?3",
                params![self.namespace, channel, root],
                |row| row.get(0),
            )
            .optional()?;
        let Some(position) = position else {
            return Ok(None);
        };
        let position: Position = serde_json::from_str(&position)?;
        let mut statement = self.db.prepare(
            "SELECT payload FROM messages WHERE namespace = ?1 AND channel = ?2 AND thread = ?3 ORDER BY ts"
        )?;
        let payloads = statement.query_map(params![self.namespace, channel, root], |row| {
            row.get::<_, String>(0)
        })?;
        let mut messages = Vec::new();
        for payload in payloads {
            messages.push(serde_json::from_str(&payload?)?);
        }
        Ok(Some(Feed {
            messages,
            cursor: position.cursor,
            selected_ts: position.selected_ts,
            read_scroll: position.read_scroll,
        }))
    }

    pub fn save_feed(&mut self, channel: &str, thread: Option<&str>, feed: &Feed) -> Result<()> {
        let root = thread.unwrap_or("");
        let position = serde_json::to_string(&Position {
            cursor: if feed.messages.len() > MAX_MESSAGES {
                String::new()
            } else {
                feed.cursor.clone()
            },
            selected_ts: feed.selected_ts.clone(),
            read_scroll: feed.read_scroll,
        })?;
        let tx = self.db.transaction()?;
        // rowid breaks same-second ties when pruning least recently visited feeds.
        tx.execute(
            "DELETE FROM feeds WHERE namespace = ?1 AND channel = ?2 AND thread = ?3",
            params![self.namespace, channel, root],
        )?;
        tx.execute("INSERT INTO feeds(namespace, channel, thread, position, touched) VALUES (?1, ?2, ?3, ?4, unixepoch())", params![self.namespace, channel, root, position])?;
        {
            let mut insert = tx.prepare("INSERT OR REPLACE INTO messages(namespace, channel, thread, ts, payload) VALUES (?1, ?2, ?3, ?4, ?5)")?;
            // Feed order is chronological in the application. Persist only its recent tail.
            for message in feed
                .messages
                .iter()
                .skip(feed.messages.len().saturating_sub(MAX_MESSAGES))
            {
                insert.execute(params![
                    self.namespace,
                    channel,
                    root,
                    message.ts,
                    serde_json::to_string(message)?
                ])?;
            }
        }
        tx.execute(
            "DELETE FROM feeds WHERE namespace = ?1 AND rowid NOT IN
             (SELECT rowid FROM feeds WHERE namespace = ?1 ORDER BY touched DESC, rowid DESC LIMIT ?2)",
            params![self.namespace, MAX_FEEDS as i64],
        )?;
        tx.execute(
            "DELETE FROM feeds WHERE touched < unixepoch() - ?1",
            [MAX_AGE_SECONDS],
        )?;
        tx.commit()?;
        Ok(())
    }
}

pub enum Event {
    Startup(Startup),
    Feed {
        id: u64,
        channel: String,
        thread: Option<String>,
        feed: Feed,
    },
    Error(String),
}

enum Job {
    Load {
        id: u64,
        channel: String,
        thread: Option<String>,
    },
    Snapshot(Snapshot),
    Select(String),
    Save {
        channel: String,
        thread: Option<String>,
        feed: Feed,
    },
    Message {
        channel: String,
        thread: Option<String>,
        message: Message,
    },
    Stop,
}

pub struct Worker {
    jobs: Sender<Job>,
    handle: Option<JoinHandle<()>>,
}

impl Worker {
    pub fn new(path: PathBuf, namespace: String, send: impl Fn(Event) + Send + 'static) -> Self {
        let (jobs, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            let mut cache = match Cache::open(&path, &namespace) {
                Ok(cache) => cache,
                Err(e) => {
                    send(Event::Error(format!("{e:#}")));
                    return;
                }
            };
            match cache.load_startup() {
                Ok(Some(startup)) => send(Event::Startup(startup)),
                Ok(None) => {}
                Err(e) => send(Event::Error(format!("{e:#}"))),
            }
            for job in rx {
                let result = match job {
                    Job::Load {
                        id,
                        channel,
                        thread,
                    } => cache.load_feed(&channel, thread.as_deref()).map(|feed| {
                        if let Some(feed) = feed {
                            send(Event::Feed {
                                id,
                                channel,
                                thread,
                                feed,
                            });
                        }
                    }),
                    Job::Snapshot(snapshot) => cache.save_snapshot(&snapshot),
                    Job::Select(channel) => cache.select_channel(&channel),
                    Job::Save {
                        channel,
                        thread,
                        feed,
                    } => cache.save_feed(&channel, thread.as_deref(), &feed),
                    Job::Message {
                        channel,
                        thread,
                        message,
                    } => (|| {
                        let mut feed = cache
                            .load_feed(&channel, thread.as_deref())?
                            .unwrap_or_default();
                        feed.messages.retain(|m| m.ts != message.ts);
                        feed.messages.push(message);
                        feed.messages.sort_by(|a, b| a.ts.cmp(&b.ts));
                        cache.save_feed(&channel, thread.as_deref(), &feed)
                    })(),
                    Job::Stop => break,
                };
                if let Err(e) = result {
                    send(Event::Error(format!("{e:#}")));
                }
            }
        });
        Self {
            jobs,
            handle: Some(handle),
        }
    }

    pub fn load(&self, id: u64, channel: String, thread: Option<String>) {
        let _ = self.jobs.send(Job::Load {
            id,
            channel,
            thread,
        });
    }
    pub fn snapshot(&self, snapshot: Snapshot) {
        let _ = self.jobs.send(Job::Snapshot(snapshot));
    }
    pub fn select(&self, channel: String) {
        let _ = self.jobs.send(Job::Select(channel));
    }
    pub fn save(&self, channel: String, thread: Option<String>, feed: Feed) {
        let _ = self.jobs.send(Job::Save {
            channel,
            thread,
            feed,
        });
    }
    pub fn message(&self, channel: String, thread: Option<String>, message: Message) {
        let _ = self.jobs.send(Job::Message {
            channel,
            thread,
            message,
        });
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.jobs.send(Job::Stop);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
