//! The scripts against stand-ins for the system's commands, and the read-only ones against the
//! real pacman of the machine the tests run on.

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

use system_manager::data::{packages, system::System};
use system_manager::script::{op, query};

struct Sandbox {
    dir: PathBuf,
    /// Removed with the sandbox, also where the test fails.
    _tmp: tempfile::TempDir,
}

struct Outcome {
    code: i32,
    out: String,
    err: String,
    events: String,
    calls: String,
}

impl Sandbox {
    fn new(test: &str) -> Sandbox {
        let tmp = tempfile::Builder::new()
            .prefix(&format!("sm-scripts-{test}-"))
            .tempdir()
            .unwrap();
        let dir = tmp.path().to_path_buf();
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        let sandbox = Sandbox { dir, _tmp: tmp };
        // sudo runs the command as it is: the sandbox has nothing to elevate.
        sandbox.stub("sudo", r#"if [ "$1" = -n ]; then shift; fi; exec "$@""#);
        // The machine's own container engines stay out of reach, unless a test asks for one.
        for engine in ["docker", "podman"] {
            sandbox.absent(engine);
        }
        sandbox
    }

    /// A command that is not there, as the shell answers for one, and nothing logged.
    fn absent(&self, command: &str) {
        let path = self.dir.join("bin").join(command);
        std::fs::write(&path, "#!/bin/bash\nexit 127\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// A command that logs its arguments, then runs `body`.
    fn stub(&self, command: &str, body: &str) {
        let path = self.dir.join("bin").join(command);
        let log = self.dir.join("calls");
        std::fs::write(
            &path,
            format!(
                "#!/bin/bash\necho \"{command} $*\" >>'{}'\n{body}\n",
                log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// The commands a run finds: the stand-ins first, then the machine's own.
    fn search(&self) -> String {
        format!("{}:/usr/bin", self.dir.join("bin").display())
    }

    fn run(&self, script: &str, args: &[&str], env: &[(&str, &str)]) -> Outcome {
        let scripts = concat!(env!("CARGO_MANIFEST_DIR"), "/scripts");
        let events = self.dir.join("events");
        let output = Command::new("bash")
            .arg("-c")
            .arg(r#"exec 3>"$SM_EVENTS"; exec bash --noprofile --norc "$0" "$@""#)
            .arg(format!("{scripts}/{script}/{script}.sh"))
            .args(args)
            .env("PATH", self.search())
            .env("SM_EVENTS", &events)
            .env("SM_AUR", "")
            // The machine's own Flatpak stays out of reach, unless a test asks for it.
            .env("SM_FLATPAK", "off")
            // A session answers, unless a test takes it away.
            .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/sandbox")
            .env_remove("XDG_RUNTIME_DIR")
            .env("HOME", &self.dir)
            .env("XDG_CONFIG_HOME", self.dir.join("config"))
            .env("XDG_CACHE_HOME", self.dir.join("cache"))
            // chezmoi keeps its source there, and no run may reach the machine's own.
            .env("XDG_DATA_HOME", self.dir.join(".local/share"))
            .env("XDG_STATE_HOME", self.dir.join(".local/state"))
            .envs(env.iter().copied())
            .output()
            .unwrap();
        Outcome {
            code: output.status.code().unwrap_or(-1),
            out: String::from_utf8_lossy(&output.stdout).into(),
            err: String::from_utf8_lossy(&output.stderr).into(),
            events: std::fs::read_to_string(events).unwrap_or_default(),
            calls: std::fs::read_to_string(self.dir.join("calls")).unwrap_or_default(),
        }
    }
}

#[test]
fn updates_lists_the_repositories_then_the_aur() {
    let s = Sandbox::new("updates");
    s.stub(
        "checkupdates",
        "echo 'linux-zen 7.2.8.zen1-1 -> 7.2.9.zen1-1'",
    );
    s.stub("paru", "echo 'paru-bin 2.0.0-1 -> 2.1.0-1'; exit 1");
    let o = s.run("upgrade", &["list"], &[("SM_AUR", "paru")]);
    assert_eq!(o.code, 0, "{}", o.events);
    assert_eq!(
        o.out,
        "repo\tlinux-zen\t7.2.8.zen1-1\t7.2.9.zen1-1\naur\tparu-bin\t2.0.0-1\t2.1.0-1\n"
    );
}

#[test]
fn flatpak_updates_name_both_versions_else_both_commits() {
    let s = Sandbox::new("flatpak");
    s.stub("checkupdates", "exit 2");
    s.stub(
        "flatpak",
        r#"case "$*" in
"list --columns=application,arch,branch,active,version")
    printf 'com.x.App\tx86_64\tstable\t00000000000a\t1.1\n'
    printf 'org.gnome.Platform\tx86_64\t48\t00000000000b\t\n'
    printf 'org.gnome.Platform\tx86_64\t49\t00000000000c\t\n'
    printf 'org.x.Rebuilt\tx86_64\tstable\t00000000000d\t2.0\n' ;;
"remote-ls --updates --columns=application,arch,branch,commit,version")
    printf 'com.x.App\tx86_64\tstable\t00000000001a\t1.2\n'
    printf 'org.gnome.Platform\tx86_64\t49\t00000000001c\t\n'
    printf 'org.x.Rebuilt\tx86_64\tstable\t00000000001d\t2.0\n' ;;
*) exit 1 ;;
esac"#,
    );
    let o = s.run("upgrade", &["list"], &[("SM_FLATPAK", "on")]);
    assert_eq!(o.code, 0, "{}", o.events);
    assert_eq!(
        o.out,
        "flatpak\tcom.x.App\t1.1\t1.2\n\
         flatpak\torg.gnome.Platform\t00000000000c\t00000000001c\n\
         flatpak\torg.x.Rebuilt\t00000000000d\t00000000001d\n"
    );
}

#[test]
fn updates_says_nothing_when_nothing_is_pending() {
    let s = Sandbox::new("current");
    s.stub("checkupdates", "exit 2");
    s.stub("paru", "exit 1");
    s.stub("flatpak", "exit 0");
    let o = s.run(
        "upgrade",
        &["list"],
        &[("SM_AUR", "paru"), ("SM_FLATPAK", "on")],
    );
    assert_eq!((o.code, o.out.as_str()), (0, ""));
}

#[test]
fn updates_tells_an_unreachable_server_from_a_failing_helper() {
    let s = Sandbox::new("offline");
    s.stub("checkupdates", "exit 1");
    assert_eq!(s.run("upgrade", &["list"], &[]).code, 10);
    s.stub(
        "checkupdates",
        "echo 'linux-zen 7.2.8.zen1-1 -> 7.2.9.zen1-1'",
    );
    s.stub("paru", "echo 'error: could not resolve host' >&2; exit 1");
    s.stub("flatpak", "exit 1");
    let o = s.run(
        "upgrade",
        &["list"],
        &[("SM_AUR", "paru"), ("SM_FLATPAK", "on")],
    );
    assert_eq!(o.code, 0, "{}", o.events);
    assert_eq!(
        o.out,
        "repo\tlinux-zen\t7.2.8.zen1-1\t7.2.9.zen1-1\nfailed\taur\nfailed\tflatpak\n"
    );
}

#[test]
fn updates_leave_out_what_pacman_ignores_and_name_a_branch_where_two_are_pending() {
    let s = Sandbox::new("ignored");
    s.stub("checkupdates", "exit 2");
    s.stub(
        "paru",
        "echo 'foo 1-1 -> 2-1 [ignored]'; echo 'bar 1-1 -> 2-1'; exit 0",
    );
    s.stub(
        "flatpak",
        r#"case "$1" in
list) printf 'org.gnome.Platform\tx86_64\t49\ta1\t\norg.gnome.Platform\tx86_64\t50\tb1\t\n' ;;
remote-ls) printf 'org.gnome.Platform\tx86_64\t49\ta2\t\norg.gnome.Platform\tx86_64\t50\tb2\t\n' ;;
esac"#,
    );
    let o = s.run(
        "upgrade",
        &["list"],
        &[("SM_AUR", "paru"), ("SM_FLATPAK", "on")],
    );
    assert_eq!(o.code, 0, "{}", o.events);
    assert_eq!(
        o.out,
        "aur\tbar\t1-1\t2-1\n\
         flatpak\torg.gnome.Platform//49\ta1\ta2\n\
         flatpak\torg.gnome.Platform//50\tb1\tb2\n"
    );
}

#[test]
fn nopasswd_asks_sudo_without_a_prompt_and_tells_a_needed_password_by_its_code() {
    let s = Sandbox::new("nopasswd");
    s.stub("sudo", "echo 'sudo: a password is required' >&2; exit 1");
    let o = s.run("system", &["nopasswd"], &[]);
    assert_eq!(
        (o.code, o.calls.as_str()),
        (10, "sudo --non-interactive --validate\n")
    );
    s.stub("sudo", "exit 0");
    assert_eq!(s.run("system", &["nopasswd"], &[]).code, 0);
}

#[test]
fn an_unexpected_failure_names_file_line_and_command() {
    let s = Sandbox::new("trap");
    s.stub("pacman", "exit 3");
    let o = s.run("packages", &["installed"], &[]);
    assert_eq!(o.code, 1);
    let first = o.events.lines().next().unwrap_or_default();
    let f: Vec<&str> = first.split('\t').collect();
    assert_eq!(
        f[..4],
        ["fail", "3", "packages/packages.sh", "10"],
        "{first}"
    );
}

/// A run holds shutdown and sleep off while it lasts, a query does not.
#[test]
fn a_run_holds_shutdown_off_and_a_query_does_not() {
    let s = Sandbox::new("hold");
    s.stub("systemd-inhibit", "exit 0");
    s.stub("pacman", "exit 0");
    let o = s.run("upgrade", &["run"], &[("SM_RUN", "Updating the system")]);
    assert_eq!(o.code, 0, "{}", o.events);
    // The lock is taken in the background, so its stand-in may log only after the run.
    let held = "systemd-inhibit --what=shutdown:sleep --mode=block --who=System Manager --why=Updating the system tail --pid=";
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut calls = o.calls;
    while !calls.contains(held) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(20));
        calls = std::fs::read_to_string(s.dir.join("calls")).unwrap_or_default();
    }
    assert!(calls.contains(held), "{calls}");
    let q = Sandbox::new("hold-query");
    q.stub("systemd-inhibit", "exit 0");
    let o = q.run("system", &["nopasswd"], &[]);
    assert!(!o.calls.contains("systemd-inhibit"), "{}", o.calls);
}

