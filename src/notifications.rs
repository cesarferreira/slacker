use crate::api::Message;
use std::collections::{HashMap, HashSet};

pub struct Notifications {
    latest: HashMap<String, String>,
    pub unread: usize,
    focused: bool,
    bell_pending: bool,
}

impl Default for Notifications {
    fn default() -> Self {
        Self {
            latest: HashMap::new(),
            unread: 0,
            focused: true,
            bell_pending: false,
        }
    }
}

impl Notifications {
    pub fn focus(&mut self, focused: bool) {
        self.focused = focused;
        if focused {
            self.unread = 0;
            self.bell_pending = false;
        }
    }

    pub fn observe(&mut self, channel: &str, messages: &[Message], me: &str, historical: bool) {
        if historical {
            return;
        }
        let newest = messages.iter().map(|m| m.ts.as_str()).max().unwrap_or("");
        let Some(previous) = self.latest.get_mut(channel) else {
            self.latest.insert(channel.to_owned(), newest.to_owned());
            return;
        };
        let incoming: HashSet<_> = messages
            .iter()
            .filter(|message| message.ts > *previous && message.user != me)
            .map(|message| &message.ts)
            .collect();
        if !self.focused && !incoming.is_empty() {
            self.unread = self.unread.saturating_add(incoming.len());
            self.bell_pending = true;
        }
        if newest > previous.as_str() {
            *previous = newest.to_owned();
        }
    }

    pub fn take_bell(&mut self) -> bool {
        std::mem::take(&mut self.bell_pending)
    }

    pub fn title(&self, workspace: &str, conversation: &str) -> String {
        let context = format!("{conversation} · {workspace}")
            .chars()
            .filter(|c| !c.is_control())
            .collect::<String>();
        if self.unread == 0 {
            format!("slacker · {context}")
        } else {
            format!("({}) slacker · {context}", self.unread)
        }
    }
}
