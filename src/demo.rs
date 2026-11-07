use crate::{
    api::{Attachment, Message, Reaction},
    media::{Pixels, Prepared},
};
use anyhow::{Context, Result};
use std::{collections::HashMap, io::Write};

fn reaction(name: &str, count: usize) -> Reaction {
    Reaction {
        name: name.into(),
        count,
        users: Vec::new(),
    }
}
fn file(name: &str, mimetype: &str) -> Attachment {
    Attachment {
        name: name.into(),
        mimetype: mimetype.into(),
        ..Default::default()
    }
}

type Reply = (&'static str, &'static str);
type Post = (&'static str, &'static str, &'static [Reply]);

const TOPICS: [&str; 7] = [
    "general",
    "engineering",
    "design",
    "random",
    "Maya Chen",
    "Leo Rivera",
    "You",
];

fn posts(topic: &str) -> &'static [Post] {
    match topic {
        "engineering" => &[
            (
                "maya",
                "Morning! The incremental build is down to 18 seconds. Yesterday it was 43 ⚡",
                &[
                    (
                        "leo",
                        "The build cache change paid off! Cold build is still 52 seconds.",
                    ),
                    ("you", "I'll add both build timings to the release notes."),
                ],
            ),
            (
                "you",
                "Nice. Was the extra work coming from the asset scanner?",
                &[],
            ),
            (
                "leo",
                "Yep, it was hashing unchanged files. Also: the video decoder handles this alpine pan at a steady 12 fps now.",
                &[
                    (
                        "maya",
                        "The decoder isn't dropping frames on my laptop either 🎉",
                    ),
                    (
                        "you",
                        "Tested the alpine clip while resizing. Still smooth.",
                    ),
                ],
            ),
            (
                "maya",
                "The memory graph finally looks boring. My favourite kind of graph.",
                &[],
            ),
            (
                "leo",
                "One thing left before we ship: can someone check reconnect after sleep?",
                &[(
                    "you",
                    "On it. I'll test reconnect with the laptop asleep for five minutes.",
                )],
            ),
            (
                "you",
                "Reconnect passed locally. Messages arrive once, in order ✅",
                &[],
            ),
            (
                "maya",
                "Perfect. Green build, coffee refill, then release ☕",
                &[],
            ),
        ],
        "design" => &[
            (
                "maya",
                "Trying a quieter sidebar today: fewer dividers, more space around channel groups. Thoughts?",
                &[
                    ("you", "The sidebar groups are much easier to scan now."),
                    ("leo", "Yes! Keep unread channel names bold in the sidebar."),
                ],
            ),
            (
                "leo",
                "Love the breathing room. The selected channel could use a little more contrast.",
                &[],
            ),
            (
                "you",
                "What if threads stayed beside the conversation? Losing my place is the part that feels awkward.",
                &[
                    (
                        "maya",
                        "Agreed. The thread panel should keep its parent visible.",
                    ),
                    (
                        "leo",
                        "And the composer should clearly say which conversation gets the reply.",
                    ),
                ],
            ),
            (
                "maya",
                "Yes. A narrow panel on the right, with a clear close button. Escape should work too.",
                &[],
            ),
            (
                "leo",
                "Tiny detail: send should keep the cursor in the composer. I always have one more thought 😅",
                &[(
                    "you",
                    "Especially for quick follow-ups. Focus should stay where I was typing.",
                )],
            ),
            (
                "you",
                "Also checking the palette with a light terminal background this afternoon.",
                &[],
            ),
            (
                "maya",
                "Putting those into the next pass. Thanks for the sharp eyes 👀",
                &[],
            ),
        ],
        "random" => &[
            (
                "leo",
                "Weekend roll call: what is everyone doing when the laptops close?",
                &[
                    (
                        "maya",
                        "My weekend plan is a book, a bakery, and absolutely no alarms.",
                    ),
                    ("you", "Weekend hike if the rain gives us a break 🌦️"),
                ],
            ),
            (
                "you",
                "Attempting sourdough again. The starter has more meetings than I do.",
                &[],
            ),
            (
                "maya",
                "Our newest teammate has already mastered work–life balance 🐶",
                &[
                    ("leo", "Employee of the month. No contest 😂"),
                    (
                        "you",
                        "Please tell the dog I said hello. And that the nap looks excellent.",
                    ),
                ],
            ),
            (
                "leo",
                "That is the face of someone who has never opened a spreadsheet.",
                &[],
            ),
            (
                "maya",
                "Important lunch question: ramen or tacos?",
                &[(
                    "you",
                    "Tacos. The place by the station has the good salsa again 🌮",
                )],
            ),
            (
                "you",
                "Tacos now, ramen next week. A roadmap everyone can support.",
                &[],
            ),
            (
                "leo",
                "Finally, a planning meeting with unanimous approval 🌮",
                &[],
            ),
        ],
        "Maya Chen" => &[
            (
                "maya",
                "Hey! Got twenty minutes to pair on the reconnect issue after lunch?",
                &[
                    (
                        "you",
                        "Absolutely. Let's pair at 14:00; I'll bring the failing trace.",
                    ),
                    ("maya", "Perfect, I'll have the reconnect branch ready."),
                ],
            ),
            (
                "you",
                "Yep, 14:00 works. Is it the duplicate message after wake?",
                &[],
            ),
            (
                "maya",
                "That's the one. I think the old poll finishes after the new session starts.",
                &[
                    (
                        "you",
                        "That would explain the ordering. Let's compare the session IDs.",
                    ),
                    ("maya", "Good call. The log has both request IDs now."),
                ],
            ),
            (
                "you",
                "I can reproduce it by closing the laptop while history is loading.",
                &[],
            ),
            (
                "maya",
                "Nice catch. Could you keep that trace for our regression check?",
                &[(
                    "you",
                    "Saved it locally. I'll walk you through the timestamps when we pair.",
                )],
            ),
            (
                "you",
                "Done. I also added a note about cancelling the old request.",
                &[],
            ),
            ("maya", "Thanks! See you at two ☕", &[]),
        ],
        "Leo Rivera" => &[
            (
                "leo",
                "Hey, are you taking the train to the offsite? Trying to book before the fares go up.",
                &[
                    (
                        "you",
                        "Yep! The 08:42 train looks good. Want to grab seats together?",
                    ),
                    (
                        "leo",
                        "Booked the train. Carriage C, next to the snack supply ☕",
                    ),
                ],
            ),
            (
                "you",
                "Definitely. I'd rather skip driving after a long hike.",
                &[],
            ),
            (
                "leo",
                "Same. I've found a short loop from the station to the lake, about 40 minutes.",
                &[
                    ("you", "That sounds ideal for the easy walking group."),
                    ("leo", "I'll share the route and the café stop in general."),
                ],
            ),
            (
                "you",
                "Could you send me the route? I'll check the elevation before we promise it's easy 😅",
                &[],
            ),
            (
                "leo",
                "Good shout. Any chance you have a spare waterproof jacket?",
                &[(
                    "you",
                    "Yep, I'll bring the blue one. It packs into its own pocket.",
                )],
            ),
            (
                "you",
                "All sorted. I'll bring a flask too. Station coffee is a gamble.",
                &[],
            ),
            (
                "leo",
                "Excellent planning. See you by the departure board!",
                &[],
            ),
        ],
        "You" => &[
            (
                "you",
                "Notes for Monday: check the reconnect trace before standup.",
                &[
                    (
                        "you",
                        "Added the request IDs to these notes so I can find the exact wake event.",
                    ),
                    ("you", "Remember to test on a slow connection too."),
                ],
            ),
            (
                "you",
                "Small win: the build finally finishes before the kettle boils ☕",
                &[],
            ),
            (
                "you",
                "Design idea: keep thread drafts when moving between conversations.",
                &[
                    (
                        "you",
                        "Try a channel draft and two separate thread drafts, then reopen each.",
                    ),
                    (
                        "you",
                        "Also check focus after sending; the next thought should be easy to type.",
                    ),
                ],
            ),
            (
                "you",
                "Offsite packing list: waterproof, walking shoes, power bank, coffee flask.",
                &[],
            ),
            (
                "you",
                "Read later: the notes about terminal image protocols.",
                &[(
                    "you",
                    "Compare resize behaviour with a photo and a video preview.",
                )],
            ),
            ("you", "Reminder: book the train before Friday.", &[]),
            (
                "you",
                "Enough tabs for today. Pick this up after a walk 🌿",
                &[],
            ),
        ],
        _ => &[
            (
                "maya",
                "Morning team! Quick offsite check: are we still good for Friday the 21st?",
                &[
                    (
                        "leo",
                        "Friday works for me. Happy to help with offsite transport.",
                    ),
                    (
                        "you",
                        "Confirmed! I'll put the offsite details in the calendar.",
                    ),
                ],
            ),
            (
                "you",
                "Works for me. Can we leave enough time for a walk before dinner?",
                &[],
            ),
            (
                "leo",
                "Found the spot for our next team offsite. Who's in? ⛰️",
                &[
                    (
                        "maya",
                        "That lake is beautiful. Let's pick a route with an easy option too.",
                    ),
                    ("you", "I'm in! I'll check train times and bring coffee ☕"),
                ],
            ),
            (
                "maya",
                "Absolutely. One short trail, one long trail, and zero compulsory icebreakers.",
                &[],
            ),
            (
                "you",
                "I'll collect dietary preferences today. Any strong feelings about dinner?",
                &[(
                    "leo",
                    "Something with a good vegetarian option, please. Happy to help book.",
                )],
            ),
            (
                "leo",
                "Anything warm after a day outside gets my vote 🍲",
                &[],
            ),
            (
                "maya",
                "Lovely. Travel details on Monday. Thanks everyone!",
                &[],
            ),
        ],
    }
}

