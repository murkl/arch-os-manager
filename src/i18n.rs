//! English source strings, German from `locales/de.po`.

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::En, Lang::De];

    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::De => "de",
        }
    }

    /// The language's own name, so it can be found without reading the current one.
    pub fn name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::De => "Deutsch",
        }
    }

    pub fn from_code(code: &str) -> Option<Lang> {
        Lang::ALL.into_iter().find(|l| l.code() == code)
    }

    /// The language of messages as the environment sets it, the way gettext reads it.
    pub fn detect() -> Lang {
        let value = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .filter_map(|k| std::env::var(k).ok())
            .find(|v| !v.is_empty())
            .unwrap_or_default();
        if value.starts_with("de") {
            Lang::De
        } else {
            Lang::En
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// Held by tests that switch the language or read English text, which share one process.
#[cfg(test)]
pub static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
static DE: OnceLock<HashMap<String, &'static str>> = OnceLock::new();

pub const DE_PO: &str = include_str!("../locales/de.po");

pub fn set(lang: Lang) {
    CURRENT.store(lang as u8, Ordering::Relaxed);
}

pub fn current() -> Lang {
    if CURRENT.load(Ordering::Relaxed) == Lang::De as u8 {
        Lang::De
    } else {
        Lang::En
    }
}

pub fn t(msgid: &str) -> &str {
    if current() == Lang::En {
        return msgid;
    }
    let catalog = DE.get_or_init(|| {
        parse_po(DE_PO)
            .into_iter()
            .map(|(k, v)| (k, &*Box::leak(v.into_boxed_str())))
            .collect()
    });
    catalog.get(msgid).copied().unwrap_or(msgid)
}

/// Translates, then fills `{name}` placeholders.
pub fn tf(msgid: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut out = t(msgid).to_string();
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), &value.to_string());
    }
    out
}

/// The singular or the plural by `n`, as English and German count, then the placeholders.
pub fn tn(one: &str, many: &str, n: usize, args: &[(&str, &dyn Display)]) -> String {
    let mut all: Vec<(&str, &dyn Display)> = vec![("n", &n)];
    all.extend_from_slice(args);
    tf(if n == 1 { one } else { many }, &all)
}

/// msgid → msgstr, without the header and without untranslated entries.
pub fn parse_po(src: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let (mut id, mut text) = (String::new(), String::new());
    let mut target: Option<bool> = None; // Some(false) msgid, Some(true) msgstr
    let mut flush = |id: &mut String, text: &mut String| {
        if !id.is_empty() && !text.is_empty() {
            map.insert(std::mem::take(id), std::mem::take(text));
        }
        id.clear();
        text.clear();
    };
    for line in src.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("msgid ") {
            flush(&mut id, &mut text);
            id = unquote(rest);
            target = Some(false);
        } else if let Some(rest) = line.strip_prefix("msgstr ") {
            text = unquote(rest);
            target = Some(true);
        } else if line.starts_with('"') {
            match target {
                Some(false) => id += &unquote(line),
                Some(true) => text += &unquote(line),
                None => {}
            }
        } else if line.is_empty() || line.starts_with('#') {
            target = None;
        }
    }
    flush(&mut id, &mut text);
    map
}

fn unquote(s: &str) -> String {
    let inner = s.trim().trim_start_matches('"').trim_end_matches('"');
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multiline_entries_and_escapes() {
        let po = r#"
msgid ""
msgstr "Content-Type: text/plain; charset=UTF-8\n"

msgid "One"
msgstr "Eins"

msgid ""
"Two "
"lines\n"
msgstr ""
"Zwei "
"\"Zeilen\"\n"

msgid "Open"
msgstr ""
"#;
        let map = parse_po(po);
        assert_eq!(map.get("One").map(String::as_str), Some("Eins"));
        assert_eq!(
            map.get("Two lines\n").map(String::as_str),
            Some("Zwei \"Zeilen\"\n")
        );
        assert!(
            !map.contains_key("Open"),
            "untranslated entries fall back to English"
        );
        assert!(!map.contains_key(""), "the header is no entry");
    }
}