/// A check waits while an upgrade runs, and ends with 12 where the time runs out. The wait ends
/// with the run, though a server the run started lives on. Without a runtime folder of its own
/// nothing is waited for.
#[test]
fn a_check_waits_until_the_upgrade_has_ended() {
    let s = Sandbox::new("idle");
    let runtime = s.dir.join("runtime");
    std::fs::create_dir(&runtime).unwrap();
    let session = ("XDG_RUNTIME_DIR", runtime.to_str().unwrap());
    let (server, started, release) = (
        s.dir.join("server"),
        s.dir.join("started"),
        s.dir.join("release"),
    );
    s.stub("systemd-inhibit", "exit 1");
    s.stub("pacman", "exit 0");
    s.stub(
        "flatpak",
        &format!(
            "sleep 60 </dev/null >/dev/null 2>&1 &\necho $! >'{}'\ntouch '{}'\n\
             for _ in $(seq 200); do [ -e '{}' ] && exit 0; sleep 0.05; done\nexit 1",
            server.display(),
            started.display(),
            release.display()
        ),
    );
    std::thread::scope(|scope| {
        let upgrade = scope.spawn(|| {
            s.run(
                "upgrade",
                &["run"],
                &[
                    ("SM_FLATPAK", "on"),
                    ("SM_RUN", "Updating the system"),
                    session,
                ],
            )
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !started.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(s.run("upgrade", &["idle", "0"], &[session]).code, 12);
        assert_eq!(s.run("upgrade", &["idle", "0"], &[]).code, 0);
        std::fs::write(&release, "").unwrap();
        let o = upgrade.join().unwrap();
        assert_eq!(o.code, 0, "{}", o.events);
    });
    let o = s.run("upgrade", &["idle", "5"], &[session]);
    let pid = std::fs::read_to_string(&server).unwrap_or_default();
    Command::new("kill").arg(pid.trim()).status().unwrap();
    assert_eq!(o.code, 0, "{}", o.events);
}

/// Within a helper of lib.sh, the failure names the script's line that called it, and the
/// code of the command that failed.
#[test]
fn a_failure_in_a_helper_names_the_line_of_the_script() {
    let s = Sandbox::new("trap-helper");
    s.stub("pacman", "exit 3");
    let o = s.run("packages", &["search", "x"], &[]);
    let first = o.events.lines().next().unwrap_or_default();
    let f: Vec<&str> = first.split('\t').collect();
    assert_eq!(
        f[..4],
        ["fail", "3", "packages/packages.sh", "17"],
        "{first}"
    );
}

#[test]
fn search_without_matches_is_empty_not_an_error() {
    let s = Sandbox::new("search");
    s.stub("pacman", "exit 1");
    let o = s.run("packages", &["search", "nothing"], &[]);
    assert_eq!((o.code, o.out.as_str()), (0, ""));
}

#[test]
fn versions_come_from_the_cache_and_the_archive() {
    let s = Sandbox::new("versions");
    let cache = s.dir.join("pkg");
    std::fs::create_dir_all(&cache).unwrap();
    for f in [
        "foo-1.0-1-x86_64.pkg.tar.zst",
        "foo-1.0-1-x86_64.pkg.tar.zst.sig",
    ] {
        std::fs::write(cache.join(f), "").unwrap();
    }
    s.stub("pacman-conf", &format!("echo '{}/'", cache.display()));
    s.stub("curl", r#"echo '<a href="foo-0.9-1-x86_64.pkg.tar.zst">x</a> <a href="foo-0.9-1-x86_64.pkg.tar.zst.sig">'"#);
    let o = s.run("packages", &["versions", "foo"], &[]);
    assert_eq!(o.code, 0, "{}", o.events);
    let want = format!(
        "cache\t{}/foo-1.0-1-x86_64.pkg.tar.zst\narchive\thttps://archive.archlinux.org/packages/f/foo/foo-0.9-1-x86_64.pkg.tar.zst\n",
        cache.display()
    );
    assert_eq!(o.out, want);
    // Not in the archive is no error; out of reach is.
    s.stub("curl", "exit 22");
    assert_eq!(s.run("packages", &["versions", "foo"], &[]).code, 0);
    s.stub("curl", "exit 6");
    assert_eq!(s.run("packages", &["versions", "foo"], &[]).code, 12);
}

/// A held lock stays, a leftover goes, and the keyring package is updated before the system, the
/// keyring itself never recreated.
#[test]
fn repair_leaves_a_held_lock_alone_and_updates_the_keyring_first() {
    let s = Sandbox::new("repair");
    let db = s.dir.join("db");
    std::fs::create_dir_all(&db).unwrap();
    std::fs::write(db.join("db.lck"), "").unwrap();
    s.stub(
        "pacman-conf",
        &format!(r#"case "$1" in DBPath) echo {}/ ;; esac"#, db.display()),
    );
    s.stub("pacman", "exit 0");
    // A front end other than pacman, holding the lock open as libalpm does.
    s.stub("find", "echo /proc/42/fd/7");
    let o = s.run("repair", &["run"], &[]);
    assert_eq!(o.code, 10);
    assert!(db.join("db.lck").exists());
    assert!(!o.calls.contains("pacman "), "{}", o.calls);
    s.stub("find", "exit 1");
    let _ = std::fs::remove_file(s.dir.join("calls"));
    let o = s.run("repair", &["run"], &[]);
    assert_eq!(o.code, 0, "{}", o.events);
    assert!(!db.join("db.lck").exists(), "a lock nobody holds stays");
    let pacman: Vec<&str> = o
        .calls
        .lines()
        .filter(|l| l.starts_with("pacman "))
        .collect();
    assert_eq!(
        pacman,
        ["pacman -Sy --needed archlinux-keyring", "pacman -Su"]
    );
    assert!(!o.calls.contains("pacman-key"), "{}", o.calls);
    assert_eq!(o.events, "step\tlock\nstep\tkeyring\nstep\tupgrade\n");
}

#[test]
fn repair_keeps_the_old_keyring_until_the_new_one_is_complete() {
    let s = Sandbox::new("keyring");
    let gpg = s.dir.join("gnupg");
    std::fs::create_dir_all(&gpg).unwrap();
    std::fs::write(gpg.join("old"), "").unwrap();
    s.stub(
        "pacman-conf",
        &format!(
            r#"case "$1" in DBPath) echo {0}/ ;; GPGDir) echo {1}/ ;; esac"#,
            s.dir.display(),
            gpg.display()
        ),
    );
    s.stub(
        "pacman-key",
        r#"[ "$1" = --gpgdir ] || exit 3; mkdir -p "$2"; [ "$3" = --populate ] && exit 1; touch "$2/new""#,
    );
    assert_eq!(s.run("repair", &["keyring"], &[]).code, 1);
    assert!(gpg.join("old").exists(), "a failed rebuild left no keyring");
    assert!(!s.dir.join("gnupg.new").exists(), "the half new one stays");
    s.stub(
        "pacman-key",
        r#"[ "$1" = --gpgdir ] || exit 3; mkdir -p "$2"; touch "$2/new""#,
    );
    s.stub("gpgconf", "exit 0");
    let _ = std::fs::remove_file(s.dir.join("calls"));
    let o = s.run("repair", &["keyring"], &[]);
    assert_eq!(o.code, 0, "{}", o.events);
    assert!(gpg.join("new").exists() && !gpg.join("old").exists());
    // The new keyring's daemons end before it moves, so none answers for it from the old place.
    let populated = o.calls.find("--populate").unwrap();
    let killed = o.calls.find(&format!(
        "gpgconf --homedir {}.new --kill all",
        gpg.display()
    ));
    assert!(killed.is_some_and(|k| k > populated), "{}", o.calls);
    let left: Vec<_> = std::fs::read_dir(&s.dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("gnupg."))
        .collect();
    assert!(left.is_empty(), "{left:?}");

    let strange = Sandbox::new("keyring-strange");
    strange.stub("pacman-conf", r#"case "$1" in GPGDir) echo / ;; esac"#);
    let o = strange.run("repair", &["keyring"], &[]);
    assert_eq!(o.code, 11);
    assert!(!o.calls.contains("pacman-key"), "{}", o.calls);
}

/// Only what pacdiff lists right now is ever taken, kept or merged.
#[test]
fn a_configuration_update_is_only_ever_one_pacdiff_lists() {
    let s = Sandbox::new("pacnew");
    s.stub("pacdiff", "echo /etc/x.conf.pacnew");
    for op in ["take", "keep", "merge"] {
        for file in ["/etc/passwd", "/etc/y.conf.pacnew", "/etc/x.conf.pac"] {
            let o = s.run("pacnew", &[op, file], &[]);
            assert_eq!(o.code, 11, "{op} {file}");
            assert!(!o.calls.contains("sudo"), "{op} {file}: {}", o.calls);
        }
    }
}

#[test]
fn a_configuration_update_takes_a_snapshot_where_snapper_keeps_the_root() {
    let s = Sandbox::new("pacnew-snapshot");
    s.stub("pacdiff", "echo /etc/x.conf.pacnew");
    s.stub("snapper", "exit 0");
    s.stub("mv", "exit 0");
    let o = s.run("pacnew", &["take", "/etc/x.conf.pacnew"], &[]);
    assert_eq!(o.code, 0, "{}", o.events);
    let snapshots = std::path::Path::new("/etc/snapper/configs/root").exists();
    assert_eq!(
        o.calls
            .contains("sudo snapper -c root create --cleanup-algorithm number"),
        snapshots,
        "{}",
        o.calls
    );
    assert_eq!(o.events.contains("step\tsnapshot\n"), snapshots);
    assert!(
        o.calls
            .ends_with("mv -f -- /etc/x.conf.pacnew /etc/x.conf\n"),
        "{}",
        o.calls
    );
}

/// The check reinstalls nothing: it names the native packages with altered files for a reinstall
/// the person chooses, and an AUR package only in its output.
#[test]
fn repair_finds_the_native_packages_with_altered_files_only() {
    let s = Sandbox::new("repair-files");
    s.stub(
        "pacman",
        r#"case "$1" in
-Qkk) cat <<'EOF'
backup file: pacman: /etc/pacman.conf (SHA256 checksum mismatch)
pacman: 426 total files, 0 altered files
warning: bar: /usr/bin/bar (Size mismatch)
warning: bar: /usr/bin/bar (SHA256 checksum mismatch)
bar: 10 total files, 1 altered file
warning: baz-git: /usr/bin/baz (No such file or directory)
baz-git: 3 total files, 1 altered file
warning: filesystem: /boot (Permissions mismatch)
filesystem: 127 total files, 1 altered file
warning: systemd: /var/log/journal (GID mismatch)
warning: systemd: /usr/lib/systemd/systemd (Modification time mismatch)
systemd: 1995 total files, 2 altered files
EOF
exit 1 ;;
-Qqn) printf 'bar\nfilesystem\npacman\nsystemd\n' ;;
esac"#,
    );
    let o = s.run("repair", &["files"], &[]);
    assert_eq!(o.code, 0, "{}", o.events);
    assert!(!o.calls.contains("pacman -S"), "{}", o.calls);
    assert!(
        o.out.contains("foreign, not reinstalled: baz-git"),
        "{}",
        o.out
    );
    assert_eq!(o.events, "step\tcheck\nfound\tbar\n");

    let clean = Sandbox::new("repair-files-clean");
    clean.stub("pacman", r#"case "$1" in -Qkk) printf 'pacman: 426 total files, 0 altered files\n' ;; -Qqn) echo pacman ;; esac"#);
    let o = clean.run("repair", &["files"], &[]);
    assert!(o.code == 0 && !o.calls.contains("pacman -S"), "{}", o.calls);
    assert_eq!(o.events, "step\tcheck\n");

    // Without a single summary, pacman checked nothing.
    let refused = Sandbox::new("repair-files-refused");
    refused.stub("sudo", "echo 'sudo: a password is required' >&2; exit 1");
    assert_eq!(refused.run("repair", &["files"], &[]).code, 12);
}

/// What installs from the package lists waits while they are newer than the system, as after an
/// update that broke off, a package pacman ignores aside. Removing alone does not ask.
#[test]
fn nothing_installs_from_package_lists_newer_than_the_system() {
    let s = Sandbox::new("behind");
    let pending = s.dir.join("pending");
    s.stub(
        "pacman",
        &format!(
            r#"case "$1" in -Qu) [ -s '{0}' ] || exit 1; cat '{0}' ;; -Qq) ;; *) exit 0 ;; esac"#,
            pending.display()
        ),
    );
    let installs: [(&str, &[&str], i32); 4] = [
        ("packages", &["install", "repo", "foo"], 13),
        ("packages", &["change", "repo:foo"], 13),
        ("features", &["install", "flatpak"], 11),
        ("repair", &["reinstall", "foo"], 13),
    ];
    for (script, args, code) in installs {
        for (lists, refused) in [
            ("bar 1.0-1 -> 1.1-1\nbaz 1.0-1 -> 1.1-1 [ignored]\n", true),
            ("baz 1.0-1 -> 1.1-1 [ignored]\n", false),
            ("", false),
        ] {
            std::fs::write(&pending, lists).unwrap();
            let _ = std::fs::remove_file(s.dir.join("calls"));
            let o = s.run(script, args, &[]);
            let label = format!("{script} {args:?} with {lists:?}");
            assert_eq!(o.code, if refused { code } else { 0 }, "{label}");
            assert_eq!(
                o.calls.contains("pacman -S"),
                !refused,
                "{label}: {}",
                o.calls
            );
        }
    }
    std::fs::write(&pending, "bar 1.0-1 -> 1.1-1\n").unwrap();
    let _ = std::fs::remove_file(s.dir.join("calls"));
    let o = s.run("packages", &["change", "remove:foo"], &[]);
    assert_eq!(o.code, 0, "{}", o.events);
    assert!(!o.calls.contains("-Qu"), "{}", o.calls);
}

#[test]
fn upgrade_takes_the_repositories_first_then_the_aur_then_flatpak() {
    let s = Sandbox::new("upgrade");
    for c in ["pacman", "paru", "flatpak"] {
        s.stub(c, "exit 0");
    }
    let all = [
        ("SM_AUR", "paru"),
        ("SM_FLATPAK", "on"),
        ("SM_AUR_REVIEW", "off"),
    ];
    let o = s.run("upgrade", &["run"], &all);
    assert_eq!(o.code, 0, "{}", o.events);
    let order: Vec<&str> = o.calls.lines().filter(|l| !l.starts_with("sudo")).collect();
    assert_eq!(
        order,
        [
            "pacman -Syu",
            "paru --skipreview -Sua",
            "flatpak update --assumeyes --noninteractive"
        ]
    );
    assert_eq!(o.events, "step\trepo\nstep\taur\nstep\tflatpak\n");
    // A failing helper leaves Flatpak still to update, then says so.
    s.stub("paru", "exit 1");
    let o = s.run("upgrade", &["run"], &all);
    assert_eq!(o.code, 11, "{}", o.events);
    assert!(
        o.calls
            .ends_with("flatpak update --assumeyes --noninteractive\n"),
        "{}",
        o.calls
    );
}

/// In the terminal no tool is told yes, the AUR helper keeps its review, and every program is
/// echoed before it runs, the helper through its variable too, the script's own logic not. A
/// failure is reported as ever, and echoed as nothing.
#[test]
fn in_the_terminal_the_tools_ask_and_every_program_is_echoed() {
    let s = Sandbox::new("terminal");
    for c in ["pacman", "paru", "flatpak"] {
        s.stub(c, "exit 0");
    }
    let all = [
        ("SM_AUR", "paru"),
        ("SM_FLATPAK", "on"),
        ("SM_TERMINAL", "on"),
    ];
    let o = s.run("upgrade", &["run"], &all);
    assert_eq!(o.code, 0, "{}", o.events);
    let calls: Vec<&str> = o.calls.lines().filter(|l| !l.starts_with("sudo")).collect();
    assert_eq!(calls, ["pacman -Syu", "paru -Sua", "flatpak update"]);
    let echoed = [
        " \x1b[90m$\x1b[0m sudo pacman -Syu",
        " \x1b[90m$\x1b[0m paru -Sua",
        " \x1b[90m$\x1b[0m flatpak update",
    ];
    assert_eq!(o.err.lines().collect::<Vec<_>>(), echoed);
    s.stub("flatpak", "exit 3");
    let o = s.run("upgrade", &["run"], &all);
    assert_eq!(o.code, 1);
    assert!(
        o.events.contains("fail\t3\tupgrade/upgrade.sh\t"),
        "{}",
        o.events
    );
    assert_eq!(o.err.lines().collect::<Vec<_>>(), echoed);
}

/// Where another app store updates Flatpak, the check and the update leave it out.
#[test]
fn updates_leave_flatpak_out_unless_its_updates_are_on() {
    let s = Sandbox::new("flatpak-out");
    for c in ["pacman", "flatpak"] {
        s.stub(c, "exit 0");
    }
    s.stub("checkupdates", "exit 2");
    for handled in ["off", "all"] {
        let list = s.run("upgrade", &["list"], &[("SM_FLATPAK", handled)]);
        let run = s.run("upgrade", &["run"], &[("SM_FLATPAK", handled)]);
        assert_eq!((list.code, run.code), (0, 0), "{}", run.events);
        assert_eq!(run.events, "step\trepo\n");
    }
    let calls = std::fs::read_to_string(s.dir.join("calls")).unwrap_or_default();
    assert!(!calls.contains("flatpak"), "{calls}");
}

#[test]
fn a_notification_nobody_answers_stands_until_the_next() {
    let s = Sandbox::new("notify");
    let args = [
        "notify",
        "System update",
        "6 updates available.",
        "Update",
        "1",
    ];
    s.stub("notify-send", "exec sleep 30");
    let started = std::time::Instant::now();
    let o = s.run("desktop", &args, &[]);
    assert_eq!((o.code, o.out.as_str()), (0, "standing\n"), "{}", o.events);
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    s.stub("notify-send", "echo open");
    assert_eq!(s.run("desktop", &args, &[]).out, "chosen\topen\n");
    s.stub("notify-send", "exit 1");
    assert_eq!(s.run("desktop", &args, &[]).code, 1);
}

/// The bus and the notification server as busctl names them, and what is closed.
const BUS: &str = r#"case "$*" in
*GetId*) echo 's "bus1"' ;;
*GetNameOwner*) echo 's ":1.5"' ;;
*CloseNotification*) ;;
*) exit 1 ;;
esac"#;