pub fn messages() -> Vec<Message> {
    messages_for("general")
}

pub fn messages_for(topic: &str) -> Vec<Message> {
    let topic = topic.strip_prefix('#').unwrap_or(topic);
    let channel = TOPICS.iter().position(|name| *name == topic).unwrap_or(0);
    posts(TOPICS[channel])
        .iter()
        .enumerate()
        .map(|(index, (user, text, replies))| {
            let files = match (channel, index) {
                (0, 2) => vec![file("alpine-lake.jpg", "image/jpeg")],
                (1, 2) => vec![file("weekend-preview.mp4", "video/mp4")],
                (3, 2) => vec![file("office-dog.jpg", "image/jpeg")],
                _ => Vec::new(),
            };
            let reactions = match (channel, index) {
                (0, 0) => vec![reaction("+1", 4)],
                (0, 2) => vec![reaction("heart", 6), reaction("eyes", 3)],
                (1, 0) => vec![reaction("rocket", 4), reaction("fire", 2)],
                (1, 2) => vec![reaction("tada", 3)],
                (2, 0) => vec![reaction("eyes", 2)],
                (2, 4) => vec![reaction("+1", 5)],
                (3, 2) => vec![reaction("heart", 8), reaction("joy", 3)],
                (3, 6) => vec![reaction("joy", 2)],
                (4, 6) => vec![reaction("+1", 1)],
                (5, 0) => vec![reaction("white_check_mark", 1)],
                (6, 1) => vec![reaction("coffee", 1)],
                _ => Vec::new(),
            };
            Message {
                ts: format!("{}.000001", 1_731_000_000 + channel * 10_000 + index * 60),
                user: (*user).into(),
                text: (*text).into(),
                files,
                reactions,
                reply_count: replies.len(),
                ..Default::default()
            }
        })
        .collect()
}

