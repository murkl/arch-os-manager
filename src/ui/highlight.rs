//! A PKGBUILD in colour: what it sets, what it runs, and above all what reaches the network,
//! elevates or deletes, so a reader sees at once what installing it does.

use super::Tone;
use super::detail::Span;

/// Commands that reach the network, elevate, run code or delete.
const RISKY: &[&str] = &[
    "sudo", "curl", "wget", "git", "eval", "rm", "sh", "bash", "python", "python3", "npm", "pip",
    "su",
];
/// Commands that put files in place.
const PLACING: &[&str] = &[
    "install", "cp", "mv", "ln", "mkdir", "chmod", "chown", "touch", "tar", "bsdtar", "patch",
    "sed",
];
const KEYWORDS: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "in", "do", "done", "while", "until", "case",
    "esac", "function", "return", "local", "export", "cd",
];

pub fn bash(text: &str) -> Vec<Vec<Span>> {
    text.lines().map(line).collect()
}

fn push(out: &mut Vec<Span>, text: &str, tone: Tone, bold: bool) {
    if text.is_empty() {
        return;
    }
    match out.last_mut() {
        Some(last) if last.1 == tone && last.2 == bold => last.0.push_str(text),
        _ => out.push((text.to_string(), tone, bold)),
    }
}

fn line(src: &str) -> Vec<Span> {
    let mut out = Vec::new();
    let indent = src.len() - src.trim_start().len();
    push(&mut out, &src[..indent], Tone::Normal, false);
    let rest = &src[indent..];
    // A function: build() {, package() {
    if let Some(name) = rest
        .strip_suffix("() {")
        .filter(|n| n.chars().all(|c| c.is_alphanumeric() || c == '_'))
    {
        push(&mut out, name, Tone::Warn, true);
        push(&mut out, "() {", Tone::Muted, false);
        return out;
    }
    let chars: Vec<char> = rest.chars().collect();
    let mut i = 0;
    let mut command = true;
    while i < chars.len() {
        let c = chars[i];
        let start = i;
        match c {
            '#' if start == 0 || chars[start - 1].is_whitespace() => {
                push(
                    &mut out,
                    &chars[i..].iter().collect::<String>(),
                    Tone::Muted,
                    false,
                );
                break;
            }
            '\'' => {
                i += 1;
                while i < chars.len() && chars[i] != '\'' {
                    i += 1;
                }
                i = (i + 1).min(chars.len());
                push(
                    &mut out,
                    &chars[start..i].iter().collect::<String>(),
                    Tone::Ok,
                    false,
                );
                command = false;
            }
            '"' => {
                i += 1;
                push(&mut out, "\"", Tone::Ok, false);
                let mut piece = start + 1;
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] == '\\' {
                        i += 2;
                        continue;
                    }
                    if chars[i] == '$' {
                        push(
                            &mut out,
                            &chars[piece..i].iter().collect::<String>(),
                            Tone::Ok,
                            false,
                        );
                        let end = variable(&chars, i);
                        push(
                            &mut out,
                            &chars[i..end].iter().collect::<String>(),
                            Tone::Accent,
                            false,
                        );
                        i = end;
                        piece = i;
                        continue;
                    }
                    i += 1;
                }
                i = (i + 1).min(chars.len());
                push(
                    &mut out,
                    &chars[piece.min(i)..i].iter().collect::<String>(),
                    Tone::Ok,
                    false,
                );
                command = false;
            }
            '$' => {
                i = variable(&chars, i);
                push(
                    &mut out,
                    &chars[start..i].iter().collect::<String>(),
                    Tone::Accent,
                    false,
                );
                command = false;
            }
            ';' | '|' | '&' | '(' | ')' | '{' | '}' | '=' => {
                i += 1;
                push(&mut out, &c.to_string(), Tone::Muted, false);
                command = matches!(c, ';' | '|' | '&' | '(' | '{');
            }
            c if c.is_whitespace() => {
                i += 1;
                push(&mut out, &c.to_string(), Tone::Normal, false);
            }
            _ => {
                while i < chars.len()
                    && !chars[i].is_whitespace()
                    && !"'\"$;|&(){}=".contains(chars[i])
                {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                let assigned = chars.get(i) == Some(&'=') && start == 0;
                let (tone, bold) = if assigned {
                    (Tone::Accent, true)
                } else if KEYWORDS.contains(&word.as_str()) {
                    (Tone::Normal, true)
                } else if command && RISKY.contains(&word.as_str()) {
                    (Tone::Error, true)
                } else if command && PLACING.contains(&word.as_str()) {
                    (Tone::Warn, false)
                } else if word.starts_with("http://") || word.starts_with("https://") {
                    (Tone::Accent, false)
                } else {
                    (Tone::Normal, false)
                };
                push(&mut out, &word, tone, bold);
                command = KEYWORDS.contains(&word.as_str()) && word != "in";
            }
        }
    }
    out
}

/// The end of `$name`, `${…}` or `$(…)` starting at `i`.
fn variable(chars: &[char], i: usize) -> usize {
    let mut j = i + 1;
    match chars.get(j) {
        Some('{') | Some('(') => {
            let close = if chars[j] == '{' { '}' } else { ')' };
            while j < chars.len() && chars[j] != close {
                j += 1;
            }
            (j + 1).min(chars.len())
        }
        _ => {
            while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            j.max(i + 1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tones(src: &str) -> Vec<(String, Tone)> {
        line(src)
            .into_iter()
            .map(|(t, tone, _)| (t, tone))
            .collect()
    }

    #[test]
    fn colours_what_a_pkgbuild_sets_runs_and_risks() {
        assert_eq!(
            tones(r#"pkgname=foo # the name"#),
            [
                ("pkgname".into(), Tone::Accent),
                ("=".into(), Tone::Muted),
                ("foo ".into(), Tone::Normal),
                ("# the name".into(), Tone::Muted)
            ]
        );
        assert_eq!(line("package() {")[0], ("package".into(), Tone::Warn, true));
        let risky = tones(r#"  curl -o x "$srcdir/${pkgname}.tar""#);
        assert!(risky.contains(&("curl".into(), Tone::Error)), "{risky:?}");
        assert!(
            risky.contains(&("$srcdir".into(), Tone::Accent)),
            "{risky:?}"
        );
        assert!(
            risky.contains(&("${pkgname}".into(), Tone::Accent)),
            "{risky:?}"
        );
        let placing = tones(r#"  install -Dm755 bin "$pkgdir/usr/bin/foo""#);
        assert_eq!(placing[1], ("install".into(), Tone::Warn));
        assert!(
            tones("source=('https://x/y')")
                .iter()
                .any(|(t, tone)| t == "'https://x/y'" && *tone == Tone::Ok)
        );
    }

    #[test]
    fn keeps_every_character() {
        let src = r#"  [ -n "$x" ] && echo 'a # b' "c\"d" # e"#;
        let joined: String = line(src).into_iter().map(|(t, ..)| t).collect();
        assert_eq!(joined, src);
    }
}
