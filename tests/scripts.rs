//! The scripts against stand-ins for the system's commands, and the read-only ones against the
//! real pacman of the machine the tests run on.

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

use system_manager::data::{packages, system::System};
use system_manager::script::{op, query};

struct Sandbox {
    dir: PathBuf,
}

struct Outcome {
    code: i32,
    out: String,
    events: String,
    calls: String,
}

impl Sandbox {
    fn new(test: &str) -> Sandbox {
        let dir = std::env::temp_dir().join(format!("sm-scripts-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        let sandbox = Sandbox { dir };
        // sudo runs the command as it is: the sandbox has nothing to elevate.
        sandbox.stub("sudo", r#"exec "$@""#);
        sandbox
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

    fn run(&self, script: &str, args: &[&str], env: &[(&str, &str)]) -> Outcome {
        let scripts = concat!(env!("CARGO_MANIFEST_DIR"), "/scripts");
        let events = self.dir.join("events");
        let output = Command::new("bash")
            .arg("-c")
            .arg(r#"exec 3>"$SM_EVENTS"; exec bash --noprofile --norc "$0" "$@""#)
            .arg(format!("{scripts}/{script}/script.sh"))
            .args(args)
            .env(
                "PATH",
                format!("{}:/usr/bin", self.dir.join("bin").display()),
            )
            .env("SM_EVENTS", &events)
            .env("SM_AUR", "")
            // A session answers, unless a test takes it away.
            .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/sandbox")
            .env_remove("XDG_RUNTIME_DIR")
            .env("HOME", &self.dir)
            .env("XDG_CONFIG_HOME", self.dir.join("config"))
            .env("XDG_CACHE_HOME", self.dir.join("cache"))
            .envs(env.iter().copied())
            .output()
            .unwrap();
        Outcome {
            code: output.status.code().unwrap_or(-1),
            out: String::from_utf8_lossy(&output.stdout).into(),
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
    s.stub("flatpak", "");
    let o = s.run("update", &["list"], &[("SM_AUR", "paru")]);
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
    let o = s.run("update", &["list"], &[]);
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
    let o = s.run("update", &["list"], &[("SM_AUR", "paru")]);
    assert_eq!((o.code, o.out.as_str()), (0, ""));
}

#[test]
fn updates_tells_an_unreachable_server_from_a_failing_helper() {
    let s = Sandbox::new("offline");
    s.stub("checkupdates", "exit 1");
    assert_eq!(s.run("update", &["list"], &[]).code, 10);
    s.stub("checkupdates", "exit 2");
    s.stub("paru", "echo 'error: could not resolve host' >&2; exit 1");
    assert_eq!(s.run("update", &["list"], &[("SM_AUR", "paru")]).code, 11);
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
    assert_eq!(f[..4], ["fail", "3", "packages/script.sh", "10"], "{first}");
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

#[test]
fn repair_leaves_a_held_lock_and_a_strange_keyring_alone() {
    let s = Sandbox::new("repair");
    let db = s.dir.join("db");
    std::fs::create_dir_all(&db).unwrap();
    std::fs::write(db.join("db.lck"), "").unwrap();
    s.stub(
        "pacman-conf",
        &format!(
            r#"case "$1" in DBPath) echo {}/ ;; GPGDir) echo / ;; esac"#,
            db.display()
        ),
    );
    // A front end other than pacman, holding the lock open as libalpm does.
    s.stub("find", "echo /proc/42/fd/7");
    let o = s.run("repair", &["run"], &[]);
    assert_eq!(o.code, 10);
    assert!(db.join("db.lck").exists());
    s.stub("find", "exit 1");
    let o = s.run("repair", &["run"], &[]);
    assert_eq!(o.code, 11);
    assert!(!db.join("db.lck").exists(), "a lock nobody holds stays");
    assert!(!o.calls.contains("pacman-key"), "{}", o.calls);
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
    s.stub("pacman", "exit 0");
    s.stub(
        "pacman-key",
        r#"[ "$1" = --gpgdir ] || exit 3; mkdir -p "$2"; [ "$3" = --populate ] && exit 1; touch "$2/new""#,
    );
    assert_eq!(s.run("repair", &["run"], &[]).code, 1);
    assert!(gpg.join("old").exists(), "a failed rebuild left no keyring");
    s.stub(
        "pacman-key",
        r#"[ "$1" = --gpgdir ] || exit 3; mkdir -p "$2"; touch "$2/new""#,
    );
    assert_eq!(s.run("repair", &["run"], &[]).code, 0);
    assert!(gpg.join("new").exists() && !gpg.join("old").exists());
    let left: Vec<_> = std::fs::read_dir(&s.dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("gnupg."))
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

#[test]
fn a_configuration_update_is_only_ever_a_pacnew_or_pacsave() {
    let s = Sandbox::new("pacnew");
    for op in ["take", "keep", "merge"] {
        let o = s.run("pacnew", &[op, "/etc/passwd"], &[]);
        assert_eq!(o.code, 11, "{op}");
        assert!(!o.calls.contains("sudo"), "{op}: {}", o.calls);
    }
}

#[test]
fn a_configuration_update_takes_a_snapshot_where_snapper_keeps_the_root() {
    let s = Sandbox::new("pacnew-snapshot");
    s.stub("snapper", "exit 0");
    s.stub("mv", "exit 0");
    let o = s.run("pacnew", &["take", "/etc/x.conf.pacnew"], &[]);
    assert_eq!(o.code, 0, "{}", o.events);
    let snapshots = std::path::Path::new("/etc/snapper/configs/root").exists();
    assert_eq!(
        o.calls
            .contains("snapper --no-dbus -c root create --cleanup-algorithm number"),
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

#[test]
fn repair_reinstalls_the_native_packages_with_altered_files_only() {
    let s = Sandbox::new("repair-files");
    s.stub(
        "pacman",
        r#"case "$1" in
-Qkk) printf 'backup file: pacman: /etc/pacman.conf (Size mismatch)\npacman: 426 total files, 0 altered files\nwarning: bar: /usr/bin/bar (Size mismatch)\nbar: 10 total files, 1 altered files\nwarning: baz-git: /usr/bin/baz (No such file or directory)\nbaz-git: 3 total files, 1 altered files\n'; exit 1 ;;
-Qqn) printf 'bar\npacman\n' ;;
esac"#,
    );
    let o = s.run("repair", &["files"], &[]);
    assert_eq!(o.code, 0, "{}", o.events);
    assert!(o.calls.contains("pacman -S -- bar\n"), "{}", o.calls);
    assert!(!o.calls.contains("baz-git") && !o.calls.contains("pacman -S -- pacman"));
    assert!(
        o.out.contains("foreign, not reinstalled: baz-git"),
        "{}",
        o.out
    );
    assert_eq!(o.events, "step\tcheck\nstep\treinstall\n");

    let clean = Sandbox::new("repair-files-clean");
    clean.stub("pacman", r#"case "$1" in -Qkk) printf 'pacman: 426 total files, 0 altered files\n' ;; -Qqn) echo pacman ;; esac"#);
    let o = clean.run("repair", &["files"], &[]);
    assert!(o.code == 0 && !o.calls.contains("pacman -S"), "{}", o.calls);
    assert_eq!(o.events, "step\tcheck\n");
}

#[test]
fn upgrade_takes_the_repositories_first_then_the_aur_then_flatpak() {
    let s = Sandbox::new("upgrade");
    for c in ["pacman", "paru", "flatpak"] {
        s.stub(c, "exit 0");
    }
    let o = s.run("update", &["run"], &[("SM_AUR", "paru")]);
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
}

#[test]
fn a_notification_nobody_answers_ends_as_dismissed() {
    let s = Sandbox::new("notify");
    let args = [
        "notify",
        "System update",
        "6 updates are ready.",
        "Update",
        "1",
    ];
    s.stub("notify-send", "exec sleep 30");
    let started = std::time::Instant::now();
    let o = s.run("desktop", &args, &[]);
    assert_eq!((o.code, o.out.as_str()), (0, ""), "{}", o.events);
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    s.stub("notify-send", "echo open");
    assert_eq!(s.run("desktop", &args, &[]).out, "open\n");
    s.stub("notify-send", "exit 1");
    assert_eq!(s.run("desktop", &args, &[]).code, 1);
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
        stored.contains("check-cmd\t'\"/opt/it\\'s mine/system-manager\" check'\n"),
        "{stored}"
    );
    assert!(
        stored.contains("update-cmd\t'\"/opt/it\\'s mine/system-manager\" --window upgrade'\n")
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

#[test]
fn the_update_indicator_comes_from_extensions_gnome_org_and_goes_again() {
    let s = Sandbox::new("extension");
    s.stub("gnome-shell", "echo 'GNOME Shell 51.1'");
    s.stub(
        "curl",
        r#"case "$*" in
*extension-info*shell_version=51) echo '{"uuid": "x", "download_url": "/download-extension/x.zip?version_tag=1", "pk": 1010}' ;;
*download-extension*) echo zip ;;
*) exit 22 ;;
esac"#,
    );
    s.stub("gnome-extensions", "exit 0");
    s.stub("gsettings", "exit 0");
    let data = s.dir.join("data");
    let env = [("XDG_DATA_HOME", data.to_str().unwrap())];
    let o = s.run("desktop", &["extension-install"], &env);
    assert_eq!(o.code, 0, "{}", o.events);
    let calls: Vec<&str> = o
        .calls
        .lines()
        .filter(|l| l.starts_with("gnome-extensions"))
        .collect();
    assert!(
        calls[0].starts_with("gnome-extensions install --force /"),
        "{calls:?}"
    );
    assert_eq!(
        calls[1],
        "gnome-extensions enable arch-update@RaphaelRochet"
    );

    s.stub("gnome-shell", "echo 'GNOME Shell 99.0'");
    assert_eq!(s.run("desktop", &["extension-install"], &env).code, 11);

    // The running shell takes it out.
    let dir = indicator_installed(&s);
    let o = s.run("desktop", &["extension-remove"], &env);
    assert_eq!(o.code, 0, "{}", o.events);
    assert!(
        o.calls
            .contains("gnome-extensions uninstall arch-update@RaphaelRochet")
    );
    assert!(o.calls.contains("gsettings --schemadir"));
    assert!(dir.exists(), "the stand-in shell removes nothing");
    // Without a shell, the folder goes by hand.
    s.stub(
        "gnome-extensions",
        r#"[ "$1" = uninstall ] && exit 2; exit 0"#,
    );
    assert_eq!(s.run("desktop", &["extension-remove"], &env).code, 0);
    assert!(!dir.exists());
    // Nothing installed, nothing to do.
    assert_eq!(s.run("desktop", &["extension-remove"], &env).code, 0);
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
        2
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
    s.stub("pacman", r#"if [ "$1" = -Qqo ]; then echo "$2-bin"; fi"#);
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
    assert!(
        o.calls.ends_with("pacman -Rns -- paru-bin\n"),
        "{}",
        o.calls
    );
}

/// The Recovery rebuilds a rolled back snapshot's kernel from the cache, so those files stay.
/// Needs /.snapshots to be created, so it runs as root, as in the CI container.
#[test]
fn cleanup_keeps_the_kernels_the_snapshots_need() {
    if !rustix::process::getuid().is_root() || std::path::Path::new("/.snapshots").exists() {
        eprintln!("skipped: needs root and no /.snapshots");
        return;
    }
    let local = "/.snapshots/1/snapshot/var/lib/pacman/local/linux-zen-7.2.8.zen1-1";
    std::fs::create_dir_all(local).unwrap();
    std::fs::write(
        format!("{local}/files"),
        "%FILES%\nusr/lib/modules/7.2.8-zen1-1-zen/vmlinuz\n",
    )
    .unwrap();
    let s = Sandbox::new("cleanup");
    let cache = s.dir.join("pkg");
    std::fs::create_dir_all(&cache).unwrap();
    let old_kernel = "linux-zen-7.2.8.zen1-1-x86_64.pkg.tar.zst";
    let other = "foo-1.0-1-x86_64.pkg.tar.zst";
    for f in [old_kernel, other] {
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
        &format!("echo '==> Candidate packages:'; echo '{old_kernel}'; echo '{other}'; echo"),
    );
    s.stub(
        "pacman",
        r#"case "$1" in -Qdtq) exit 1 ;; *) exec /usr/bin/pacman "$@" ;; esac"#,
    );
    let o = s.run("cleanup", &["plan"], &[]);
    std::fs::remove_dir_all("/.snapshots").unwrap();
    assert_eq!(o.code, 0, "{}", o.events);
    assert!(
        o.out.contains("keep\tlinux-zen-7.2.8.zen1-1\n"),
        "{}",
        o.out
    );
    assert!(
        o.out
            .contains(&format!("cache\t{}/{other}\t5\n", cache.display())),
        "{}",
        o.out
    );
    assert!(
        !o.out
            .contains(&format!("cache\t{}/{old_kernel}", cache.display())),
        "{}",
        o.out
    );
}

#[test]
fn cleanup_removes_a_cached_package_with_its_signature() {
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
        &format!("echo '==> Candidate packages:'; echo '{package}'; echo"),
    );
    s.stub(
        "pacman",
        r#"case "$1" in -Qdtq) exit 1 ;; *) exec /usr/bin/pacman "$@" ;; esac"#,
    );
    s.stub("flatpak", "exit 0");
    let o = s.run("cleanup", &["run"], &[]);
    assert_eq!(o.code, 0, "{}", o.events);
    assert!(!cache.join(package).exists());
    assert!(
        !cache.join(format!("{package}.sig")).exists(),
        "the signature goes with the package"
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
    let dir = std::env::temp_dir().join(format!("sm-aur-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("seed")).unwrap();
    let remote = dir.join("aur/arch-os-manager.git");
    let sh = |script: &str| {
        let out = Command::new("bash")
            .arg("-c")
            .arg(script)
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .env("DIR", &dir)
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
    sh(r#"tools/aur.sh 2.0.0 LICENSE "$DIR/stamped" && tools/aur-push.sh "$DIR/stamped""#);
    let tree = sh(r#"git -C "$REMOTE" ls-tree --name-only master"#);
    assert_eq!(tree, ".SRCINFO\nPKGBUILD\narch-os-manager.install\n");
    let commit = sh(r#"git -C "$REMOTE" log -1 --format='%an <%ae> %s' master"#);
    assert_eq!(commit, "murkl <murkl@users.noreply.github.com> 2.0.0\n");
    assert!(sh(r#"tools/aur-push.sh "$DIR/stamped""#).contains("there already"));
    std::fs::remove_dir_all(&dir).unwrap();
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