pub fn thread(parent: Message) -> Vec<Message> {
    let seconds = parent
        .ts
        .split('.')
        .next()
        .and_then(|ts| ts.parse::<u64>().ok())
        .unwrap_or(0);
    let offset = seconds.checked_sub(1_731_000_000);
    let replies = offset.and_then(|offset| {
        let channel = usize::try_from(offset / 10_000).ok()?;
        let index = usize::try_from((offset % 10_000) / 60).ok()?;
        if offset % 10_000 % 60 != 0 {
            return None;
        }
        posts(TOPICS.get(channel)?).get(index).map(|post| post.2)
    });
    let reply_count = parent.reply_count;
    let mut messages = vec![parent];
    for index in 0..reply_count {
        let (user, text) = replies
            .and_then(|items| items.get(index))
            .copied()
            .unwrap_or((
                "you",
                "Thanks for starting this conversation. I'll follow up here.",
            ));
        messages.push(Message {
            ts: format!("{}.000001", seconds + index as u64 + 1),
            user: user.into(),
            text: text.into(),
            reactions: if index == 0 {
                vec![reaction("+1", 1)]
            } else {
                Vec::new()
            },
            ..Default::default()
        });
    }
    messages
}

fn asset(name: &str) -> Option<&'static [u8]> {
    match name {
        "alpine-lake.jpg" => Some(include_bytes!("../assets/demo/alpine-lake.jpg")),
        "office-dog.jpg" => Some(include_bytes!("../assets/demo/office-dog.jpg")),
        "weekend-preview.mp4" => Some(include_bytes!("../assets/demo/weekend-preview.mp4")),
        _ => None,
    }
}

pub fn previews() -> HashMap<String, Pixels> {
    ["alpine-lake.jpg", "office-dog.jpg", "weekend-preview.mp4"]
        .into_iter()
        .filter_map(|name| {
            let poster = if name.ends_with("mp4") {
                "alpine-lake.jpg"
            } else {
                name
            };
            let image = image::load_from_memory(asset(poster)?)
                .ok()?
                .thumbnail(640, 360)
                .to_rgb8();
            Some((
                name.to_owned(),
                Pixels {
                    width: image.width(),
                    height: image.height(),
                    rgb: image.into_raw(),
                },
            ))
        })
        .collect()
}

pub fn prepare(file: &Attachment) -> Result<Prepared> {
    let bytes = asset(&file.name).context("This attachment is not bundled with the demo")?;
    let mut temporary = tempfile::NamedTempFile::new()?;
    temporary.write_all(bytes)?;
    let mut prepared = Prepared::local(temporary.path().to_path_buf(), file.is_video())?;
    prepared.title = file.name.clone();
    prepared.temporary = Some(temporary);
    Ok(prepared)
}

pub fn emoji(name: &str) -> String {
    match name {
        "+1" | "thumbsup" => "👍",
        "heart" => "❤️",
        "rocket" => "🚀",
        "eyes" => "👀",
        "tada" => "🎉",
        "fire" => "🔥",
        "joy" => "😂",
        "white_check_mark" => "✅",
        "clap" => "👏",
        "whale" => "🐳",
        "coffee" => "☕",
        "speech_balloon" => "💬",
        "no_entry" => "⛔",
        "palm_tree" => "🌴",
        "house" => "🏠",
        "headphones" => "🎧",
        _ => return format!(":{}:", crate::app::safe_text(name)),
    }
    .to_owned()
}