/// notify-send giving the id 12 on the descriptor it is told.
const SHOWN: &str = r#"for a; do case "$a" in --id-fd=*) fd="${a#*=}" ;; esac; done
echo 12 >&"$fd""#;

#[test]
fn a_notification_closes_the_one_its_server_still_shows_first() {
    let s = Sandbox::new("notify-close-first");
    s.stub("busctl", BUS);
    s.stub("notify-send", &format!("{SHOWN}\nexec sleep 30"));
    let notify = |handle: &str| {
        let args = [
            "notify",
            "System update",
            "6 updates available.",
            "Update",
            "1",
            handle,
        ];
        let _ = std::fs::remove_file(s.dir.join("calls"));
        s.run("desktop", &args, &[])
    };
    let o = notify("bus1 :1.5 7");
    assert_eq!(o.out, "shown\tbus1 :1.5 12\nstanding\n", "{}", o.events);
    let shown = o.calls.find("notify-send").unwrap();
    assert!(
        o.calls
            .find("CloseNotification u 7")
            .is_some_and(|c| c < shown),
        "{}",
        o.calls
    );
    assert!(!o.calls.contains("--replace-id"), "{}", o.calls);
    // After a new login or a restarted shell, the old id names nothing of this one.
    for other in ["bus0 :1.5 7", "bus1 :1.4 7", ""] {
        let o = notify(other);
        assert_eq!(o.out, "shown\tbus1 :1.5 12\nstanding\n", "{other:?}");
        assert!(
            !o.calls.contains("CloseNotification"),
            "{other:?}: {}",
            o.calls
        );
    }
    // A server may answer the close of one already gone with an error.
    let refusing = BUS.replace("*CloseNotification*) ;;", "*CloseNotification*) exit 1 ;;");
    s.stub("busctl", &refusing);
    assert_eq!(notify("bus1 :1.5 7").out, "shown\tbus1 :1.5 12\nstanding\n");
    // Without a server to name, nothing is kept.
    s.stub("busctl", "exit 1");
    assert_eq!(notify("bus1 :1.5 7").out, "standing\n");
}

#[test]
fn a_notification_without_a_label_has_no_button_and_still_waits() {
    let s = Sandbox::new("notify-plain");
    s.stub("notify-send", "exec sleep 30");
    let o = s.run("desktop", &["notify", "Title", "Text", "", "1"], &[]);
    assert_eq!((o.code, o.out.as_str()), (0, "standing\n"), "{}", o.events);
    assert!(
        o.calls.contains(" --wait ") && !o.calls.contains("--action"),
        "{}",
        o.calls
    );
}

#[test]
fn a_notification_closes_only_where_its_server_still_shows_it() {
    let s = Sandbox::new("notify-close");
    s.stub("busctl", BUS);
    for (handle, closed) in [("bus1 :1.5 7", true), ("bus0 :1.5 7", false), ("", false)] {
        let _ = std::fs::remove_file(s.dir.join("calls"));
        let o = s.run("desktop", &["close", handle], &[]);
        assert_eq!(o.code, 0, "{handle:?}: {}", o.events);
        assert_eq!(
            o.calls.contains("CloseNotification u 7"),
            closed,
            "{handle:?}: {}",
            o.calls
        );
    }
}

#[test]
fn a_notification_names_the_menu_entry_for_its_icon() {
    let s = Sandbox::new("notify-entry");
    s.stub("notify-send", "");
    let o = s.run("desktop", &["notify", "Title", "Text", "Open", "1"], &[]);
    assert_eq!(o.code, 0, "{}", o.events);
    assert!(
        o.calls
            .contains(" --hint=string:desktop-entry:system-manager "),
        "{}",
        o.calls
    );
}

/// GSettings as a file of key and value, read back as it was written.
const GSETTINGS: &str = r#"store="${HOME}/gsettings"
case "$3" in
set) printf '%s\t%s\n' "$5" "$6" >>"$store" ;;
get) awk -F'\t' -v k="$5" '$1 == k { v = $2 } END { print v }' "$store" ;;
esac"#;

/// The indicator as GNOME keeps an extension this user installed.
fn indicator_installed(s: &Sandbox) -> std::path::PathBuf {
    let dir = s
        .dir
        .join("data/gnome-shell/extensions/arch-update@RaphaelRochet");
    std::fs::create_dir_all(dir.join("schemas")).unwrap();
    std::fs::write(dir.join("metadata.json"), "{}").unwrap();
    dir
}

