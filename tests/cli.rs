use std::process::Command;

#[test]
fn help_and_doctor_work_without_an_interactive_terminal() {
    for arg in ["--help", "--version", "doctor"] {
        let output = Command::new(env!("CARGO_BIN_EXE_slacker"))
            .arg(arg)
            .env_remove("SLACK_TOKEN")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{arg}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn demo_and_subcommands_cannot_be_combined() {
    let output = Command::new(env!("CARGO_BIN_EXE_slacker"))
        .args(["--demo", "doctor"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked"));
}

#[test]
fn ghostty_term_enables_graphics_without_term_program() {
    let output = Command::new(env!("CARGO_BIN_EXE_slacker"))
        .args(["--graphics", "auto", "doctor"])
        .env("TERM", "xterm-ghostty")
        .env_remove("TERM_PROGRAM")
        .env_remove("KITTY_WINDOW_ID")
        .env_remove("TMUX")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Kitty graphics: enabled"));
}

#[test]
fn generic_xterm_does_not_enable_kitty_graphics() {
    let output = Command::new(env!("CARGO_BIN_EXE_slacker"))
        .args(["--graphics", "auto", "doctor"])
        .env("TERM", "xterm-256color")
        .env_remove("TERM_PROGRAM")
        .env_remove("KITTY_WINDOW_ID")
        .env_remove("TMUX")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Kitty graphics: unavailable / disabled")
    );
}

#[test]
fn kitty_graphics_are_enabled_by_default_without_terminal_name_detection() {
    let output = Command::new(env!("CARGO_BIN_EXE_slacker"))
        .arg("doctor")
        .env("TERM", "xterm-256color")
        .env_remove("TERM_PROGRAM")
        .env_remove("KITTY_WINDOW_ID")
        .env_remove("TMUX")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Kitty graphics: enabled"));
}
