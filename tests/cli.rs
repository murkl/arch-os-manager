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

/// The renewal lib.sh starts for a run hands the password, read once, to `system.sh renew` at
/// each interval while the shell of the run lives, and stops at the first refusal.
#[test]
fn renews_sudo_while_the_shell_of_the_run_lives_and_stops_at_a_refusal() {
    let dir = std::env::temp_dir().join(format!("sm-renew-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let script = |name: &str, status: i32| {
        let path = dir.join(name);
        let body = format!(
            "[ \"$1\" = renew ] || exit 2\nread -r pw\necho \"$pw\" >>\"$0.taken\"\nexit {status}\n"
        );
        std::fs::write(&path, body).unwrap();
        path
    };
    let (fine, refusing) = (script("fine.sh", 0), script("refusing.sh", 1));
    let taken = |script: &std::path::Path| {
        std::fs::read_to_string(script.with_extension("sh.taken")).unwrap_or_default()
    };
    // The shell of a run, which ends after 3.5 s.
    let shell = Command::new("bash")
        .arg("-c")
        .arg(r#""$0" sudo-renew "$$" 1 "$1" <<<pw & "$0" sudo-renew "$$" 1 "$2" <<<pw & sleep 3.5"#)
        .arg(env!("CARGO_BIN_EXE_system-manager"))
        .args([&fine, &refusing])
        .status()
        .unwrap();
    assert!(shell.success());
    std::thread::sleep(std::time::Duration::from_secs(2));
    let after = taken(&fine);
    assert!(after.lines().count() >= 2, "{after:?}");
    assert!(after.lines().all(|l| l == "pw"), "{after:?}");
    assert_eq!(taken(&refusing), "pw\n");
    std::thread::sleep(std::time::Duration::from_secs(2));
    assert_eq!(taken(&fine), after, "renewed after the shell ended");
    std::fs::remove_dir_all(&dir).unwrap();
}