#[test]
fn the_update_indicator_checks_and_updates_through_the_program() {
    let s = Sandbox::new("indicator");
    s.stub("systemctl", "exit 1");
    s.stub("gsettings", GSETTINGS);
    indicator_installed(&s);
    let env = [("XDG_DATA_HOME", s.dir.join("data"))];
    let env = [("XDG_DATA_HOME", env[0].1.to_str().unwrap())];
    let o = s.run(
        "desktop",
        &["apply", "/opt/it's mine/system-manager", "off", "off", "on"],
        &env,
    );
    assert_eq!(o.code, 0, "{}", o.events);
    let stored = std::fs::read_to_string(s.dir.join("gsettings")).unwrap();
    assert!(
        stored.contains("check-cmd\t\"\\\"/opt/it's mine/system-manager\\\" check\"\n"),
        "{stored}"
    );
    assert!(
        stored.contains("update-cmd\t\"\\\"/opt/it's mine/system-manager\\\" --window upgrade\"\n")
    );
    // GNOME keeps something else than it was given.
    s.stub("gsettings", r#"[ "$3" = get ] && echo "'other'"; exit 0"#);
    assert_eq!(
        s.run("desktop", &["apply", "/opt/sm", "off", "off", "on"], &env)
            .code,
        13
    );
}

#[test]
fn without_a_session_the_indicator_is_set_up_on_a_private_bus() {
    let s = Sandbox::new("bus");
    s.stub("systemctl", "exit 1");
    s.stub("gsettings", GSETTINGS);
    s.stub("dbus-run-session", r#"shift; exec "$@""#);
    indicator_installed(&s);
    let data = s.dir.join("data");
    let session = [("XDG_DATA_HOME", data.to_str().unwrap())];
    let o = s.run(
        "desktop",
        &["apply", "/opt/sm", "off", "off", "on"],
        &session,
    );
    assert!(
        o.code == 0 && !o.calls.contains("dbus-run-session"),
        "{}",
        o.calls
    );
    let chroot = [
        ("XDG_DATA_HOME", data.to_str().unwrap()),
        ("DBUS_SESSION_BUS_ADDRESS", ""),
    ];
    let o = s.run(
        "desktop",
        &["apply", "/opt/sm", "off", "off", "on"],
        &chroot,
    );
    assert_eq!(o.code, 0, "{}", o.events);
    assert!(
        o.calls
            .contains("dbus-run-session -- gsettings --schemadir"),
        "{}",
        o.calls
    );
    let stored = std::fs::read_to_string(s.dir.join("gsettings")).unwrap();
    assert!(
        stored.contains("check-cmd\t'\"/opt/sm\" check'\n"),
        "{stored}"
    );
}

/// GNOME Shell installs the indicator, after asking the person, and takes it out again. A shell
/// that does not answer leaves it as it is.
#[test]
fn the_update_indicator_comes_and_goes_through_gnome_shell() {
    let s = Sandbox::new("extension");
    s.stub("busctl", r#"echo 's "successful"'"#);
    s.stub("gnome-extensions", "exit 0");
    s.stub("gsettings", "exit 0");
    let data = s.dir.join("data");
    let env = [("XDG_DATA_HOME", data.to_str().unwrap())];
    let o = s.run("desktop", &["extension-install"], &env);
    assert_eq!(o.code, 0, "{}", o.events);
    assert_eq!(
        o.calls,
        "busctl --user --timeout=300s call org.gnome.Shell /org/gnome/Shell org.gnome.Shell.Extensions InstallRemoteExtension s arch-update@RaphaelRochet\n"
    );
    for answer in [r#"echo 's "cancelled"'"#, "exit 1"] {
        s.stub("busctl", answer);
        assert_eq!(
            s.run("desktop", &["extension-install"], &env).code,
            11,
            "{answer}"
        );
    }

    let dir = indicator_installed(&s);
    let _ = std::fs::remove_file(s.dir.join("calls"));
    let o = s.run("desktop", &["extension-remove"], &env);
    assert_eq!(o.code, 0, "{}", o.events);
    let calls: Vec<&str> = o.calls.lines().collect();
    assert_eq!(
        calls,
        [
            &format!(
                "gsettings --schemadir {}/schemas reset-recursively org.gnome.shell.extensions.arch-update",
                dir.display()
            ),
            "gnome-extensions disable arch-update@RaphaelRochet",
            "gnome-extensions uninstall arch-update@RaphaelRochet",
        ]
    );
    s.stub(
        "gnome-extensions",
        r#"[ "$1" = uninstall ] && exit 2; exit 0"#,
    );
    assert_ne!(s.run("desktop", &["extension-remove"], &env).code, 0);
    assert!(dir.exists(), "nothing goes by hand");
    std::fs::remove_dir_all(&dir).unwrap();
    // Nothing installed, nothing to do.
    assert_eq!(s.run("desktop", &["extension-remove"], &env).code, 0);
}

/// The timer comes back as a notification stops waiting for its answer, so the next replaces it.
#[test]
fn the_timer_keeps_the_time_a_notification_waits() {
    let s = Sandbox::new("desktop-every");
    s.stub("systemctl", "exit 1");
    let data = s.dir.join("data");
    let env = [("XDG_DATA_HOME", data.to_str().unwrap())];
    let timer = s
        .dir
        .join("config/systemd/user/system-manager-notify.timer");
    for interval in ["hourly", "daily", "weekly"] {
        let o = s.run("desktop", &["apply", "/x", "off", interval, "off"], &env);
        assert_eq!(o.code, 0, "{}", o.events);
        let unit = std::fs::read_to_string(&timer).unwrap();
        let span = unit
            .lines()
            .find_map(|l| l.strip_prefix("OnUnitActiveSec="))
            .unwrap();
        let (n, unit) = span.split_at(span.len() - 1);
        let per = match unit {
            "h" => 3600,
            "d" => 24 * 3600,
            "w" => 7 * 24 * 3600,
            other => panic!("{other}"),
        };
        let settings = system_manager::settings::Settings {
            notify: Some(interval.into()),
            ..Default::default()
        };
        assert_eq!(settings.every().as_secs(), n.parse::<u64>().unwrap() * per);
    }
}

#[test]
fn the_menu_entry_and_the_timer_follow_on_and_off() {
    let s = Sandbox::new("desktop");
    // No user manager answers, as in an installer's chroot: files only, nothing started.
    s.stub("systemctl", "exit 1");
    let data = s.dir.join("data");
    let config = s.dir.join("config/systemd/user");
    let env = [("XDG_DATA_HOME", data.to_str().unwrap())];
    assert_eq!(
        s.run(
            "desktop",
            &[
                "apply",
                "/opt/sm with space/system-manager",
                "on",
                "hourly",
                "off"
            ],
            &env
        )
        .code,
        0
    );
    let entry = std::fs::read_to_string(data.join("applications/system-manager.desktop")).unwrap();
    assert!(
        entry.contains("Exec=\"/opt/sm with space/system-manager\" --window\n"),
        "{entry}"
    );
    assert!(entry.contains("Icon=system-manager\n"));
    let icon = std::fs::read_to_string(data.join("icons/hicolor/scalable/apps/system-manager.svg"))
        .unwrap();
    assert!(icon.starts_with("<?xml"));
    let service = std::fs::read_to_string(config.join("system-manager-notify.service")).unwrap();
    assert!(
        service.contains("ExecStart=\"/opt/sm with space/system-manager\" notify --new\n"),
        "{service}"
    );
    let timer = config.join("system-manager-notify.timer");
    assert!(
        std::fs::read_to_string(&timer)
            .unwrap()
            .contains("\nOnUnitActiveSec=1h\n")
    );
    let wants = config.join("timers.target.wants/system-manager-notify.timer");
    assert_eq!(
        std::fs::read_link(&wants).unwrap(),
        std::path::Path::new("../system-manager-notify.timer")
    );
    assert!(
        !s.run("desktop", &["apply", "/x", "on", "hourly", "off"], &env)
            .calls
            .contains("systemctl --user start")
    );
    assert_eq!(
        s.run("desktop", &["apply", "/x", "on", "weekly", "off"], &env)
            .code,
        0
    );
    assert!(
        std::fs::read_to_string(&timer)
            .unwrap()
            .contains("\nOnUnitActiveSec=1w\n")
    );
    assert_eq!(
        s.run("desktop", &["apply", "/x", "on", "often", "off"], &env)
            .code,
        1,
        "an interval the runtime never passes is a slip of the program"
    );

    assert_eq!(
        s.run("desktop", &["apply", "/opt/sm", "off", "off", "off"], &env)
            .code,
        0
    );
    assert!(!data.join("applications/system-manager.desktop").exists());
    assert!(!config.join("system-manager-notify.timer").exists());
    assert!(std::fs::symlink_metadata(&wants).is_err());

    // A path a menu entry cannot quote is refused before anything is written.
    assert_eq!(
        s.run(
            "desktop",
            &["apply", "/opt/50%/sm", "on", "off", "off"],
            &env
        )
        .code,
        10
    );
    assert!(!data.join("applications/system-manager.desktop").exists());
}

#[test]
fn features_tell_working_missing_and_broken_apart() {
    let s = Sandbox::new("features");
    s.stub("good", "exit 0");
    s.stub("bad", "exit 1");
    s.stub("pacman", "echo 'good 1.0-1'");
    let o = s.run(
        "features",
        &[
            "probe",
            "good=good --help",
            "bad=bad --version",
            "gone=no-such-command --version",
        ],
        &[],
    );
    assert_eq!(o.out, "good\tok\t1.0-1\nbad\tbroken\ngone\tmissing\n");
}

/// A -bin helper links the libalpm of its build and breaks at pacman's next soname, so the
/// helper is built from source; a helper installed elsewhere may be any of its packages.
#[test]
fn an_aur_helper_is_built_from_source_and_removed_by_its_owner() {
    let s = Sandbox::new("helper");
    s.stub(
        "pacman",
        r#"if [ "$1" = -Qqo ]; then echo "${3##*/}-bin"; fi"#,
    );
    s.stub("paru", "");
    s.stub("curl", "");
    s.stub("tar", r#"mkdir -p "$4/paru""#);
    s.stub(
        "makepkg",
        r#"echo "in ${PWD##*/}" >>"$(dirname "$0")/../calls""#,
    );
    let o = s.run("features", &["install", "paru"], &[]);
    assert_eq!(o.code, 0, "{}{}", o.events, o.calls);
    assert!(o.calls.contains("/snapshot/paru.tar.gz\n"), "{}", o.calls);
    assert!(
        o.calls
            .contains("makepkg --syncdeps --install --rmdeps --noconfirm\nin paru\n"),
        "{}",
        o.calls
    );
    let o = s.run("features", &["remove", "paru"], &[]);
    assert_eq!(o.code, 0, "{}{}", o.events, o.calls);
    let owner = format!("pacman -Qqo -- {}\n", s.dir.join("bin/paru").display());
    assert!(
        o.calls.contains(&owner),
        "the command, not ./paru: {}",
        o.calls
    );
    assert!(
        o.calls.ends_with("pacman -Rns -- paru-bin\n"),
        "{}",
        o.calls
    );
}

/// The plan counts a package without its signature, and the run leaves the choice to paccache,
/// asked as the plan asked it.
#[test]
fn the_cache_is_trimmed_by_paccache_as_planned() {
    let s = Sandbox::new("cleanup-sig");
    let cache = s.dir.join("pkg");
    std::fs::create_dir_all(&cache).unwrap();
    let package = "foo-1.0-1-x86_64.pkg.tar.zst";
    for f in [package.to_string(), format!("{package}.sig")] {
        std::fs::write(cache.join(f), "12345").unwrap();
    }
    s.stub(
        "pacman-conf",
        &format!(
            r#"case "$1" in CacheDir) echo '{}/' ;; DBPath) echo /var/lib/pacman/ ;; esac"#,
            cache.display()
        ),
    );
    s.stub(
        "paccache",
        &format!("echo '==> Candidate packages:'; echo '{package}'; echo '{package}.sig'; echo"),
    );
    s.stub(
        "pacman",
        r#"case "$1" in -Qdtq) exit 1 ;; *) exec /usr/bin/pacman "$@" ;; esac"#,
    );
    let o = s.run("cleanup", &["plan"], &[]);
    assert_eq!(o.code, 0, "{}", o.events);
    assert_eq!(
        o.out.matches("cache\t").count(),
        1,
        "a signature is no package of its own: {}",
        o.out
    );
    let o = s.run("cleanup", &["run", "cache"], &[]);
    assert_eq!(o.code, 0, "{}", o.events);
    let trims: Vec<&str> = o.calls.lines().filter(|l| l.starts_with("sudo ")).collect();
    assert_eq!(
        trims,
        [
            "sudo paccache --remove --keep 3",
            "sudo paccache --remove --uninstalled --keep 0"
        ]
    );
}

// The read-only scripts on the real pacman, whose output the parsers are written against.

#[test]
fn reads_the_real_installed_packages() {
    let all = packages::parse_installed(&query(op::PACKAGES_INSTALLED, &[]).unwrap());
    let pacman = all
        .iter()
        .find(|p| p.name == "pacman")
        .expect("pacman is installed");
    assert!(pacman.explicit || !pacman.version.is_empty());
}

#[test]
fn reads_real_search_results_and_details() {
    let found = packages::parse_search(&query(op::PACKAGES_SEARCH, &["^pacman$"]).unwrap());
    assert!(
        found
            .iter()
            .any(|f| f.repo == "core" && f.name == "pacman" && f.installed),
        "{found:?}"
    );
    let info = packages::parse_info(&query(op::PACKAGES_INFO, &["local", "pacman"]).unwrap());
    assert_eq!(packages::field(&info, "Name"), Some("pacman"));
    assert!(packages::field(&info, "Install Reason").is_some());
}

#[test]
fn reads_the_real_system_state() {
    let s = System::parse(&query(op::SYSTEM_STATE, &[]).unwrap());
    assert!(!s.hostname.is_empty());
    assert!(!s.kernel.is_empty() && !s.pacman.is_empty());
    assert!(s.booted.is_some());
}

#[test]
fn the_old_manager_is_read_then_cleared_away_and_leaves_the_dock_to_its_successor() {
    let s = Sandbox::new("migrate");
    let home = s.dir.join("arch-os");
    std::fs::create_dir_all(home.join("bin/gum")).unwrap();
    std::fs::create_dir_all(home.join("config")).unwrap();
    std::fs::create_dir_all(home.join("database")).unwrap();
    std::fs::write(
        home.join("config/settings.conf"),
        "AUTOSTART_NOTIFY=false                # Enable update notify\nAUR_MANGER_REPO=yay-bin   # helper\n",
    )
    .unwrap();
    std::fs::write(home.join("settings.yaml"), "theme: nord\n").unwrap();
    let autostart = s.dir.join("config/autostart");
    std::fs::create_dir_all(&autostart).unwrap();
    std::fs::write(
        autostart.join("arch-os.desktop"),
        "[Desktop Entry]\nExec=bash -c 'sleep 60 && /usr/bin/arch-os notify'\n",
    )
    .unwrap();
    std::fs::write(autostart.join("other.desktop"), "[Desktop Entry]\n").unwrap();
    let h = home.to_str().unwrap();

    let o = s.run("migrate", &["read", h], &[]);
    assert_eq!(o.code, 0, "{}", o.events);
    let left: Vec<&str> = o.out.lines().filter(|l| l.starts_with("left\t")).collect();
    assert_eq!(left.len(), 4, "{}", o.out);
    assert!(o.out.contains("notify\tfalse\n") && o.out.contains("aur-helper\tyay\n"));

    s.stub(
        "gsettings",
        r#"store="${HOME}/favorites"
[ -f "$store" ] || echo "['firefox.desktop', 'arch-os.desktop']" >"$store"
case "$1" in get) cat "$store" ;; set) echo "$4" >"$store" ;; esac"#,
    );
    let o = s.run("migrate", &["clean", h], &[]);
    assert_eq!(o.code, 0, "{}", o.events);
    for gone in ["bin", "database", "config"] {
        assert!(!home.join(gone).exists(), "{gone} is left");
    }
    assert!(home.join("settings.yaml").exists());
    assert!(!autostart.join("arch-os.desktop").exists());
    assert!(autostart.join("other.desktop").exists());
    assert_eq!(
        std::fs::read_to_string(s.dir.join("favorites")).unwrap(),
        "['firefox.desktop', 'system-manager.desktop']\n"
    );
    assert_eq!(s.run("migrate", &["read", h], &[]).out, "");

    // GNOME keeps the old dock.
    std::fs::write(s.dir.join("favorites"), "['arch-os.desktop']\n").unwrap();
    s.stub(
        "gsettings",
        r#"[ "$1" = get ] && echo "['arch-os.desktop']"; exit 0"#,
    );
    assert_eq!(s.run("migrate", &["clean", h], &[]).code, 10);
}

/// A release's push to the AUR, against a repository standing in for it that holds a file the
/// release no longer has.
#[test]
fn a_release_replaces_what_the_aur_has() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    std::fs::create_dir_all(dir.join("seed")).unwrap();
    let remote = dir.join("aur/arch-os-manager.git");
    let sh = |script: &str| {
        let out = Command::new("bash")
            .arg("-c")
            .arg(script)
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .env("DIR", dir)
            .env("REMOTE", &remote)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_COUNT", "1")
            .env(
                "GIT_CONFIG_KEY_0",
                format!("url.file://{}/aur/.insteadOf", dir.display()),
            )
            .env("GIT_CONFIG_VALUE_0", "ssh://aur@aur.archlinux.org/")
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).into_owned()
            + &String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{script}\n{text}");
        text
    };
    sh(r#"set -e
        git init -q --bare --initial-branch=master "$REMOTE"
        cd "$DIR/seed" && git init -q --initial-branch=master
        touch PKGBUILD arch-os.desktop && git add -A
        git -c user.name=old -c user.email=old@example.org commit -q -m 1.9.1
        git push -q "$REMOTE" master"#);
    sh(r#"tools/aur.sh stamp 2.0.0 LICENSE "$DIR/stamped" && tools/aur.sh push "$DIR/stamped""#);
    let tree = sh(r#"git -C "$REMOTE" ls-tree --name-only master"#);
    assert_eq!(tree, ".SRCINFO\nPKGBUILD\narch-os-manager.install\n");
    let commit = sh(r#"git -C "$REMOTE" log -1 --format='%an <%ae> %s' master"#);
    assert_eq!(commit, "murkl <murkl@users.noreply.github.com> 2.0.0\n");
    assert!(sh(r#"tools/aur.sh push "$DIR/stamped""#).contains("there already"));
}

/// A release built as root, as in CI, gets what the package needs before makepkg, which builds
/// nothing without it, as a helper installs it first.
#[test]
fn a_release_is_built_once_its_dependencies_are_there() {
    let s = Sandbox::new("aur-build");
    s.stub("id", "echo 0");
    s.stub(
        "curl",
        r#"while [ "$1" != -o ]; do shift; done; echo tarball >"$2""#,
    );
    for command in ["pacman", "useradd", "chown"] {
        s.stub(command, "");
    }
    // Only the read-back runs, as the user the test has.
    s.stub(
        "runuser",
        r#"case "$*" in *--printsrcinfo*) ;; *) exit 0 ;; esac
if [ "$(/usr/bin/id -u)" -eq 0 ]; then exec /usr/bin/runuser "$@"; fi
while [ "$1" != -- ]; do shift; done
shift
exec "$@""#,
    );
    let built = s.dir.join("built");
    let out = Command::new("tools/aur.sh")
        .args(["build", "2.0.0", built.to_str().unwrap()])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("PATH", format!("{}:/usr/bin", s.dir.join("bin").display()))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let calls = std::fs::read_to_string(s.dir.join("calls")).unwrap();
    let srcinfo = std::fs::read_to_string(built.join(".SRCINFO")).unwrap();
    let needed: Vec<&str> = srcinfo
        .lines()
        .filter_map(|l| {
            l.strip_prefix("\tdepends = ")
                .or_else(|| l.strip_prefix("\tmakedepends = "))
        })
        .collect();
    assert!(needed.contains(&"pacman-contrib") && needed.contains(&"cargo"));
    let install = format!("pacman -S --needed --noconfirm -- {}\n", needed.join(" "));
    let installed = calls.find(&install);
    assert!(
        installed.is_some() && installed < calls.find("runuser -u builder"),
        "{calls}"
    );
}

#[test]
fn the_aur_tool_runs_only_its_own_commands() {
    let out = Command::new("tools/aur.sh")
        .arg("rm")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).starts_with("usage: "));
}

#[test]
fn the_icon_outdates_a_cache_another_program_left_in_the_theme() {
    let s = Sandbox::new("icon-cache");
    s.stub("systemctl", "exit 1");
    let data = s.dir.join("data");
    let env = [("XDG_DATA_HOME", data.to_str().unwrap())];
    // GTK trusts the cache while the theme is not newer than it, and a file
    // two levels down leaves the theme's time alone.
    let theme = data.join("icons/hicolor");
    std::fs::create_dir_all(theme.join("scalable/apps")).unwrap();
    let cache = theme.join("icon-theme.cache");
    std::fs::write(&cache, "").unwrap();
    let cached = std::fs::metadata(&cache).unwrap().modified().unwrap();
    let age = |older: bool| {
        let when = if older {
            cached - std::time::Duration::from_secs(60)
        } else {
            std::fs::metadata(&theme).unwrap().modified().unwrap()
        };
        std::fs::File::open(&theme)
            .unwrap()
            .set_modified(when)
            .unwrap();
        when
    };
    age(true);
    assert_eq!(
        s.run("desktop", &["apply", "/x", "on", "off", "off"], &env)
            .code,
        0
    );
    assert!(age(false) >= cached);

    // A start that changes nothing leaves the theme alone.
    let old = age(true);
    assert_eq!(
        s.run("desktop", &["apply", "/x", "on", "off", "off"], &env)
            .code,
        0
    );
    assert_eq!(age(false), old);

    age(true);
    assert_eq!(
        s.run("desktop", &["apply", "/x", "off", "off", "off"], &env)
            .code,
        0
    );
    assert!(age(false) >= cached);
}

#[test]
fn packages_reach_the_aur_through_its_interface_a_hundred_names_at_a_time() {
    let s = Sandbox::new("aur-info");
    s.stub("curl", r#"echo '{"resultcount":0,"results":[]}'"#);
    let names: Vec<String> = (0..150).map(|i| format!("p{i}")).collect();
    let mut args = vec!["aur-info"];
    args.extend(names.iter().map(String::as_str));
    let o = s.run("packages", &args, &[]);
    assert_eq!(o.code, 0, "{}", o.events);
    let requests: Vec<&str> = o.calls.lines().filter(|l| l.starts_with("curl")).collect();
    assert_eq!(requests.len(), 2, "{}", o.calls);
    assert_eq!(requests[0].matches("arg[]=").count(), 100);
    assert!(requests[1].contains("arg[]=p149") && requests[1].matches("arg[]=").count() == 50);
    assert!(
        requests.iter().all(|r| r.contains("--proto =https")),
        "through fetch"
    );
    assert_eq!(o.out.lines().filter(|l| !l.is_empty()).count(), 2);

    let o = s.run("packages", &["aur-search", "foo bar"], &[]);
    assert_eq!(o.code, 0, "{}", o.events);
    assert!(o.calls.contains("arg=foo bar"), "{}", o.calls);
    s.stub("curl", "exit 22");
    for args in [
        &["aur-search", "x"][..],
        &["aur-info", "x"],
        &["pkgbuild", "x"],
    ] {
        assert_eq!(s.run("packages", args, &[]).code, 10, "{args:?}");
    }
}

#[test]
fn packages_install_remove_and_go_back_through_pacman_or_the_helper() {
    let s = Sandbox::new("packages-change");
    s.stub("pacman", "exit 0");
    s.stub("paru", "exit 0");
    let runs: [(&[&str], &str); 4] = [
        (
            &["install", "repo", "a", "b"],
            "pacman -S --needed -- a b\n",
        ),
        (&["install", "aur", "c"], "paru --skipreview -S -- aur/c\n"),
        (&["remove", "a", "b"], "pacman -Rns -- a b\n"),
        (
            &[
                "downgrade",
                "/var/cache/pacman/pkg/a-1-1-x86_64.pkg.tar.zst",
            ],
            "pacman -U -- /var/cache/pacman/pkg/a-1-1-x86_64.pkg.tar.zst\n",
        ),
    ];
    for (args, call) in runs {
        let o = s.run(
            "packages",
            args,
            &[("SM_AUR", "paru"), ("SM_AUR_REVIEW", "off")],
        );
        assert_eq!(o.code, 0, "{args:?}: {}", o.events);
        assert!(o.calls.ends_with(call), "{args:?}: {}", o.calls);
    }
    let o = s.run("packages", &["info", "sync", "extra/a"], &[]);
    assert!(o.calls.ends_with("pacman -Si -- extra/a\n"), "{}", o.calls);
    s.stub("pacman", "exit 1");
    assert_eq!(s.run("packages", &["info", "local", "gone"], &[]).code, 11);
}

/// With its review, as by default, the helper asks the person on the terminal, which the runtime
/// is told to show. Switched off, it goes without the review and its menus.
#[test]
fn the_aur_helper_reviews_on_the_terminal_unless_switched_off() {
    let s = Sandbox::new("aur-review");
    s.stub("pacman", "exit 0");
    for (helper, skip) in [
        ("paru", "--skipreview"),
        (
            "yay",
            "--answerclean None --answerdiff None --answeredit None",
        ),
    ] {
        s.stub(helper, "exit 0");
        for (review, call, events) in [
            (None, format!("{helper} -S -- aur/c\n"), "terminal\n"),
            (Some("on"), format!("{helper} -S -- aur/c\n"), "terminal\n"),
            (Some("off"), format!("{helper} {skip} -S -- aur/c\n"), ""),
        ] {
            let mut env = vec![("SM_AUR", helper)];
            env.extend(review.map(|r| ("SM_AUR_REVIEW", r)));
            let o = s.run("packages", &["install", "aur", "c"], &env);
            assert_eq!(o.code, 0, "{helper} {review:?}: {}", o.events);
            assert!(o.calls.ends_with(&call), "{helper} {review:?}: {}", o.calls);
            assert_eq!(o.events, events, "{helper} {review:?}");
        }
    }
}

#[test]
fn a_change_installs_first_and_removes_only_what_is_still_there_by_its_own_name() {
    let s = Sandbox::new("packages-swap");
    // After the installs b is gone, and sh is only what bash provides.
    s.stub(
        "pacman",
        r#"if [ "$1" = -Qq ]; then printf '%s\n' a bash; fi"#,
    );
    s.stub("paru", "exit 0");
    let args = [
        "change",
        "repo:extra/x",
        "aur:y",
        "remove:a",
        "remove:b",
        "remove:sh",
    ];
    let o = s.run(
        "packages",
        &args,
        &[("SM_AUR", "paru"), ("SM_AUR_REVIEW", "off")],
    );
    assert_eq!(o.code, 0, "{}", o.events);
    let calls: Vec<&str> = o
        .calls
        .lines()
        .filter(|l| !l.starts_with("sudo "))
        .collect();
    assert_eq!(
        calls,
        [
            "pacman -Qu",
            "pacman -S --needed -- extra/x",
            "paru --skipreview -S -- aur/y",
            "pacman -Qq",
            "pacman -Rns -- a",
        ]
    );
    let o = s.run("packages", &["change", "repo:extra/x", "remove:b"], &[]);
    assert_eq!(o.code, 0, "{}", o.events);
    assert!(o.calls.ends_with("pacman -Qq\n"), "{}", o.calls);
}

#[test]
fn a_configuration_update_is_compared_kept_or_merged_where_meld_saved() {
    let s = Sandbox::new("pacnew-merge");
    // Where this machine keeps snapshots, none is taken of the sandbox.
    s.stub("snapper", "exit 0");
    let etc = s.dir.join("etc");
    std::fs::create_dir_all(etc.join("sudoers.d")).unwrap();
    let (current, new) = (etc.join("foo.conf"), etc.join("foo.conf.pacnew"));
    std::fs::write(&current, "a=1\n").unwrap();
    std::fs::write(&new, "a=2\n").unwrap();
    let path = |p: &std::path::Path| p.to_str().unwrap().to_string();
    s.stub(
        "pacdiff",
        &format!(
            "find '{}' -name '*.pacnew' -o -name '*.pacsave*' -o -name '*.pacorig'",
            etc.display()
        ),
    );

    let o = s.run("pacnew", &["diff", &path(&new)], &[]);
    assert_eq!(o.code, 0, "{}", o.events);
    assert!(o.out.contains("-a=1\n+a=2\n"), "{}", o.out);
    let gone = path(&etc.join("gone.conf.pacnew"));
    assert_eq!(s.run("pacnew", &["diff", &gone], &[]).code, 10);

    // Meld closed without a change: nothing happens.
    s.stub("meld", "exit 0");
    let o = s.run("pacnew", &["merge", &path(&new)], &[]);
    assert_eq!(o.code, 12, "{}{}", o.events, o.calls);
    assert!(new.exists());
    // Saved on the left: that goes into the configuration, and the update is done.
    s.stub("meld", r#"echo merged >"$1""#);
    let o = s.run("pacnew", &["merge", &path(&new)], &[]);
    assert_eq!(o.code, 0, "{}", o.events);
    assert_eq!(std::fs::read_to_string(&current).unwrap(), "merged\n");
    assert!(!new.exists());
    assert!(o.calls.contains("meld "), "{}", o.calls);
    assert!(
        o.calls
            .contains(&format!("sudo stat -c %a %u %g -- {}\n", current.display())),
        "a folder closed to the user still tells owner and mode: {}",
        o.calls
    );

    // A sudoers that does not parse never replaces the one that does.
    let (sudoers, update) = (
        etc.join("sudoers.d/wheel"),
        etc.join("sudoers.d/wheel.pacnew"),
    );
    std::fs::write(&sudoers, "%wheel ALL=(ALL:ALL) ALL\n").unwrap();
    std::fs::write(&update, "x\n").unwrap();
    s.stub("visudo", "exit 1");
    assert_eq!(s.run("pacnew", &["merge", &path(&update)], &[]).code, 13);
    assert_eq!(
        std::fs::read_to_string(&sudoers).unwrap(),
        "%wheel ALL=(ALL:ALL) ALL\n"
    );

    let o = s.run("pacnew", &["keep", &path(&update)], &[]);
    assert_eq!(o.code, 0, "{}", o.events);
    assert!(!update.exists() && sudoers.exists());
    for odd in ["foo.conf.pacsave.1", "foo.conf.pacorig"] {
        let file = etc.join(odd);
        std::fs::write(&file, "").unwrap();
        assert_eq!(
            s.run("pacnew", &["keep", &path(&file)], &[]).code,
            0,
            "{odd}"
        );
    }
    assert_eq!(s.run("pacnew", &["keep", &path(&current)], &[]).code, 11);
}

/// A service changes only as the page offers it, for the system as root, for the user without.
#[test]
fn a_service_changes_only_by_the_verbs_offered() {
    let s = Sandbox::new("services-verbs");
    s.stub("systemctl", "exit 0");
    for verb in ["start", "stop", "restart", "enable", "disable"] {
        let o = s.run("services", &["system", verb, "foo.service"], &[]);
        assert_eq!(o.code, 0, "{verb}: {}", o.events);
        assert!(
            o.calls.ends_with(&format!(
                "sudo systemctl {verb} -- foo.service\nsystemctl {verb} -- foo.service\n"
            )),
            "{verb}: {}",
            o.calls
        );
    }
    let _ = std::fs::remove_file(s.dir.join("calls"));
    for (scope, verb) in [("system", "mask"), ("user", "isolate"), ("system", "--now")] {
        let o = s.run("services", &[scope, verb, "foo.service"], &[]);
        assert_eq!(o.code, 1, "{scope} {verb}");
    }
    assert_eq!(
        std::fs::read_to_string(s.dir.join("calls")).unwrap_or_default(),
        ""
    );
    let o = s.run("services", &["user", "restart", "bar.service"], &[]);
    assert!(
        o.calls
            .ends_with("systemctl --user restart -- bar.service\n"),
        "{}",
        o.calls
    );
}

#[test]
fn the_mirror_list_names_its_servers_and_a_ranking_tells_offline_from_failed() {
    let s = Sandbox::new("mirrors");
    s.stub(
        "systemctl",
        r#"case "$*" in *ConditionResult*) echo no ;; *) exit 0 ;; esac"#,
    );
    let o = s.run("mirrors", &["list"], &[]);
    assert_eq!(o.code, 0, "{}", o.events);
    assert!(
        o.out.starts_with("changed\t") && o.out.contains("\ntimer\ton\n"),
        "{}",
        o.out
    );
    assert_eq!(
        s.run("mirrors", &["rank"], &[]).code,
        10,
        "a condition not met"
    );
    s.stub("systemctl", "exit 1");
    assert_eq!(s.run("mirrors", &["rank"], &[]).code, 11);
}

#[test]
fn firmware_with_nothing_to_update_is_no_failure_and_an_update_sends_no_report() {
    let s = Sandbox::new("firmware");
    s.stub(
        "fwupdmgr",
        r#"case "$1" in get-updates) echo '{"Devices": []}'; exit 2 ;; refresh) exit 0 ;; esac"#,
    );
    let o = s.run("firmware", &["list"], &[]);
    assert_eq!((o.code, o.out.as_str()), (0, "{\"Devices\": []}\n"));
    let o = s.run("firmware", &["update"], &[]);
    assert_eq!(o.code, 0, "{}", o.events);
    assert!(
        o.calls
            .ends_with("fwupdmgr update --assume-yes --no-reboot-check --no-unreported-check\n"),
        "{}",
        o.calls
    );
    s.stub("fwupdmgr", "exit 1");
    assert_eq!(s.run("firmware", &["update"], &[]).code, 10, "no catalogue");
}

#[test]
fn services_are_listed_logged_and_changed_in_their_scope() {
    let s = Sandbox::new("services");
    s.stub("systemctl", "echo '[]'");
    s.stub("journalctl", "exit 0");
    let o = s.run("services", &["list", "user"], &[]);
    assert_eq!((o.code, o.out.as_str()), (0, "units\t[]\nfiles\t[]\n"));
    assert!(
        o.calls.contains("systemctl --user list-units"),
        "{}",
        o.calls
    );
    // The system's names what the session needs, as systemctl shows each unit apart.
    s.stub(
        "systemctl",
        r#"case "$1" in show) printf 'gdm.service\n\ndbus-broker.service\n' ;; *) echo '[]' ;; esac"#,
    );
    let o = s.run("services", &["list", "system"], &[]);
    assert_eq!(
        (o.code, o.out.as_str()),
        (
            0,
            "units\t[]\nfiles\t[]\nsession\tgdm.service\nsession\tdbus-broker.service\n"
        )
    );
    assert!(
        o.calls.contains(
            "systemctl show --property=Id --value display-manager.service dbus.service\n"
        ),
        "{}",
        o.calls
    );
    s.stub("systemctl", "echo '[]'");
    let runs: [(&[&str], &str); 4] = [
        (
            &["log", "user", "a.service"],
            "journalctl --user-unit a.service -n 30 --output=json --all --no-pager\n",
        ),
        (
            &["log", "system", "b.service"],
            "journalctl --unit b.service -n 30 --output=json --all --no-pager\n",
        ),
        (
            &["system", "restart", "b.service"],
            "systemctl restart -- b.service\n",
        ),
        (
            &["user", "stop", "a.service"],
            "systemctl --user stop -- a.service\n",
        ),
    ];
    for (args, call) in runs {
        let o = s.run("services", args, &[]);
        assert_eq!(o.code, 0, "{args:?}: {}", o.events);
        assert!(o.calls.ends_with(call), "{args:?}: {}", o.calls);
    }
}

#[test]
fn the_news_the_log_and_a_restart_ask_their_tools() {
    let s = Sandbox::new("small");
    s.stub("curl", "echo '<rss/>'");
    s.stub("journalctl", "exit 0");
    s.stub("systemctl", "exit 0");
    let o = s.run("news", &["fetch"], &[]);
    assert_eq!((o.code, o.out.as_str()), (0, "<rss/>\n"));
    assert!(
        o.calls.contains("https://archlinux.org/feeds/news/"),
        "{}",
        o.calls
    );
    s.stub("curl", "exit 6");
    assert_eq!(s.run("news", &["fetch"], &[]).code, 10);
    let o = s.run("log", &["errors"], &[]);
    assert!(
        o.calls
            .contains("journalctl --boot --priority=3 --lines=500 --output=json --all"),
        "{}",
        o.calls
    );
    let o = s.run("system", &["reboot"], &[]);
    assert!(
        o.code == 0 && o.calls.ends_with("systemctl reboot\n"),
        "{}",
        o.calls
    );
    s.stub("sudo", "exit 1");
    assert_eq!(s.run("system", &["auth"], &[]).code, 1);
}

/// A configuration as packaged gets the last word appended: by speed, in the country of the time
/// zone where it has mirrors. One changed by hand stays as it is.
#[test]
fn reflector_ranks_by_speed_in_the_country_of_the_time_zone_where_it_has_mirrors() {
    let s = Sandbox::new("reflector");
    let conf = s.dir.join("reflector.conf");
    let packaged = "--save /etc/pacman.d/mirrorlist\n--protocol https\n--latest 5\n--sort age\n";
    // pacman tells a configuration changed by hand where the sandbox's differs from the package's.
    s.stub(
        "pacman",
        &format!(
            r#"if [ "$1" = -Qkk ] && [ "$(cat '{0}')" != '{1}' ]; then
echo 'backup file: reflector: /etc/xdg/reflector/reflector.conf (Modification time mismatch)'
echo 'backup file: reflector: /etc/xdg/reflector/reflector.conf (SHA256 checksum mismatch)'
fi"#,
            conf.display(),
            packaged.trim_end()
        ),
    );
    s.stub("systemctl", "exit 0");
    s.stub("tee", r#"cat >>"$HOME/reflector.conf""#);
    s.stub("readlink", "echo /usr/share/zoneinfo/Europe/Amsterdam");
    s.stub("reflector", "printf 'Country Code Count\\n------- ---- -----\\nNetherlands NL 30\\nUnited States US 200\\n'");
    std::fs::write(&conf, packaged).unwrap();
    let o = s.run("features", &["install", "reflector"], &[]);
    assert_eq!(o.code, 0, "{}{}", o.events, o.calls);
    assert_eq!(
        std::fs::read_to_string(&conf).unwrap(),
        format!("{packaged}\n# System Manager\n--age 12\n--latest 10\n--sort rate\n--country NL\n")
    );
    assert!(
        o.calls
            .contains("sudo tee -a /etc/xdg/reflector/reflector.conf\n"),
        "{}",
        o.calls
    );
    assert!(
        o.calls
            .ends_with("systemctl enable --now reflector.timer\n"),
        "{}",
        o.calls
    );

    // Changed by hand, by the lines before too: nothing is appended, the timer still runs.
    let changed = std::fs::read_to_string(&conf).unwrap();
    let o = s.run("features", &["install", "reflector"], &[]);
    assert_eq!(o.code, 0, "{}{}", o.events, o.calls);
    assert_eq!(std::fs::read_to_string(&conf).unwrap(), changed);
    assert!(
        o.out.contains("kept\t/etc/xdg/reflector/reflector.conf"),
        "{}",
        o.out
    );
    assert!(
        o.calls
            .ends_with("systemctl enable --now reflector.timer\n")
    );

    // A country without a mirror ranks them all.
    std::fs::write(&conf, packaged).unwrap();
    s.stub("readlink", "echo /usr/share/zoneinfo/Pacific/Pitcairn");
    assert_eq!(s.run("features", &["install", "reflector"], &[]).code, 0);
    let appended = std::fs::read_to_string(&conf).unwrap();
    assert!(
        appended.ends_with("--sort rate\n") && !appended.contains("--country"),
        "{appended}"
    );

    let o = s.run("features", &["remove", "reflector"], &[]);
    let (disabled, removed) = (
        o.calls.rfind("systemctl disable --now reflector.timer\n"),
        o.calls.rfind("pacman -Rns -- reflector\n"),
    );
    assert!(disabled.is_some() && disabled < removed, "{}", o.calls);
    let o = s.run("features", &["install", "fwupd"], &[]);
    assert!(
        o.calls
            .ends_with("systemctl enable --now fwupd-refresh.timer\n"),
        "{}",
        o.calls
    );
}

/// pacman, its cache and its configuration with nothing to clean up.
fn nothing_of_pacman(s: &Sandbox) {
    s.stub(
        "pacman-conf",
        &format!(
            r#"case "$1" in CacheDir) echo '{}/' ;; DBPath) echo /var/lib/pacman/ ;; esac"#,
            s.dir.join("pkg").display()
        ),
    );
    s.stub(
        "paccache",
        "echo '==> no candidate packages found for pruning'",
    );
    s.stub(
        "pacman",
        r#"case "$1" in -Qdtq) exit 1 ;; *) exec /usr/bin/pacman "$@" ;; esac"#,
    );
}

/// Unused Flatpak runtimes go only where the run is asked for them, and the plan leaves Flatpak
/// to the probe.
#[test]
fn cleanup_drops_unused_runtimes_only_when_asked() {
    let s = Sandbox::new("cleanup-flatpak");
    nothing_of_pacman(&s);
    s.stub("flatpak", "exit 0");
    let o = s.run("cleanup", &["plan"], &[]);
    assert_eq!(o.code, 0, "{}", o.events);
    let o = s.run("cleanup", &["run", "orphans", "cache", "aur"], &[]);
    assert_eq!(o.code, 0, "{}", o.events);
    let calls = std::fs::read_to_string(s.dir.join("calls")).unwrap_or_default();
    assert!(!calls.contains("flatpak"), "{calls}");
    let o = s.run("cleanup", &["run", "flatpak"], &[]);
    assert_eq!(o.code, 0, "{}", o.events);
    assert!(
        o.calls
            .ends_with("flatpak uninstall --unused --assumeyes --noninteractive\n"),
        "{}",
        o.calls
    );
}

/// The system's Docker is asked and cleaned up only while it runs, at the socket systemd listens
/// on, and only its unused images and build cache go, never a container or a volume. An inactive
/// Docker stays as it is.
#[test]
fn docker_is_asked_only_while_it_runs_and_keeps_containers_and_volumes() {
    let s = Sandbox::new("cleanup-docker");
    nothing_of_pacman(&s);
    let socket = s.dir.join("docker.sock");
    let units = s.dir.join("units");
    std::fs::create_dir_all(&units).unwrap();
    // Each unit a file, active where it holds `active`.
    s.stub(
        "systemctl",
        &format!(
            r#"d='{}'
case "$1" in
cat) [ -e "$d/$2" ] ;;
is-active) grep -qx active "$d/$3" 2>/dev/null ;;
show) echo "$(cat "$d/listen") (Stream)" ;;
*) exit 1 ;;
esac"#,
            units.display()
        ),
    );
    s.stub(
        "docker",
        r#"case "$*" in
--version) echo 'Docker version 29.9.1' ;;
*" system df --format json") [ ! -e "$HOME/broken" ] && echo '{"Type":"Images","Reclaimable":"1GB (50%)"}' ;;
*" system df --verbose --format json") echo '{"Images":[]}' ;;
*" image prune "*) [ ! -e "$HOME/broken" ] ;;
esac"#,
    );
    std::fs::write(&socket, "").unwrap();
    let set = |service: &str, others: &str, listen: &str| {
        for (unit, state) in [
            ("docker.service", service),
            ("docker.socket", others),
            ("containerd.service", others),
        ] {
            let path = units.join(unit);
            match state {
                "none" => drop(std::fs::remove_file(&path)),
                _ => std::fs::write(&path, format!("{state}\n")).unwrap(),
            }
        }
        std::fs::write(units.join("listen"), listen).unwrap();
    };
    let active = |unit: &str| std::fs::read_to_string(units.join(unit)).unwrap() == "active\n";
    let plan = |unit: &str, running: &str, listen: &str| {
        let service = match (unit, running) {
            ("no", _) => "none",
            (_, "yes") => "active",
            _ => "inactive",
        };
        set(service, "active", listen);
        let o = s.run("cleanup", &["plan"], &[]);
        assert_eq!(o.code, 0, "{}", o.events);
        o
    };
    let sock = socket.display().to_string();
    let gone = s.dir.join("gone.sock").display().to_string();
    assert!(!plan("no", "no", &sock).out.contains("docker"));
    let o = plan("yes", "no", &sock);
    assert!(o.out.ends_with("docker\tstopped\n"), "{}", o.out);
    assert!(
        !o.calls.contains("--host"),
        "a stopped Docker is not asked: {}",
        o.calls
    );
    assert!(plan("yes", "yes", &gone).out.ends_with("docker\tdenied\n"));
    let o = plan("yes", "yes", &sock);
    assert!(
        o.out.ends_with(concat!(
            "docker\tready\n",
            "docker-df\t{\"Type\":\"Images\",\"Reclaimable\":\"1GB (50%)\"}\n",
            "docker-du\t{\"Images\":[]}\n",
        )),
        "{}",
        o.out
    );
    assert!(
        o.calls.contains(&format!(
            "docker --host unix://{sock} system df --format json\n"
        )),
        "{}",
        o.calls
    );
    std::fs::write(s.dir.join("broken"), "").unwrap();
    assert!(plan("yes", "yes", &sock).out.ends_with("docker\tfailed\n"));
    std::fs::remove_file(s.dir.join("broken")).unwrap();

    let prune = format!(
        "sudo docker --host unix://{sock} image prune --all --force\nsudo docker --host unix://{sock} builder prune --all --force\n"
    );
    let sudo = |o: &Outcome| -> String {
        o.calls
            .lines()
            .filter(|l| l.starts_with("sudo "))
            .map(|l| format!("{l}\n"))
            .collect()
    };
    let run = || {
        let _ = std::fs::remove_file(s.dir.join("calls"));
        s.run("cleanup", &["run", "docker"], &[])
    };
    let o = s.run("cleanup", &["run", "orphans"], &[]);
    assert!(!o.calls.contains("prune"), "{}", o.calls);

    set("active", "active", &sock);
    let o = run();
    assert_eq!(
        (o.code, o.events.as_str()),
        (0, "step\tdocker\n"),
        "{}",
        o.calls
    );
    assert_eq!(sudo(&o), prune, "a running Docker is only pruned");
    let docker: Vec<&str> = o
        .calls
        .lines()
        .filter(|l| l.contains("docker --host"))
        .collect();
    assert!(
        docker
            .iter()
            .all(|l| !l.contains("volume") && !l.contains("container")),
        "{docker:?}"
    );

    // Socket activated or all of it off, nothing starts and nothing is pruned.
    for others in ["active", "inactive"] {
        set("inactive", others, &sock);
        let o = run();
        assert_eq!((o.code, o.events.as_str()), (0, ""), "{}", o.calls);
        assert_eq!(sudo(&o), "", "{}", o.calls);
        assert!(!active("docker.service"));
    }

    // Without the group docker the run still cleans up, as root.
    set("active", "active", &gone);
    let o = run();
    assert_eq!(sudo(&o), prune.replace(&sock, &gone), "{}", o.calls);
}