/// A string literal's text, from just after its opening quote, and what follows it.
#[cfg(test)]
fn literal_at(src: &str) -> (String, &str) {
    let mut text = String::new();
    let mut chars = src.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return (text, &src[i + 1..]),
            '\\' => match chars.next() {
                Some((_, 'n')) => text.push('\n'),
                Some((_, other)) => text.push(other),
                None => {}
            },
            _ => text.push(c),
        }
    }
    (text, "")
}

/// Every msgid the program uses: the literals given to t and tf, and the texts of the scripts' yaml.
#[cfg(test)]
pub fn used() -> std::collections::BTreeSet<String> {
    let mut ids = std::collections::BTreeSet::new();
    let mut stack = vec![std::path::PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src"
    ))];
    while let Some(path) = stack.pop() {
        if path.is_dir() {
            stack.extend(std::fs::read_dir(&path).unwrap().map(|e| e.unwrap().path()));
            continue;
        }
        let src = std::fs::read_to_string(&path).unwrap();
        // Spelled apart, so this line is not found as a call itself.
        for call in [concat!("t", "("), concat!("tf", "("), concat!("tn", "(")] {
            for (i, _) in src.match_indices(call) {
                // Only calls, not words ending in t such as `print(`.
                let before = src[..i].chars().last().unwrap_or(' ');
                if before.is_alphanumeric() || before == '_' {
                    continue;
                }
                // A literal, possibly on the next line where rustfmt put it.
                // tn takes two: the singular, then the plural.
                let mut rest = &src[i + call.len()..];
                for _ in 0..if call.starts_with("tn") { 2 } else { 1 } {
                    let Some(literal) = rest
                        .trim_start()
                        .trim_start_matches(',')
                        .trim_start()
                        .strip_prefix('"')
                    else {
                        break;
                    };
                    let (text, after) = literal_at(literal);
                    ids.insert(text);
                    rest = after;
                }
            }
        }
    }
    let mut add = |s: &str| {
        ids.insert(s.to_string());
    };
    let mut scripts: Vec<&str> = crate::script::op::ALL.iter().map(|o| o.script).collect();
    scripts.push("lib");
    scripts.dedup();
    for script in scripts {
        let m = crate::script::meta(script);
        for o in m.ops.values() {
            add(&o.title);
            o.confirm
                .iter()
                .chain(o.steps.values())
                .for_each(|s| add(s));
        }
        m.errors.values().for_each(|s| add(s));
        m.hints.values().for_each(|h| add(&h.text));
        m.probe
            .values()
            .flat_map(|kind| kind.values())
            .for_each(|p| add(&p.adds));
        m.facts.values().for_each(|s| add(s));
    }
    ids
}

#[cfg(test)]
mod catalog {
    use super::*;

    fn placeholders(s: &str) -> Vec<String> {
        let mut v: Vec<String> = s
            .split('{')
            .skip(1)
            .filter_map(|p| p.split_once('}').map(|(n, _)| n.to_string()))
            .collect();
        v.sort();
        v
    }

    #[test]
    fn german_covers_every_text_exactly_and_keeps_its_placeholders() {
        let de = parse_po(DE_PO);
        let used = used();
        let missing: Vec<&String> = used.iter().filter(|id| !de.contains_key(*id)).collect();
        assert!(
            missing.is_empty(),
            "not in de.po:\n{}",
            missing
                .iter()
                .map(|m| format!("{m:?}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        let unused: Vec<&String> = de.keys().filter(|id| !used.contains(*id)).collect();
        assert!(unused.is_empty(), "in de.po but used nowhere: {unused:?}");
        for (id, text) in &de {
            assert_eq!(
                placeholders(id),
                placeholders(text),
                "placeholders differ: {id:?}"
            );
        }
    }
}
