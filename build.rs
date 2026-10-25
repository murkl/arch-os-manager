// Embeds scripts/ and themes/ so the binary is the whole program, names the operations the
// scripts declare, and names its version.
use serde::Deserialize;
use std::collections::BTreeMap;
use std::{env, fs, path::Path, process::Command};

/// Of a script's yaml, only which operations it declares.
#[derive(Deserialize)]
struct Meta {
    #[serde(default)]
    ops: BTreeMap<String, serde::de::IgnoredAny>,
}

/// Every file below `dir` but dotfiles, as paths relative to it.
fn files(dir: &Path, prefix: &str, out: &mut Vec<String>) {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.unwrap())
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().into_string().unwrap();
        if name.starts_with('.') {
            continue;
        }
        let path = format!("{prefix}{name}");
        if e.file_type().unwrap().is_dir() {
            files(&e.path(), &format!("{path}/"), out);
        } else {
            out.push(path);
        }
    }
}

fn main() {
    let mut code = String::new();
    for (name, dir) in [("SCRIPTS", "scripts"), ("THEMES", "themes")] {
        println!("cargo::rerun-if-changed={dir}");
        let mut found = Vec::new();
        files(Path::new(dir), "", &mut found);
        code += &format!("pub static {name}: &[(&str, &[u8])] = &[\n");
        for f in found {
            code += &format!(
                "    ({f:?}, include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/{dir}/{f}\"))),\n"
            );
        }
        code += "];\n";
    }
    let out = env::var("OUT_DIR").map(std::path::PathBuf::from).unwrap();
    fs::write(out.join("embedded.rs"), code).unwrap();
    fs::write(out.join("ops.rs"), ops()).unwrap();

    // What a release hands in, else a pre-release of the patch after the last release tag.
    println!("cargo::rerun-if-env-changed=SM_VERSION");
    println!("cargo::rerun-if-changed=.git/refs/tags");
    let version = match env::var("SM_VERSION") {
        Ok(v) if !v.is_empty() => v,
        _ => next_dev(&last_release()),
    };
    println!("cargo::rustc-env=SM_VERSION={version}");
}

/// `script::op`: a constant per operation of every `scripts/<name>/<name>.yaml`, `upgrade run`
/// as `UPGRADE_RUN`, and `ALL`.
fn ops() -> String {
    let mut found = Vec::new();
    files(Path::new("scripts"), "", &mut found);
    let mut code = String::new();
    let mut all = Vec::new();
    for f in found.iter().filter_map(|f| {
        let (dir, file) = f.split_once('/')?;
        (file == format!("{dir}.yaml")).then_some(dir)
    }) {
        let path = format!("scripts/{f}/{f}.yaml");
        let src = fs::read_to_string(&path).unwrap();
        let meta: Meta = serde_saphyr::from_str(&src).unwrap_or_else(|e| panic!("{path}: {e}"));
        for name in meta.ops.keys() {
            let konst = format!("{f}_{name}").to_uppercase().replace('-', "_");
            code += &format!("pub const {konst}: Op = Op {{ script: {f:?}, name: {name:?} }};\n");
            all.push(konst);
        }
    }
    code += &format!("pub const ALL: &[Op] = &[{}];\n", all.join(", "));
    code
}

/// The newest release tag before HEAD, `v` dropped, by the patterns of murkl/workflows.
fn last_release() -> String {
    let out = Command::new("git")
        .args(["describe", "--tags", "--abbrev=0"])
        .args([
            "--match",
            "[0-9]*.[0-9]*.[0-9]*",
            "--match",
            "v[0-9]*.[0-9]*.[0-9]*",
        ])
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "no release tag in reach: fetch the tags or set SM_VERSION"
    );
    let tag = String::from_utf8(out.stdout).unwrap();
    tag.trim().trim_start_matches('v').to_owned()
}

/// `1.9.1` as `1.9.2-dev`.
fn next_dev(released: &str) -> String {
    let parts: Vec<u64> = released.split('.').filter_map(|p| p.parse().ok()).collect();
    let [major, minor, patch] = parts[..] else {
        panic!("the release tag {released:?} names no x.y.z");
    };
    format!("{major}.{minor}.{}-dev", patch + 1)
}