/// This user's Podman lists the images no container uses, and only those go.
#[test]
fn podman_removes_the_images_no_container_uses() {
    let s = Sandbox::new("cleanup-podman");
    nothing_of_pacman(&s);
    s.stub(
        "podman",
        r#"case "$*" in
--version) echo 'podman version 6.1.4' ;;
"system df --format {{json .}}") [ ! -e "$HOME/broken" ] && echo '{"Type":"Images","RawReclaimable":8715873}' ;;
"images --filter containers=false --format {{.Repository}}:{{.Tag}}{{\"\t\"}}{{.ID}}") printf 'docker.io/library/alpine:latest\t320994c3b997\n' ;;
esac"#,
    );
    let o = s.run("cleanup", &["plan"], &[]);
    assert_eq!(o.code, 0, "{}", o.events);
    assert!(
        o.out.ends_with(concat!(
            "podman\tready\n",
            "podman-df\t{\"Type\":\"Images\",\"RawReclaimable\":8715873}\n",
            "podman-image\tdocker.io/library/alpine:latest\t320994c3b997\n",
        )),
        "{}",
        o.out
    );
    let o = s.run("cleanup", &["run", "podman"], &[]);
    assert_eq!(
        (o.code, o.events.as_str()),
        (0, "step\tpodman\n"),
        "{}",
        o.calls
    );
    assert!(
        o.calls.ends_with("podman image prune --all --force\n"),
        "{}",
        o.calls
    );
    std::fs::write(s.dir.join("broken"), "").unwrap();
    let o = s.run("cleanup", &["plan"], &[]);
    assert!(o.out.ends_with("podman\tfailed\n"), "{}", o.out);
}

