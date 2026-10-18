//! Journal entries from `journalctl --output=json`.

use std::collections::HashSet;

use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// Microseconds since the epoch.
    pub time: i64,
    pub ident: String,
    pub message: String,
    /// The message as `pattern` makes it, made once rather than at every look.
    kind: String,
}

impl Entry {
    pub fn new(time: i64, ident: &str, message: &str) -> Entry {
        let (ident, message) = (printable(ident), printable(message));
        Entry {
            time,
            ident,
            kind: pattern(&message),
            message,
        }
    }

    pub fn clock(&self) -> String {
        jiff::Timestamp::from_microsecond(self.time)
            .map(|t| {
                t.to_zoned(jiff::tz::TimeZone::system())
                    .strftime("%H:%M:%S")
                    .to_string()
            })
            .unwrap_or_default()
    }
}

pub fn parse(out: &str) -> Vec<Entry> {
    out.lines()
        .filter_map(|line| {
            let v: Value = serde_json::from_str(line).ok()?;
            let text = |key: &str| match &v[key] {
                Value::String(s) => s.clone(),
                // Fields that are not valid UTF-8 come as an array of bytes.
                Value::Array(bytes) => String::from_utf8_lossy(
                    &bytes
                        .iter()
                        .filter_map(|b| b.as_u64().map(|b| b as u8))
                        .collect::<Vec<_>>(),
                )
                .into_owned(),
                _ => String::new(),
            };
            let ident = Some(text("SYSLOG_IDENTIFIER"))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| text("_COMM"));
            Some(Entry::new(
                text("__REALTIME_TIMESTAMP").parse().ok()?,
                &ident,
                &text("MESSAGE"),
            ))
        })
        .collect()
}

/// Without the colours and controls a program wrote for a terminal.
fn printable(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => {
                if chars.next() == Some('[') {
                    chars.by_ref().find(|c| ('@'..='~').contains(c));
                }
            }
            '\t' => out.push(' '),
            '\n' => out.push('\n'),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// The kinds of the texts the person chose to hide, made once for a look at many entries.
pub fn kinds(hidden: &[String]) -> HashSet<String> {
    hidden.iter().map(|h| pattern(h)).collect()
}

impl Entry {
    /// Whether the person chose to hide it: it is one of the texts, numbers aside.
    pub fn hidden(&self, kinds: &HashSet<String>) -> bool {
        kinds.contains(&self.kind)
    }
}

/// A message with what changes between two of its kind made one sign: numbers, addresses,
/// hashes. Hiding one hides the next of its kind, with another PID or another address.
pub fn pattern(message: &str) -> String {
    let mut out = String::new();
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        let numeric = word.chars().any(|c| c.is_ascii_digit());
        let hexish = numeric && word.len() >= 6 && word.chars().all(|c| c.is_ascii_hexdigit());
        if hexish
            || numeric
                && word
                    .trim_start_matches("0x")
                    .chars()
                    .all(|c| c.is_ascii_hexdigit())
        {
            out.push('#');
        } else if numeric {
            // A word with digits in it: each run of digits goes.
            let mut digits = false;
            for c in word.chars() {
                if c.is_ascii_digit() {
                    if !digits {
                        out.push('#');
                    }
                    digits = true;
                } else {
                    out.push(c);
                    digits = false;
                }
            }
        } else {
            out.push_str(word);
        }
        word.clear();
    };
    for c in message.chars() {
        if c.is_alphanumeric() || c == '_' {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

pub fn visible<'a>(entries: &'a [Entry], hidden: &[String]) -> impl Iterator<Item = &'a Entry> {
    let kinds = kinds(hidden);
    entries.iter().filter(move |e| !e.hidden(&kinds))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_text_and_byte_messages_and_hides_by_message() {
        let out = concat!(
            r#"{"__REALTIME_TIMESTAMP":"1792086079523448","SYSLOG_IDENTIFIER":"kernel","MESSAGE":"ACPI Error: AE_NOT_FOUND"}"#,
            "\n",
            r#"{"__REALTIME_TIMESTAMP":"1792086079523449","_COMM":"foo","MESSAGE":[104,105,255]}"#,
            "\n"
        );
        let e = parse(out);
        assert_eq!(e.len(), 2);
        assert_eq!(
            (e[0].ident.as_str(), e[1].ident.as_str()),
            ("kernel", "foo")
        );
        assert!(e[1].message.starts_with("hi"));
        let hidden = vec!["ACPI Error: AE_NOT_FOUND".to_string()];
        assert_eq!(visible(&e, &hidden).count(), 1);
    }

    #[test]
    fn hides_the_next_message_of_its_kind() {
        let a = "usb 1-2: device descriptor read/64, error -71 at 0x7f3a2c00 (pid 4312)";
        let b = "usb 3-1: device descriptor read/64, error -71 at 0x7f3a9e10 (pid 977)";
        assert_eq!(pattern(a), pattern(b));
        assert_eq!(
            pattern("fs-verity: hash 6dd0af06ad12 inode 759644"),
            "fs-verity: hash # inode #"
        );
        let entry = |m: &str| Entry::new(0, "kernel", m);
        assert!(entry(b).hidden(&kinds(&[pattern(a)])));
        assert!(!entry("usb 1-2: new device").hidden(&kinds(&[pattern(a)])));
    }

    #[test]
    fn hides_only_its_own_kind() {
        let entry = |m: &str| Entry::new(0, "kernel", m);
        assert!(!entry("usb 1-2: new device").hidden(&kinds(&["".into()])));
        assert!(!entry("error -71").hidden(&kinds(&[pattern("1")])));
        assert!(!entry("ACPI Error: AE_NOT_FOUND").hidden(&kinds(&["AE_NOT_FOUND".into()])));
    }

    #[test]
    fn drops_the_colours_and_controls_meant_for_a_terminal() {
        let e = Entry::new(
            0,
            "sys\x07temd",
            "\x1b[0;1;31mFailed\x1b[0m\tto start\nsecond line",
        );
        assert_eq!(e.ident, "systemd");
        assert_eq!(e.message, "Failed to start\nsecond line");
    }
}
