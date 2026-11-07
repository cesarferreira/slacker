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

pub fn messages() -> Vec<Message> {
    vec![
        Message {
            ts: "1731000000.000001".into(),
            user: "maya".into(),
            text: "Friday build is looking good. A little preview before the weekend 🚀".into(),
            files: vec![file("weekend-preview.mp4", "video/mp4")],
            reactions: vec![reaction("rocket", 4), reaction("fire", 2)],
            reply_count: 2,
            ..Default::default()
        },
        Message {
            ts: "1731000060.000001".into(),
            user: "leo".into(),
            text: "Found the spot for our next team offsite. Who's in? ⛰️".into(),
            files: vec![file("alpine-lake.jpg", "image/jpeg")],
            reactions: vec![reaction("heart", 6), reaction("eyes", 3)],
            reply_count: 2,
            ..Default::default()
        },
        Message {
            ts: "1731000120.000001".into(),
            user: "maya".into(),
            text: "Our newest teammate has already mastered work–life balance 🐶".into(),
            files: vec![file("office-dog.jpg", "image/jpeg")],
            reactions: vec![reaction("heart", 8), reaction("joy", 3)],
            reply_count: 2,
            ..Default::default()
        },
    ]
}

pub fn thread(parent: Message) -> Vec<Message> {
    let seconds = parent
        .ts
        .split('.')
        .next()
        .and_then(|ts| ts.parse::<u64>().ok())
        .unwrap_or(0);
    let (first, second) = if parent.files.iter().any(|f| f.is_video()) {
        (
            "That looks so smooth! Nice work 🎉",
            "Ready to ship. Have a great weekend everyone 🚀",
        )
    } else if parent.files.iter().any(|f| f.name == "office-dog.jpg") {
        (
            "Employee of the month. No contest 😂",
            "Adding more dog-friendly offsites to the roadmap 🐾",
        )
    } else {
        (
            "I'm in! That view is unreal 😍",
            "Same here. I'll bring coffee ☕",
        )
    };
    vec![
        parent,
        Message {
            ts: format!("{}.000001", seconds + 1),
            user: "leo".into(),
            text: first.into(),
            reactions: vec![reaction("+1", 2)],
            ..Default::default()
        },
        Message {
            ts: format!("{}.000001", seconds + 2),
            user: "you".into(),
            text: second.into(),
            reactions: vec![reaction("heart", 1)],
            ..Default::default()
        },
    ]
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
        _ => return format!(":{}:", crate::app::safe_text(name)),
    }
    .to_owned()
}