#[test]
fn cleanup_removes_orphans_and_the_builds_of_packages_gone() {
    let s = Sandbox::new("cleanup-builds");
    let cache = s.dir.join("pkg");
    std::fs::create_dir_all(&cache).unwrap();
    let builds = s.dir.join("cache/paru/clone");
    for b in ["gone-pkg", "pacman"] {
        std::fs::create_dir_all(builds.join(b)).unwrap();
        std::fs::write(builds.join(b).join("PKGBUILD"), "x").unwrap();
    }
    s.stub(
        "pacman-conf",
        &format!(
            r#"case "$1" in CacheDir) echo '{}/' ;; DBPath) echo /var/lib/pacman/ ;; esac"#,
            cache.display()
        ),
    );
    s.stub(
        "paccache",
        "echo '==> no candidate packages found for pruning'",
    );
    s.stub(
        "pacman",
        r#"case "$1" in
-Qdtq) echo orphan-lib ;;
-Qi) printf 'Name            : orphan-lib\nInstalled Size  : 1.50 MiB\n' ;;
-Rns) exit 0 ;;
*) exec /usr/bin/pacman "$@" ;;
esac"#,
    );
    let o = s.run("cleanup", &["plan"], &[]);
    assert_eq!(o.code, 0, "{}", o.events);
    assert!(
        o.out.contains("orphan\torphan-lib\t1.50 MiB\n"),
        "{}",
        o.out
    );
    assert!(
        o.out
            .contains(&format!("aur\t{}/gone-pkg\t", builds.display())),
        "{}",
        o.out
    );
    assert!(
        !o.out.contains("clone/pacman"),
        "an installed package keeps its build"
    );
    // Only the parts given.
    let o = s.run("cleanup", &["run", "cache"], &[]);
    assert_eq!(
        (o.code, o.events.as_str()),
        (0, "step\tcache\n"),
        "{}",
        o.calls
    );
    assert!(!o.calls.contains("-Rns") && builds.join("gone-pkg").exists());
    let o = s.run("cleanup", &["run", "orphans", "aur"], &[]);
    assert_eq!(o.code, 0, "{}", o.events);
    assert!(
        o.calls.contains("pacman -Rns --noconfirm -- orphan-lib\n"),
        "{}",
        o.calls
    );
    assert!(!builds.join("gone-pkg").exists() && builds.join("pacman").exists());
    // A part no plan knows stops the run before anything goes.
    let o = s.run("cleanup", &["run", "trash"], &[]);
    assert_eq!(o.code, 1, "{}", o.events);
    assert!(o.events.starts_with("fail\t"), "{}", o.events);
}

