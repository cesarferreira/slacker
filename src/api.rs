use std::{collections::HashMap, time::Duration};

use anyhow::{Context, Result, bail};
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Conversation {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub is_im: bool,
    #[serde(default)]
    pub is_private: bool,
    #[serde(default)]
    pub is_member: bool,
}

impl Conversation {
    pub fn label(&self) -> String {
        format!(
            "{}{}",
            if self.is_im {
                "@"
            } else if self.is_private {
                "▣ "
            } else {
                "#"
            },
            self.name
        )
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Attachment {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub mimetype: String,
    #[serde(default)]
    pub url_private: String,
    #[serde(default)]
    pub url_private_download: String,
}

impl Attachment {
    pub fn is_video(&self) -> bool {
        self.mimetype.starts_with("video/") || self.mimetype == "image/gif"
    }
    pub fn is_media(&self) -> bool {
        self.mimetype.starts_with("image/") || self.is_video()
    }
    pub fn url(&self) -> &str {
        if self.url_private_download.is_empty() {
            &self.url_private
        } else {
            &self.url_private_download
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Message {
    pub ts: String,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub reply_count: usize,
    #[serde(default)]
    pub files: Vec<Attachment>,
}

pub struct Snapshot {
    pub workspace: String,
    pub me: String,
    pub conversations: Vec<Conversation>,
    pub users: HashMap<String, String>,
}

pub struct History {
    pub messages: Vec<Message>,
    pub cursor: String,
}

pub fn check(value: Value) -> Result<Value> {
    if value.get("ok").and_then(Value::as_bool) != Some(true) {
        bail!(
            "Slack: {}",
            value
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("invalid response")
        );
    }
    Ok(value)
}

pub fn decode_messages(value: Value) -> Result<Vec<Message>> {
    let value = check(value)?;
    let mut messages: Vec<Message> = serde_json::from_value(
        value
            .get("messages")
            .cloned()
            .context("Slack response is missing messages")?,
    )?;
    messages.sort_by(|a, b| a.ts.cmp(&b.ts));
    Ok(messages)
}

#[derive(Clone)]
pub struct Slack {
    client: Client,
    token: String,
    base_url: String,
}

impl Slack {
    pub fn new(token: String) -> Result<Self> {
        Self::with_base_url(token, "https://slack.com/api/".into())
    }

    pub fn with_base_url(token: String, base_url: String) -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .timeout(Duration::from_secs(30))
                .connect_timeout(Duration::from_secs(10))
                .user_agent("slacker/0.1")
                .build()?,
            token,
            base_url,
        })
    }

    fn request(&self, method: &str, params: &[(&str, &str)]) -> Result<Value> {
        let response = self
            .client
            .get(format!("{}{method}", self.base_url))
            .bearer_auth(&self.token)
            .query(params)
            .send()
            .context("Could not reach Slack")?;
        Self::response(response)
    }

    fn response(response: reqwest::blocking::Response) -> Result<Value> {
        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("60");
            bail!("Slack rate limit; retry in {retry}s (R refreshes)");
        }
        let response = response
            .error_for_status()
            .context("Slack HTTP request failed")?;
        check(response.json().context("Slack returned invalid JSON")?)
    }

    fn pages(&self, method: &str, key: &str, params: &[(&str, &str)]) -> Result<Vec<Value>> {
        let mut all = Vec::new();
        let mut cursor = String::new();
        loop {
            let mut query = params.to_vec();
            query.push(("cursor", &cursor));
            let page = self.request(method, &query)?;
            all.extend(
                page[key]
                    .as_array()
                    .context("Slack response is missing collection")?
                    .iter()
                    .cloned(),
            );
            let next = page["response_metadata"]["next_cursor"]
                .as_str()
                .unwrap_or("");
            if next.is_empty() {
                break;
            }
            if next == cursor {
                bail!("Slack repeated a pagination cursor");
            }
            cursor = next.to_owned();
        }
        Ok(all)
    }

    pub fn bootstrap(&self) -> Result<Snapshot> {
        let auth = self.request("auth.test", &[])?;
        let mut users = HashMap::new();
        for user in self.pages("users.list", "members", &[("limit", "200")])? {
            let id = user["id"].as_str().unwrap_or("");
            let name = [
                user["profile"]["display_name"].as_str(),
                user["real_name"].as_str(),
                user["name"].as_str(),
            ]
            .into_iter()
            .flatten()
            .find(|s| !s.is_empty())
            .unwrap_or(id);
            users.insert(id.to_owned(), name.to_owned());
        }
        let mut conversations = Vec::new();
        for value in self.pages(
            "conversations.list",
            "channels",
            &[
                ("types", "public_channel,private_channel,im,mpim"),
                ("exclude_archived", "true"),
                ("limit", "200"),
            ],
        )? {
            let mut c: Conversation = serde_json::from_value(value)?;
            if !c.is_im && !c.is_member {
                continue;
            }
            if c.is_im {
                c.name = users
                    .get(&c.user)
                    .cloned()
                    .unwrap_or_else(|| c.user.clone());
            }
            conversations.push(c);
        }
        conversations.sort_by_key(|c| (c.is_im, c.name.to_lowercase()));
        Ok(Snapshot {
            workspace: auth["team"].as_str().unwrap_or("Slack").to_owned(),
            me: auth["user_id"].as_str().unwrap_or("").to_owned(),
            conversations,
            users,
        })
    }

    pub fn history(
        &self,
        channel: &str,
        thread: Option<&str>,
        cursor: &str,
        oldest: Option<&str>,
    ) -> Result<History> {
        let mut params = vec![("channel", channel), ("limit", "15"), ("cursor", cursor)];
        if let Some(ts) = thread {
            params.push(("ts", ts));
        }
        if let Some(oldest) = oldest {
            params.push(("oldest", oldest));
        }
        let value = self.request(
            if thread.is_some() {
                "conversations.replies"
            } else {
                "conversations.history"
            },
            &params,
        )?;
        let cursor = value["response_metadata"]["next_cursor"]
            .as_str()
            .unwrap_or("")
            .to_owned();
        Ok(History {
            messages: decode_messages(value)?,
            cursor,
        })
    }

    pub fn send(&self, channel: &str, thread: Option<&str>, text: &str) -> Result<Message> {
        let mut body = json!({"channel": channel, "text": text});
        if let Some(ts) = thread {
            body["thread_ts"] = json!(ts);
        }
        let response = self
            .client
            .post(format!("{}chat.postMessage", self.base_url))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .context("Could not send message; check Slack before retrying")?;
        let value = Self::response(response)?;
        let message = value
            .get("message")
            .cloned()
            .unwrap_or_else(|| json!({"ts":value["ts"],"text":text}));
        serde_json::from_value(message).context("Slack accepted the send but returned no message timestamp; check Slack before retrying")
    }

    pub fn download(&self, attachment: &Attachment) -> Result<tempfile::NamedTempFile> {
        use std::io::Read;
        let url =
            reqwest::Url::parse(attachment.url()).context("Attachment has no download URL")?;
        let host = url.host_str().unwrap_or("");
        if url.scheme() != "https"
            || !(host == "slack.com"
                || host.ends_with(".slack.com")
                || host.ends_with(".slack-edge.com"))
        {
            bail!("Attachment URL is outside Slack's HTTPS file hosts");
        }
        let response = self
            .client
            .get(url)
            .bearer_auth(&self.token)
            .send()
            .context("Could not download attachment")?
            .error_for_status()
            .context("Attachment download failed")?;
        const MAX: u64 = 50 * 1024 * 1024;
        if response.content_length().is_some_and(|len| len > MAX) {
            bail!("Attachment exceeds the 50 MiB preview limit");
        }
        let mut temp = tempfile::NamedTempFile::new()?;
        let copied = std::io::copy(&mut response.take(MAX + 1), &mut temp)?;
        if copied > MAX {
            bail!("Attachment exceeds the 50 MiB preview limit");
        }
        Ok(temp)
    }
}
