//! The binary as installers, scripts and the PKGBUILD call it.

use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    let home = std::env::temp_dir().join(format!("sm-cli-{}", std::process::id()));
    Command::new(env!("CARGO_BIN_EXE_system-manager"))
        .args(args)
        .env("SM_HOME", &home)
        .output()
        .unwrap()
}

#[test]
fn answers_its_version_and_help_and_refuses_what_it_does_not_know() {
    let version = run(&["--version"]);
    assert!(version.status.success());
    let text = String::from_utf8_lossy(&version.stdout);
    assert!(
        text.starts_with("system-manager ") && text.ends_with('\n'),
        "{text}"
    );

    let help = run(&["--help"]);
    assert!(String::from_utf8_lossy(&help.stdout).contains("notify [--new]"));

    for args in [&["neews"][..], &["check", "--new"]] {
        let refused = run(args);
        assert_eq!(refused.status.code(), Some(64), "{args:?}");
        assert!(String::from_utf8_lossy(&refused.stderr).starts_with("system-manager: "));
    }
}