/// What the dotfiles need beside the real git and chezmoi: a git identity.
fn dotfiles(test: &str) -> Sandbox {
    let s = Sandbox::new(test);
    std::fs::write(
        s.dir.join(".gitconfig"),
        "[user]\n\tname = Tux\n\temail = tux@example.org\n[init]\n\tdefaultBranch = main\n",
    )
    .unwrap();
    s
}

/// git without the machine's own configuration.
const TUX: [(&str, &str); 1] = [("GIT_CONFIG_NOSYSTEM", "1")];

/// chezmoi's source folder in a sandbox, where the dotfiles live.
const SOURCE: &str = ".local/share/chezmoi";

impl Sandbox {
    fn dotfiles(&self, args: &[&str]) -> Outcome {
        self.run("dotfiles", args, &TUX)
    }

    fn write(&self, path: &str, text: &str) {
        let path = self.dir.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.dir.join(path)).unwrap_or_default()
    }

    /// A file of the dotfiles, by its path in chezmoi's source folder.
    fn source(&self, path: &str) -> String {
        self.read(&format!("{SOURCE}/{path}"))
    }

    fn put(&self, path: &str, text: &str) {
        self.write(&format!("{SOURCE}/{path}"), text);
    }

    fn path(&self, path: &str) -> String {
        self.dir.join(path).display().to_string()
    }

    fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(["-C", self.dir.join(SOURCE).to_str().unwrap()])
            .args(args)
            .env("HOME", &self.dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into()
    }

    /// chezmoi's configuration with an age key of its own, as a person sets it up for secrets.
    fn encryption(&self) {
        let key = self.dir.join("config/chezmoi/key.txt");
        std::fs::create_dir_all(key.parent().unwrap()).unwrap();
        let made = Command::new("chezmoi")
            .args(["age-keygen", "--output"])
            .arg(&key)
            .env("HOME", &self.dir)
            .env("PATH", self.search())
            .output()
            .unwrap();
        assert!(
            made.status.success(),
            "{}",
            String::from_utf8_lossy(&made.stderr)
        );
        let public = Command::new("chezmoi")
            .args(["age-keygen", "--convert"])
            .arg(&key)
            .env("HOME", &self.dir)
            .env("PATH", self.search())
            .output()
            .unwrap();
        let recipient = String::from_utf8_lossy(&public.stdout).trim().to_string();
        assert!(recipient.starts_with("age1"), "{recipient}");
        self.write(
            "config/chezmoi/chezmoi.toml",
            &format!(
                "encryption = \"age\"\n[age]\n    identity = \"{}\"\n    recipient = \"{recipient}\"\n",
                key.display()
            ),
        );
    }
}

/// A bare repository beside a sandbox, as GitHub or Gitea would hold the dotfiles.
fn remote(s: &Sandbox) -> String {
    let dir = s.dir.join("remote.git");
    let ok = Command::new("git")
        .args(["init", "--quiet", "--bare", "--initial-branch=main"])
        .arg(&dir)
        .status()
        .unwrap()
        .success();
    assert!(ok);
    format!("file://{}", dir.display())
}

#[test]
fn dotfiles_are_added_backed_up_and_set_up_on_another_machine() {
    let a = dotfiles("dotfiles-a");
    assert_eq!(
        a.dotfiles(&["state"]).out.lines().nth(2),
        Some("repo\tmissing")
    );
    a.write(".config/nvim/init.lua", "vim.o.number = true\n");
    a.write(".zshrc", "alias ll='ls -l'\n");
    for path in [".config/nvim", ".zshrc"] {
        let o = a.dotfiles(&["add", &a.path(path)]);
        assert_eq!(o.code, 0, "{path}: {}{}", o.events, o.err);
    }
    // chezmoi's copies, and the files in the home folder stay as they are.
    assert_eq!(a.source("dot_zshrc"), "alias ll='ls -l'\n");
    assert_eq!(
        a.source("dot_config/nvim/init.lua"),
        "vim.o.number = true\n"
    );
    assert!(!a.dir.join(".zshrc").is_symlink());
    let state = a.dotfiles(&["state"]).out;
    assert!(
        state.contains("files\t2\nsecrets\t0\ndue\t0\nchanged\t0\n"),
        "{state}"
    );

    let o = a.dotfiles(&["backup"]);
    assert_eq!(o.code, 0, "{}{}", o.events, o.err);
    assert_eq!(o.events, "step\tfiles\nstep\tcommit\n");
    let host = std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap();
    assert_eq!(
        a.git(&["log", "--format=%s"]),
        format!("Back up {}: nvim, zshrc\n", host.trim())
    );
    let url = remote(&a);
    assert_eq!(a.dotfiles(&["connect", &url]).code, 0);
    // Changed here: a backup takes it in through chezmoi re-add and uploads it.
    a.write(".zshrc", "alias ll='ls -la'\n");
    let state = a.dotfiles(&["state"]).out;
    assert!(state.contains("due\t0\nchanged\t1\n"), "{state}");
    let o = a.dotfiles(&["backup"]);
    assert_eq!(o.code, 0, "{}{}", o.events, o.err);
    assert!(
        o.events.ends_with("step\tpull\nstep\tpush\n"),
        "{}",
        o.events
    );
    assert_eq!(a.source("dot_zshrc"), "alias ll='ls -la'\n");
    assert!(a.dotfiles(&["state"]).out.contains("changed\t0\n"));
    assert!(
        a.git(&["status", "-sb"])
            .starts_with("## main...origin/main\n")
    );

    // Another machine: connected, then set up through chezmoi in the terminal.
    let b = dotfiles("dotfiles-b");
    let o = b.dotfiles(&["connect", &url]);
    assert_eq!(o.code, 0, "{}{}", o.events, o.err);
    assert!(b.dotfiles(&["state"]).out.contains("due\t2\n"));
    let o = b.dotfiles(&["setup"]);
    assert_eq!(o.code, 0, "{}{}", o.events, o.err);
    assert_eq!(o.events, "step\tpull\nstep\tapply\nterminal\n");
    assert_eq!(b.read(".config/nvim/init.lua"), "vim.o.number = true\n");
    assert_eq!(b.read(".zshrc"), "alias ll='ls -la'\n");
    assert!(b.dotfiles(&["state"]).out.contains("due\t0\nchanged\t0\n"));
}

