use base64::{Engine, engine::general_purpose::STANDARD};
use slacker::{
    api::decode_messages,
    app::{Editor, fuzzy_score, safe_text},
    media::transmit,
};

#[test]
fn editor_preserves_unicode_when_editing_in_the_middle() {
    let mut edit = Editor::default();
    edit.insert("hi 🦀!");
    edit.left();
    edit.backspace();
    edit.insert("世界");
    assert_eq!(edit.text, "hi 世界!");
    assert_eq!(edit.cursor, "hi 世界".len());
}

#[test]
fn switcher_matches_subsequences_case_insensitively() {
    assert!(fuzzy_score("eng", "Engineering").is_some());
    assert!(fuzzy_score("eg", "engineering").is_some());
    assert!(fuzzy_score("xyz", "engineering").is_none());
    assert!(
        fuzzy_score("eng", "engineering").unwrap()
            < fuzzy_score("eng", "evening-gathering").unwrap()
    );
}

#[test]
fn remote_text_cannot_inject_terminal_control_sequences() {
    assert_eq!(
        safe_text("hey\x1b]52;c;secret\x07\rthere\nnext\tcell"),
        "hey]52;c;secretthere\nnext    cell"
    );
}

#[test]
fn slack_errors_are_not_mistaken_for_empty_history() {
    let error =
        decode_messages(serde_json::json!({"ok":false,"error":"missing_scope"})).unwrap_err();
    assert!(error.to_string().contains("missing_scope"));
}

#[test]
fn history_is_ordered_oldest_first_and_preserves_missing_user_messages() {
    let messages = decode_messages(serde_json::json!({"ok":true,"messages":[
        {"ts":"1731000001.000010","text":"new"},
        {"ts":"1731000000.000009","text":"old","bot_id":"B123"}
    ]}))
    .unwrap();
    assert_eq!(
        messages.iter().map(|m| m.text.as_str()).collect::<Vec<_>>(),
        ["old", "new"]
    );
}

#[test]
fn kitty_chunks_reassemble_original_pixels_and_are_bounded() {
    let pixels = vec![83; 32 * 64 * 3];
    let mut output = Vec::new();
    transmit(&mut output, &pixels, 32, 64, 4, 8).unwrap();
    let wire = String::from_utf8(output).unwrap();
    let mut encoded = String::new();
    let chunks: Vec<_> = wire.split("\x1b_G").skip(1).collect();
    assert!(chunks.len() > 1);
    assert!(chunks[0].starts_with("a=T,f=24,s=32,v=64,i=31,c=4,r=8,C=1,q=2,m=1;"));
    for (i, chunk) in chunks.iter().enumerate() {
        let (header, data) = chunk.split_once(';').unwrap();
        let data = data.strip_suffix("\x1b\\").unwrap();
        assert!(data.len() <= 4096);
        assert!(header.ends_with(if i + 1 == chunks.len() { "m=0" } else { "m=1" }));
        encoded.push_str(data);
    }
    assert_eq!(STANDARD.decode(encoded).unwrap(), pixels);
}
