use std::{collections::HashMap, time::Duration};

use anyhow::{Context, Result, bail};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
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
    pub is_shared: bool,
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

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
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

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Reaction {
    pub name: String,
    pub count: usize,
    #[serde(default)]
    pub users: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct PreviewField {
    #[serde(default, deserialize_with = "null_default")]
    pub title: String,
    #[serde(default, deserialize_with = "null_default")]
    pub value: String,
    #[serde(default, deserialize_with = "null_default")]
    pub short: bool,
}

fn null_default<'de, D, T>(deserializer: D) -> std::result::Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct LinkPreview {
    #[serde(default, deserialize_with = "null_default")]
    pub title: String,
    #[serde(default, deserialize_with = "null_default")]
    pub title_link: String,
    #[serde(default, deserialize_with = "null_default")]
    pub from_url: String,
    #[serde(default, deserialize_with = "null_default")]
    pub original_url: String,
    #[serde(default, deserialize_with = "null_default")]
    pub text: String,
    #[serde(default, deserialize_with = "null_default")]
    pub fallback: String,
    #[serde(default, deserialize_with = "null_default")]
    pub service_name: String,
    #[serde(default, deserialize_with = "null_default")]
    pub fields: Vec<PreviewField>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
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
    #[serde(default)]
    pub reactions: Vec<Reaction>,
    #[serde(default, deserialize_with = "null_default")]
    pub attachments: Vec<LinkPreview>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Presence {
    Active,
    Away,
    DoNotDisturb,
    #[default]
    Unknown,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Person {
    pub avatar_url: String,
    pub display_name: String,
    pub status_emoji: String,
    pub status_text: String,
    pub is_bot: bool,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Snapshot {
    pub workspace: String,
    pub me: String,
    pub conversations: Vec<Conversation>,
    pub users: HashMap<String, String>,
    pub people: HashMap<String, Person>,
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

pub fn session_api_url(value: &str) -> Result<String> {
    let mut url =
        reqwest::Url::parse(value).context("Slack returned an invalid workspace endpoint")?;
    if url.scheme() != "https"
        || !url
            .host_str()
            .is_some_and(|host| host == "slack.com" || host.ends_with(".slack.com"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some_and(|port| port != 443)
    {
        bail!("Slack returned an untrusted workspace endpoint");
    }
    url.set_path("/api/");
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string())
}

#[derive(Clone)]
pub struct Slack {
    client: Client,
    token: String,
    base_url: String,
    session: bool,
}

impl Slack {
    /// An opaque, credential-scoped namespace permits offline restore without
    /// storing tokens or risking reuse by another account. Rotation starts cold.
    pub fn cache_namespace(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut hash = Sha256::new();
        hash.update(self.base_url.as_bytes());
        hash.update([0]);
        hash.update(self.token.as_bytes());
        hash.finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

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
            session: false,
        })
    }

    pub fn with_session(token: String, cookie: String, domain: &str) -> Result<Self> {
        if domain.is_empty()
            || !domain
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            bail!("Invalid Slack workspace domain");
        }
        Self::with_session_base_url(token, cookie, format!("https://{domain}.slack.com/api/"))
    }

    pub fn with_session_base_url(token: String, cookie: String, base_url: String) -> Result<Self> {
        let url = reqwest::Url::parse(&base_url)?;
        if cookie.is_empty() || cookie.bytes().any(|b| b <= 0x20 || b >= 0x7f || b == b';') {
            bail!("Invalid Slack desktop session cookie");
        }
        let jar = std::sync::Arc::new(reqwest::cookie::Jar::default());
        let scope = if url
            .host_str()
            .is_some_and(|host| host == "slack.com" || host.ends_with(".slack.com"))
        {
            "; Domain=.slack.com; Secure"
        } else {
            ""
        };
        jar.add_cookie_str(&format!("d={cookie}; Path=/{scope}; HttpOnly"), &url);
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .user_agent("slacker/0.1")
            .cookie_provider(jar)
            .build()?;
        Ok(Self {
            client,
            token,
            base_url,
            session: true,
        })
    }

    pub fn validate(&mut self) -> Result<()> {
        let identity = self.request("auth.test", &[])?;
        if self.session
            && let Some(url) = identity["url"].as_str()
        {
            self.base_url = session_api_url(url)?;
        }
        Ok(())
    }

    fn request(&self, method: &str, params: &[(&str, &str)]) -> Result<Value> {
        let request = if self.session {
            let mut form = params.to_vec();
            form.push(("token", &self.token));
            self.client
                .post(format!("{}{method}", self.base_url))
                .form(&form)
        } else {
            self.client
                .get(format!("{}{method}", self.base_url))
                .bearer_auth(&self.token)
                .query(params)
        };
        let response = request.send().context("Could not reach Slack")?;
        Self::response(response).with_context(|| format!("Slack {method} request failed"))
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
        let mut client = self.clone();
        if self.session
            && let Some(url) = auth["url"].as_str()
        {
            client.base_url = session_api_url(url)?;
        }
        let mut users = HashMap::new();
        let mut people = HashMap::new();
        for user in client.pages("users.list", "members", &[("limit", "200")])? {
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
            people.insert(
                id.to_owned(),
                Person {
                    avatar_url: ["image_48", "image_72", "image_192", "image_32", "image_24"]
                        .iter()
                        .find_map(|key| user["profile"][key].as_str().filter(|s| !s.is_empty()))
                        .unwrap_or("")
                        .to_owned(),
                    display_name: name.to_owned(),
                    status_emoji: user["profile"]["status_emoji"]
                        .as_str()
                        .unwrap_or("")
                        .to_owned(),
                    status_text: user["profile"]["status_text"]
                        .as_str()
                        .unwrap_or("")
                        .to_owned(),
                    is_bot: user["is_bot"].as_bool().unwrap_or(false),
                },
            );
        }
        let mut conversations = Vec::new();
        let entries = if self.session {
            let boot = client.request(
                "client.userBoot",
                &[
                    ("version_all_channels", "false"),
                    ("return_all_relevant_mpdms", "true"),
                    (
                        "omit_extras",
                        "feature_usage_data,plan_info,salesforce_features",
                    ),
                ],
            )?;
            let mut entries = boot["channels"]
                .as_array()
                .context("Slack desktop boot returned no channels")?
                .clone();
            if let Some(ims) = boot["ims"].as_array() {
                entries.extend(ims.iter().cloned());
            }
            entries
        } else {
            client.pages(
                "conversations.list",
                "channels",
                &[
                    ("types", "public_channel,private_channel,im,mpim"),
                    ("exclude_archived", "true"),
                    ("limit", "200"),
                ],
            )?
        };
        for value in entries {
            if value["is_archived"].as_bool() == Some(true) {
                continue;
            }
            let mut c: Conversation = serde_json::from_value(value)?;
            if self.session {
                c.is_member = true;
            }
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
            people,
        })
    }

    /// Poll up to ten distinct visible people; callers should poll no more than once a minute.
    /// Empty IDs are ignored. Missing or unrecognized states remain unknown; any request
    /// error fetching base presence stops the batch, and callers should invalidate its
    /// cached values. One best-effort DND batch overrides only current suppression.
    pub fn presences(&self, user_ids: &[String]) -> Result<HashMap<String, Presence>> {
        let mut ids = Vec::new();
        for id in user_ids {
            if !id.is_empty() && !ids.contains(&id.as_str()) {
                ids.push(id.as_str());
            }
            if ids.len() > 10 {
                bail!("Presence lookup supports at most 10 distinct people");
            }
        }
        let mut presences = HashMap::new();
        for &id in &ids {
            let value = self.request("users.getPresence", &[("user", id)])?;
            let presence = match value["presence"].as_str() {
                Some("active") => Presence::Active,
                Some("away") => Presence::Away,
                _ => Presence::Unknown,
            };
            presences.insert(id.to_owned(), presence);
        }
        if !ids.is_empty()
            && let Ok(dnd) = self.request("dnd.teamInfo", &[("users", &ids.join(","))])
            && let Ok(now) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        {
            let now = now.as_secs();
            for id in ids {
                let state = &dnd["users"][id];
                let snoozing = (state["snooze_enabled"].as_bool() == Some(true)
                    || state["snooze_active"].as_bool() == Some(true))
                    && state["snooze_endtime"]
                        .as_u64()
                        .is_some_and(|end| now < end);
                let scheduled = state["dnd_enabled"].as_bool() == Some(true)
                    && state["next_dnd_start_ts"]
                        .as_u64()
                        .is_some_and(|start| start <= now)
                    && state["next_dnd_end_ts"]
                        .as_u64()
                        .is_some_and(|end| now < end);
                if snoozing || scheduled {
                    presences.insert(id.to_owned(), Presence::DoNotDisturb);
                }
            }
        }
        Ok(presences)
    }

    /// Fetch desktop-session unread badges in one request. Values fall back to
    /// one/zero when Slack supplies only `has_unreads`, so they are not exact counts.
    /// Missing conversation entries or unread fields remain absent (unknown).
    pub fn unread_counts(&self) -> Result<HashMap<String, usize>> {
        if !self.session {
            bail!("Workspace unread badges require Slack desktop sign-in");
        }
        let value = self.request("client.counts", &[])?;
        let mut counts = HashMap::new();
        let mut found_collection = false;
        for key in ["channels", "ims", "mpims"] {
            let Some(entries) = value.get(key) else {
                continue;
            };
            let entries = entries
                .as_array()
                .context("Slack unread response has an invalid conversation collection")?;
            found_collection = true;
            for entry in entries {
                let Some(id) = entry["id"].as_str().filter(|id| !id.is_empty()) else {
                    continue;
                };
                let count = ["unread_count_display", "unread_count"]
                    .into_iter()
                    .find_map(|key| entry[key].as_u64().and_then(|n| usize::try_from(n).ok()))
                    .or_else(|| entry["has_unreads"].as_bool().map(usize::from));
                if let Some(count) = count {
                    counts.insert(id.to_owned(), count);
                }
            }
        }
        if !found_collection {
            bail!("Slack unread response is missing conversation collections");
        }
        Ok(counts)
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
        let request = self
            .client
            .post(format!("{}chat.postMessage", self.base_url));
        let request = if self.session {
            body["token"] = json!(self.token);
            request
        } else {
            request.bearer_auth(&self.token)
        };
        let response = request
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