#[test]
fn dotfiles_name_what_they_keep_and_refuse_what_is_no_configuration() {
    let s = dotfiles("dotfiles-keep");
    s.write(".config/app/a.conf", "a\n");
    s.write(".config/app/b.conf", "b\n");
    let home = s.dir.display().to_string();
    let add = |path: &str| s.dotfiles(&["add", &format!("{home}/{path}")]).code;
    assert_eq!(add(".config/app"), 0);
    assert_eq!(add(".config/app"), 33, "kept already");
    assert_eq!(s.dotfiles(&["add", "/etc/hosts"]).code, 24);
    std::os::unix::fs::symlink("/etc/hosts", s.dir.join("hosts")).unwrap();
    assert_eq!(add("hosts"), 27);
    // The cache as XDG_CACHE_HOME puts it, and chezmoi's folder with its configuration.
    s.write("cache/x", "x\n");
    assert_eq!(add("cache/x"), 25);
    s.write("config/chezmoi/chezmoi.toml", "\n");
    assert_eq!(add("config/chezmoi/chezmoi.toml"), 25);
    std::fs::create_dir_all(s.dir.join("empty")).unwrap();
    assert_eq!(add("empty"), 31);
    for i in 0..101 {
        s.write(&format!("many/{i}"), "x\n");
    }
    assert_eq!(add("many"), 32);
    s.write("project/.git/HEAD", "x\n");
    assert_eq!(add("project"), 29);
    assert_eq!(add("nothing"), 28);
    // A folder holding nothing but kept files is named whole, the one above as well.
    let o = s.dotfiles(&["managed"]);
    assert_eq!(o.code, 0, "{}", o.events);
    assert_eq!(
        o.out,
        format!(
            "dir\t{home}/.config/\ndir\t{home}/.config/app/\nfile\t{home}/.config/app/a.conf\nfile\t{home}/.config/app/b.conf\n"
        )
    );
    assert_eq!(s.dotfiles(&["remove", &format!("{home}/.zshrc")]).code, 34);
    // Taken out, the files stay and no empty folder is left in the dotfiles to be made.
    assert_eq!(
        s.dotfiles(&["remove", &format!("{home}/.config/app")]).code,
        0
    );
    assert_eq!(s.read(".config/app/a.conf"), "a\n");
    assert!(!s.dir.join(SOURCE).join("dot_config").exists());
    assert_eq!(s.dotfiles(&["managed"]).out, "");
}

#[test]
fn secrets_are_encrypted_as_chezmoi_is_configured_and_named_apart() {
    let s = dotfiles("dotfiles-secrets");
    s.write(".ssh/id_ed25519", "PRIVATE\n");
    // Without encryption in chezmoi's configuration chezmoi refuses in its own words.
    let o = s.dotfiles(&["secret-add", &s.path(".ssh/id_ed25519")]);
    assert_eq!(o.code, 47, "{}{}", o.events, o.err);
    assert!(!o.err.is_empty());
    s.encryption();
    let o = s.dotfiles(&["secret-add", &s.path(".ssh/id_ed25519")]);
    assert_eq!(o.code, 0, "{}{}", o.events, o.err);
    let sealed = s.git(&["ls-files"]);
    let file = sealed
        .lines()
        .find(|l| l.ends_with(".age"))
        .unwrap()
        .to_string();
    assert!(file.starts_with("dot_ssh/encrypted_"), "{sealed}");
    // age's file, armored as chezmoi writes it.
    let bytes = std::fs::read(s.dir.join(SOURCE).join(&file)).unwrap();
    assert!(
        bytes.starts_with(b"-----BEGIN AGE ENCRYPTED FILE-----\n"),
        "{bytes:?}"
    );
    assert!(!bytes.windows(7).any(|w| w == b"PRIVATE"));
    s.write(".zshrc", "x\n");
    assert_eq!(s.dotfiles(&["add", &s.path(".zshrc")]).code, 0);
    let state = s.dotfiles(&["state"]).out;
    assert!(state.contains("files\t1\nsecrets\t1\n"), "{state}");
    let home = s.dir.display().to_string();
    assert_eq!(
        s.dotfiles(&["managed"]).out,
        format!("file\t{home}/.zshrc\nsecret\t{home}/.ssh/id_ed25519\n")
    );
    // A secret is no file to remove, and taken out as a secret its file stays.
    assert_eq!(s.dotfiles(&["remove", &s.path(".ssh/id_ed25519")]).code, 34);
    let o = s.dotfiles(&["secret-remove", &s.path(".ssh/id_ed25519")]);
    assert_eq!(o.code, 0, "{}{}", o.events, o.err);
    assert_eq!(s.read(".ssh/id_ed25519"), "PRIVATE\n");
    assert!(!s.dir.join(SOURCE).join("dot_ssh").exists());
}

#[test]
fn a_template_changed_here_is_named_as_a_backup_cannot_take_it_in() {
    let s = dotfiles("dotfiles-template");
    s.put("dot_npmrc.tmpl", "init-author-name={{ \"Tux\" }}\n");
    let ok = Command::new("git")
        .args([
            "-C",
            s.dir.join(SOURCE).to_str().unwrap(),
            "init",
            "--quiet",
        ])
        .status()
        .unwrap()
        .success();
    assert!(ok);
    assert_eq!(s.dotfiles(&["setup"]).code, 0);
    assert_eq!(s.read(".npmrc"), "init-author-name=Tux\n");
    s.write(".npmrc", "init-author-name=Tuxedo\n");
    let o = s.dotfiles(&["backup"]);
    assert_eq!(o.code, 0, "{}{}", o.events, o.err);
    assert_eq!(
        o.out,
        format!(
            "Changed here, its source a template: {}\n",
            s.path(".npmrc")
        )
    );
    assert_eq!(
        s.source("dot_npmrc.tmpl"),
        "init-author-name={{ \"Tux\" }}\n"
    );
}

#[test]
fn a_backup_takes_back_nothing_the_dotfiles_brought_since_the_last_setup() {
    let s = dotfiles("dotfiles-incoming");
    s.write(".zshrc", "one\n");
    s.write(".aliases", "one\n");
    for path in [".zshrc", ".aliases"] {
        assert_eq!(s.dotfiles(&["add", &s.path(path)]).code, 0);
    }
    assert_eq!(s.dotfiles(&["backup"]).code, 0);
    // What a pull brought and no setup applied yet, one of them changed here as well.
    s.put("dot_zshrc", "two\n");
    s.put("dot_aliases", "two\n");
    s.git(&["commit", "--quiet", "--all", "--message", "pulled"]);
    s.write(".aliases", "mine\n");
    let o = s.dotfiles(&["backup"]);
    assert_eq!(o.code, 0, "{}{}", o.events, o.err);
    assert_eq!(
        o.out,
        format!("Changed here and in the dotfiles: {}\n", s.path(".aliases"))
    );
    assert_eq!(s.source("dot_zshrc"), "two\n");
    assert_eq!(s.source("dot_aliases"), "two\n");
    assert_eq!(s.read(".aliases"), "mine\n");
}

#[test]
fn dotfiles_in_a_folder_of_their_repository_are_read_set_up_and_backed_up() {
    let s = dotfiles("dotfiles-root");
    s.put(".chezmoiroot", "home\n");
    s.put("README.md", "# Dotfiles\n");
    s.put("home/dot_zshrc", "one\n");
    s.git(&["init", "--quiet"]);
    assert_eq!(s.dotfiles(&["setup"]).code, 0);
    assert_eq!(s.read(".zshrc"), "one\n");
    assert!(!s.dir.join("README.md").exists());
    let state = s.dotfiles(&["state"]).out;
    assert!(state.contains("repo\tready\n"), "{state}");
    assert!(state.contains("files\t1\n"), "{state}");
    s.write(".zshrc", "two\n");
    s.write(".aliases", "x\n");
    assert_eq!(s.dotfiles(&["add", &s.path(".aliases")]).code, 0);
    let o = s.dotfiles(&["backup"]);
    assert_eq!(o.code, 0, "{}{}", o.events, o.err);
    assert_eq!(s.source("home/dot_zshrc"), "two\n");
    assert_eq!(s.source("home/dot_aliases"), "x\n");
    assert_eq!(s.git(&["status", "--porcelain"]), "");
}

#[test]
fn a_configuration_chezmoi_never_made_here_waits_for_a_setup() {
    let s = dotfiles("dotfiles-unconfigured");
    s.put(".chezmoi.toml.tmpl", "[data]\n    tag = \"x\"\n");
    s.put("dot_profile.tmpl", "{{ .tag }}\n");
    let ok = Command::new("git")
        .args([
            "-C",
            s.dir.join(SOURCE).to_str().unwrap(),
            "init",
            "--quiet",
        ])
        .status()
        .unwrap()
        .success();
    assert!(ok);
    let state = s.dotfiles(&["state"]).out;
    assert!(state.ends_with("config\tmissing\n"), "{state}");
    let o = s.dotfiles(&["setup"]);
    assert_eq!(o.code, 0, "{}{}", o.events, o.err);
    assert_eq!(s.read(".profile"), "x\n");
    assert!(s.dotfiles(&["state"]).out.contains("due\t0\n"));
    // A template that does not render leaves the state unknown, its words in the details.
    s.put("dot_profile.tmpl", "{{ .missing.value }}\n");
    let o = s.dotfiles(&["state"]);
    assert!(o.out.ends_with("status\tfailed\n"), "{}", o.out);
    assert!(o.err.contains("missing"), "{}", o.err);
}

#[test]
fn what_is_added_is_backed_up_though_a_global_ignore_file_names_it() {
    let s = dotfiles("dotfiles-ignored");
    let config = s.read(".gitconfig");
    s.write("ignore", "CLAUDE.md\n*.age\n");
    s.write(
        ".gitconfig",
        &format!(
            "{config}[core]\n\texcludesFile = {}\n",
            s.dir.join("ignore").display()
        ),
    );
    s.write(".claude/CLAUDE.md", "# Rules\n");
    let o = s.dotfiles(&["add", &s.path(".claude/CLAUDE.md")]);
    assert_eq!(o.code, 0, "{}{}", o.events, o.err);
    s.encryption();
    s.write(".ssh/id_ed25519", "PRIVATE\n");
    let o = s.dotfiles(&["secret-add", &s.path(".ssh/id_ed25519")]);
    assert_eq!(o.code, 0, "{}{}", o.events, o.err);
    let o = s.dotfiles(&["backup"]);
    assert_eq!(o.code, 0, "{}{}", o.events, o.err);
    let files = s.git(&["ls-files"]);
    assert!(files.contains("dot_claude/CLAUDE.md\n"), "{files}");
    assert!(files.contains("dot_ssh/encrypted_"), "{files}");
}

#[test]
fn a_repository_that_does_not_fit_is_refused_and_the_address_taken_back() {
    let s = dotfiles("dotfiles-connect");
    s.write(".zshrc", "x\n");
    assert_eq!(s.dotfiles(&["add", &s.path(".zshrc")]).code, 0);
    assert_eq!(s.dotfiles(&["backup"]).code, 0);
    let ours = remote(&s);
    assert_eq!(s.dotfiles(&["connect", &ours]).code, 0);
    // A repository with a history of its own.
    let other = s.dir.join("other");
    std::fs::create_dir_all(&other).unwrap();
    for args in [
        &["init", "--quiet", "--initial-branch=main"][..],
        &["commit", "--quiet", "--allow-empty", "-m", "elsewhere"],
    ] {
        let ok = Command::new("git")
            .arg("-C")
            .arg(&other)
            .args(args)
            .env("HOME", &s.dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .unwrap()
            .success();
        assert!(ok, "{args:?}");
    }
    let foreign = format!("file://{}", other.display());
    assert_eq!(s.dotfiles(&["connect", &foreign]).code, 36);
    assert_eq!(s.git(&["remote", "get-url", "origin"]), format!("{ours}\n"));
    assert_eq!(s.git(&["branch", "--remotes"]), "", "nothing of it stays");
    assert_eq!(
        s.dotfiles(&["connect", "http://example.org/dotfiles.git"])
            .code,
        37
    );
    let gone = format!("file://{}/gone.git", s.dir.display());
    assert_eq!(s.dotfiles(&["connect", &gone]).code, 12);
    assert_eq!(s.git(&["remote", "get-url", "origin"]), format!("{ours}\n"));
    assert_eq!(s.dotfiles(&["connect", ""]).code, 0);
    assert_eq!(s.git(&["remote"]), "");
    // Not a repository of its own, nothing is touched.
    let t = dotfiles("dotfiles-bare");
    assert!(
        Command::new("git")
            .args(["init", "--quiet", "--bare"])
            .arg(t.dir.join(SOURCE))
            .status()
            .unwrap()
            .success()
    );
    assert!(t.dotfiles(&["state"]).out.contains("repo\tinvalid\n"));
    assert_eq!(t.dotfiles(&["setup"]).code, 10);
}
