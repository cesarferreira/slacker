use crate::app::safe_text;
use anyhow::{Context, Result, bail};
use ratatui::{
    style::Style,
    text::{Line, Span},
};
use std::{
    collections::HashMap,
    ops::Range,
    process::{Command, Stdio},
};
use unicode_width::UnicodeWidthStr;

pub fn valid_url(value: &str) -> Option<&str> {
    if value.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return None;
    }
    let url = reqwest::Url::parse(value).ok()?;
    match url.scheme() {
        "http" | "https" if url.host_str().is_some() => Some(value),
        "mailto" if !url.path().is_empty() => Some(value),
        _ => None,
    }
}

fn entities(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn plain(text: &str, users: &HashMap<String, String>) -> String {
    // Resolve only IDs that actually occur in the text. Scanning the workspace's
    // entire user directory per fragment makes redraw cost scale with unrelated users.
    let mut rendered = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<@") {
        rendered.push_str(&rest[..start]);
        let tail = &rest[start + 2..];
        let Some(end) = tail.find(['<', '>']) else {
            rendered.push_str(&rest[start..]);
            rest = "";
            break;
        };
        if tail.as_bytes()[end] == b'>' {
            if let Some(name) = users.get(&tail[..end]) {
                rendered.push('@');
                rendered.push_str(name);
            } else {
                rendered.push_str("<@");
                rendered.push_str(&tail[..end + 1]);
            }
            rest = &tail[end + 1..];
        } else {
            rendered.push_str("<@");
            rendered.push_str(&tail[..end]);
            rest = &tail[end..];
        }
    }
    rendered.push_str(rest);
    safe_text(&entities(&rendered))
}

pub struct Link {
    pub range: Range<usize>,
    pub url: String,
}

#[derive(Default)]
pub struct Formatted {
    pub text: String,
    pub links: Vec<Link>,
}

impl Formatted {
    pub fn link(label: String, url: &str) -> Self {
        let links = valid_url(url)
            .map(|url| Link {
                range: 0..label.len(),
                url: url.into(),
            })
            .into_iter()
            .collect();
        Self { text: label, links }
    }

    fn append_plain(&mut self, text: &str, users: &HashMap<String, String>) {
        let text = plain(text, users);
        let base = self.text.len();
        let mut rest = text.as_str();
        let mut offset = 0;
        while let Some(start) = ["https://", "http://", "mailto:"]
            .iter()
            .filter_map(|scheme| rest.find(scheme))
            .min()
        {
            let candidate = &rest[start..];
            let end = candidate
                .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '`' | '|'))
                .unwrap_or(candidate.len());
            let mut url = candidate[..end].trim_end_matches(['.', ',', ';', '!', '?', ':']);
            // Punctuation enclosing a URL is not part of it; balanced URL parentheses are.
            while let Some(close) = url.chars().last().filter(|c| matches!(c, ')' | ']' | '}')) {
                let open = match close {
                    ')' => '(',
                    ']' => '[',
                    _ => '{',
                };
                if url.matches(close).count() <= url.matches(open).count() {
                    break;
                }
                url = &url[..url.len() - close.len_utf8()];
            }
            if let Some(url) = valid_url(url) {
                self.links.push(Link {
                    range: base + offset + start..base + offset + start + url.len(),
                    url: url.into(),
                });
            }
            let consumed = start + end.max(1);
            offset += consumed;
            rest = &rest[consumed..];
        }
        self.text.push_str(&text);
    }
}

/// Parse Slack angle-bracket links before decoding escaped literal brackets.
pub fn parse(text: &str, users: &HashMap<String, String>) -> Formatted {
    let mut result = Formatted::default();
    let mut scan = 0;
    let mut pending = 0;
    while let Some(start) = text[scan..].find('<').map(|i| i + scan) {
        scan = start + 1;
        let Some(end) = text[scan..].find('>').map(|i| i + scan) else {
            break;
        };
        let contents = &text[scan..end];
        let (target, label) = contents.split_once('|').unwrap_or((contents, ""));
        let target = entities(target);
        if contents.contains('<') || valid_url(&target).is_none() {
            continue;
        }
        result.append_plain(&text[pending..start], users);
        let start = result.text.len();
        let label = plain(label, users);
        if label.is_empty() || label == target {
            result.text.push_str(&target);
        } else {
            result.text.push_str(&label);
            result.text.push_str(" (");
            result.text.push_str(&target);
            result.text.push(')');
        }
        result.links.push(Link {
            range: start..result.text.len(),
            url: target,
        });
        scan = end + 1;
        pending = scan;
    }
    result.append_plain(&text[pending..], users);
    result
}

pub struct Fragment {
    pub row: usize,
    pub column: u16,
    pub width: u16,
    pub url: String,
}

#[derive(Default)]
pub struct Wrapped {
    pub lines: Vec<Line<'static>>,
    pub links: Vec<Fragment>,
}

pub fn wrap(formatted: &Formatted, width: usize, style: Style) -> Wrapped {
    let mut result = Wrapped::default();
    let mut base = 0;
    for paragraph in formatted.text.split('\n') {
        let mut consumed = 0;
        for wrapped in textwrap::wrap(paragraph, width.max(1)) {
            let value = wrapped.as_ref();
            let local = paragraph[consumed..]
                .find(value)
                .map(|i| i + consumed)
                .unwrap_or(consumed);
            let start = base + local;
            let end = start + value.len();
            let mut position = start;
            let mut spans = Vec::new();
            for link in &formatted.links {
                let from = start.max(link.range.start);
                let to = end.min(link.range.end);
                if from >= to {
                    continue;
                }
                if from > position {
                    spans.push(Span::raw(formatted.text[position..from].to_owned()));
                }
                spans.push(Span::styled(formatted.text[from..to].to_owned(), style));
                result.links.push(Fragment {
                    row: result.lines.len(),
                    column: formatted.text[start..from].width() as u16,
                    width: formatted.text[from..to].width() as u16,
                    url: link.url.clone(),
                });
                position = to;
            }
            if position < end {
                spans.push(Span::raw(formatted.text[position..end].to_owned()));
            }
            result.lines.push(Line::from(spans));
            consumed = local + value.len();
        }
        base += paragraph.len() + 1;
    }
    result
}

/// Open only explicitly supported URL schemes, without a shell or inherited TUI IO.
pub fn open(url: &str) -> Result<()> {
    if valid_url(url).is_none() {
        bail!("Unsupported or unsafe link");
    }
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut c = Command::new("open");
        c.arg("--").arg(url);
        c
    };
    #[cfg(target_os = "linux")]
    let mut command = {
        let mut c = Command::new("xdg-open");
        c.arg(url);
        c
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut c = Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler").arg(url);
        c
    };
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    let mut command = {
        let _ = url;
        Command::new("xdg-open")
    };
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Could not open the link")?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
