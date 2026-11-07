use slacker::demo;
use std::collections::HashSet;

#[test]
fn channels_have_distinct_conversations_and_media() {
    let mut timestamps = HashSet::new();
    let mut conversations = HashSet::new();
    for topic in [
        "general",
        "engineering",
        "design",
        "random",
        "Maya Chen",
        "Leo Rivera",
        "You",
    ] {
        let messages = demo::messages_for(topic);
        assert!((6..=9).contains(&messages.len()), "{topic}");
        assert!(conversations.insert(messages.iter().map(|m| m.text.clone()).collect::<Vec<_>>()));
        for message in &messages {
            assert!(
                timestamps.insert(message.ts.clone()),
                "shared timestamp in {topic}"
            );
            for attachment in &message.files {
                if attachment.name == "office-dog.jpg" {
                    assert_eq!(topic, "random");
                }
                if attachment.is_video() {
                    assert_eq!(topic, "engineering");
                    assert!(message.text.contains("decoder"));
                }
            }
        }
    }
    assert!(
        demo::messages_for("general")
            .iter()
            .any(|m| m.files.iter().any(|f| f.name == "alpine-lake.jpg"))
    );
    assert!(
        demo::messages_for("random")
            .iter()
            .any(|m| m.files.iter().any(|f| f.name == "office-dog.jpg"))
    );
    assert!(
        demo::messages_for("Maya Chen")
            .iter()
            .all(|m| matches!(m.user.as_str(), "maya" | "you"))
    );
}

#[test]
fn text_threads_stay_on_their_channel_topic() {
    for (topic, keyword) in [
        ("general", "offsite"),
        ("engineering", "build"),
        ("design", "sidebar"),
        ("random", "weekend"),
        ("Maya Chen", "pair"),
        ("Leo Rivera", "train"),
        ("You", "notes"),
    ] {
        let parent = demo::messages_for(topic)
            .into_iter()
            .find(|m| m.files.is_empty() && m.reply_count > 0)
            .unwrap();
        let expected_count = parent.reply_count;
        let thread = demo::thread(parent.clone());
        assert_eq!(thread[0].ts, parent.ts);
        assert_eq!(thread.len() - 1, expected_count);
        assert!(
            thread
                .iter()
                .skip(1)
                .any(|reply| reply.text.to_lowercase().contains(keyword)),
            "{topic}: {thread:?}"
        );
        assert!(thread.iter().all(|m| !m.text.is_empty()));
    }
}

#[test]
fn all_fixture_reply_counts_match_their_threads() {
    for topic in [
        "general",
        "engineering",
        "design",
        "random",
        "Maya Chen",
        "Leo Rivera",
        "You",
    ] {
        for parent in demo::messages_for(topic) {
            let count = parent.reply_count;
            let replies = demo::thread(parent.clone());
            assert_eq!(replies.len(), count + 1, "{}", parent.text);
            assert!(replies.iter().skip(1).all(|m| m.ts > parent.ts));
        }
    }
}

#[test]
fn additional_direct_messages_keep_their_participants() {
    for (topic, other) in [("Leo Rivera", "leo"), ("You", "you")] {
        for message in demo::messages_for(topic) {
            for item in demo::thread(message) {
                assert!(
                    item.user == "you" || item.user == other,
                    "{topic}: {}",
                    item.user
                );
            }
        }
    }
}
