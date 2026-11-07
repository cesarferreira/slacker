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
