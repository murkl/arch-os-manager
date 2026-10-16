// Embeds scripts/ and themes/ so the binary is the whole program, and names its version.
use std::{env, fs, path::Path};

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
    let out = Path::new(&env::var("OUT_DIR").unwrap()).join("embedded.rs");
    fs::write(out, code).unwrap();

    // What a release hands in, else a pre-release of the patch after the last release, as
    // release-please keeps it.
    println!("cargo::rerun-if-env-changed=SM_VERSION");
    println!("cargo::rerun-if-changed=.release-please-manifest.json");
    let version = match env::var("SM_VERSION") {
        Ok(v) if !v.is_empty() => v,
        _ => next_dev(&fs::read_to_string(".release-please-manifest.json").unwrap()),
    };
    println!("cargo::rustc-env=SM_VERSION={version}");
}

/// `{ ".": "1.9.1" }` as `1.9.2-dev`.
fn next_dev(manifest: &str) -> String {
    let released = manifest.split('"').nth(3).unwrap_or_default();
    let parts: Vec<u64> = released.split('.').filter_map(|p| p.parse().ok()).collect();
    let [major, minor, patch] = parts[..] else {
        panic!(".release-please-manifest.json names no x.y.z: {released:?}");
    };
    format!("{major}.{minor}.{}-dev", patch + 1)
}
