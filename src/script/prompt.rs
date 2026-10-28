//! Questions a run asks, recognised in the C locale the scripts run in.

use super::sudo;
use crate::i18n::{t, tf};

#[derive(Clone, Debug, PartialEq)]
pub enum Prompt {
    Password,
    YesNo {
        question: String,
        yes: bool,
    },
    /// Numbered choices, such as pacman's providers, where the answer is the number.
    Choice {
        question: String,
        options: Vec<(String, String)>,
        default: String,
    },
    /// Free input, such as the menus of AUR helpers, where enter keeps their default.
    Text {
        question: String,
    },
    /// A secret only the person knows, such as a token or a passphrase: hidden as it is typed,
    /// and held nowhere.
    Secret {
        question: String,
    },
    /// ssh meeting a host it has no key of, answered in words.
    HostKey {
        host: String,
        fingerprint: String,
    },
}

impl Prompt {
    /// Finds the question on the unfinished line, with the finished lines before it.
    pub fn detect(current: &str, recent: &[String]) -> Option<Prompt> {
        if current.ends_with(sudo::PROMPT) {
            return Some(Prompt::Password);
        }
        let line = current.trim_end();
        // git asking for credentials, ssh for the passphrase of a key.
        if line.ends_with(':')
            && (line.starts_with("Password for '")
                || line.starts_with("Enter passphrase for key '"))
        {
            return Some(Prompt::Secret {
                question: line.to_string(),
            });
        }
        if line.starts_with("Username for '") && line.ends_with("':") {
            return Some(Prompt::Text {
                question: line.to_string(),
            });
        }
        if line.ends_with("(yes/no/[fingerprint])?") {
            let after = |lead: &str| {
                recent
                    .iter()
                    .rev()
                    .find_map(|l| l.split_once(lead).map(|(_, rest)| rest.to_string()))
            };
            let host = after("authenticity of host '")
                .and_then(|r| r.split_once('\'').map(|(h, _)| h.to_string()))
                .unwrap_or_default();
            let fingerprint = after("key fingerprint is ")
                .map(|f| f.trim_end_matches('.').to_string())
                .unwrap_or_default();
            return Some(Prompt::HostKey { host, fingerprint });
        }
        for (tag, yes) in [
            ("[Y/n]", true),
            ("[y/N]", false),
            ("[Y|n]", true),
            ("[y|N]", false),
        ] {
            if let Some(q) = line
                .strip_suffix(tag)
                .or_else(|| line.strip_suffix(&format!("{tag}:")))
            {
                return Some(Prompt::YesNo {
                    question: clean(q),
                    yes,
                });
            }
        }
        if let Some(rest) = line.strip_prefix("Enter a number (default=")
            && let Some(default) = rest.strip_suffix("):")
        {
            let at = recent
                .iter()
                .rposition(|l| l.starts_with(":: There are"))
                .or_else(|| recent.iter().rposition(|l| l.starts_with(":: ")));
            let question = at.map(|i| clean(&recent[i])).unwrap_or_default();
            let options = recent[at.map_or(0, |i| i + 1)..]
                .iter()
                .flat_map(|l| numbered(l))
                .collect();
            return Some(Prompt::Choice {
                question,
                options,
                default: default.to_string(),
            });
        }
        if line.starts_with("Enter a selection (default=") || line == "==>" {
            let question = recent
                .iter()
                .rev()
                .find(|l| l.starts_with(":: There are"))
                .or_else(|| {
                    recent
                        .iter()
                        .rev()
                        .find(|l| l.starts_with(":: ") || l.starts_with("==> "))
                })
                .map(|l| clean(l))
                .unwrap_or_default();
            return Some(Prompt::Text { question });
        }
        None
    }

    /// The answer given without asking, where the person already said yes before the run.
    pub fn automatic(&self) -> Option<&'static str> {
        let Prompt::YesNo { question, .. } = self else {
            return None;
        };
        [
            "Proceed with installation?",
            "Proceed with download?",
            "Do you want to remove these packages?",
        ]
        .iter()
        .any(|q| question.starts_with(q))
        .then_some("y")
    }
}

impl Prompt {
    /// The question in the interface language, where its wording is known.
    pub fn friendly(&self) -> String {
        match self {
            Prompt::Password => t("Authentication required").to_string(),
            Prompt::YesNo { question, .. } | Prompt::Choice { question, .. } => translate(question),
            Prompt::Text { question } | Prompt::Secret { question } => ask(question),
            Prompt::HostKey { host, fingerprint } => tf(
                "Trust the host {host}? The fingerprint of its key is {fingerprint}.",
                &[("host", host), ("fingerprint", fingerprint)],
            ),
        }
    }

    /// What the box of a typed answer shows while it is empty.
    pub fn placeholder(&self) -> &'static str {
        match self {
            Prompt::Password => t("Password"),
            Prompt::Secret { question } if question.starts_with("Password for '") => {
                t("Password or token")
            }
            Prompt::Secret { .. } => t("Passphrase"),
            Prompt::Text { question } if question.starts_with("Username for '") => t("User name"),
            Prompt::Text { .. } => t("Empty for default"),
            Prompt::YesNo { .. } | Prompt::Choice { .. } | Prompt::HostKey { .. } => "",
        }
    }

    /// The answers of a question to choose from, yes first, in the words the program takes.
    pub fn answers(&self) -> Option<[&'static str; 2]> {
        match self {
            Prompt::YesNo { .. } => Some(["y", "n"]),
            Prompt::HostKey { .. } => Some(["yes", "no"]),
            _ => None,
        }
    }

    /// The answer the cursor starts on.
    pub fn default_answer(&self) -> Option<&'static str> {
        let [yes, no] = self.answers()?;
        match self {
            Prompt::YesNo { yes: true, .. } => Some(yes),
            _ => Some(no),
        }
    }
}

/// git's and ssh's questions for a text, worded for people. What is in quotes is kept.
fn ask(q: &str) -> String {
    let quoted = || q.split('\'').nth(1).unwrap_or_default();
    if q.starts_with("Username for '") {
        return tf("The user name for {url}", &[("url", &quoted())]);
    }
    if q.starts_with("Password for '") {
        return tf("The password or token for {url}", &[("url", &quoted())]);
    }
    if q.starts_with("Enter passphrase for key '") {
        return tf("The passphrase of the key {key}", &[("key", &quoted())]);
    }
    q.to_string()
}

fn translate(q: &str) -> String {
    let q = q.trim();
    if let Some(rest) = q.strip_prefix("Replace ")
        && let Some((old, new)) = rest.trim_end_matches('?').split_once(" with ")
    {
        return tf("Replace {old} with {new}?", &[("old", &old), ("new", &new)]);
    }
    if let Some((pair, remove)) = q.split_once(" are in conflict")
        && let Some((a, b)) = pair.split_once(" and ")
        && let Some(remove) = remove
            .rsplit_once("Remove ")
            .map(|(_, r)| r.trim_end_matches('?'))
    {
        return tf(
            "{a} and {b} conflict. Remove {remove}?",
            &[("a", &a), ("b", &b), ("remove", &remove)],
        );
    }
    if let Some(rest) = q.strip_prefix("Import PGP key ") {
        let (key, owner) = rest
            .trim_end_matches('?')
            .split_once(", ")
            .unwrap_or((rest, ""));
        return tf(
            "Import the PGP key {key} of {owner}?",
            &[("key", &key), ("owner", &owner.trim_matches('"'))],
        );
    }
    if let Some(rest) = q.strip_prefix("There are ")
        && let Some((_, what)) = rest.split_once(" providers available for ")
    {
        return tf(
            "Which package should provide {what}?",
            &[("what", &what.trim_end_matches(':'))],
        );
    }
    q.to_string()
}

fn clean(s: &str) -> String {
    s.trim()
        .trim_start_matches("::")
        .trim_start_matches("==>")
        .trim()
        .to_string()
}

/// `1) jre-openjdk  2) jre17-openjdk` as pairs of answer and label.
fn numbered(line: &str) -> Vec<(String, String)> {
    let words: Vec<&str> = line.split_whitespace().collect();
    words
        .windows(2)
        .filter_map(|w| {
            let n = w[0].strip_suffix(')')?;
            n.chars()
                .all(|c| c.is_ascii_digit())
                .then(|| (n.to_string(), w[1].to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_pacman_questions() {
        assert_eq!(
            Prompt::detect(":: Replace foo with extra/bar? [Y/n] ", &[]),
            Some(Prompt::YesNo {
                question: "Replace foo with extra/bar?".into(),
                yes: true
            })
        );
        assert_eq!(
            Prompt::detect(":: foo and bar are in conflict. Remove bar? [y/N] ", &[]),
            Some(Prompt::YesNo {
                question: "foo and bar are in conflict. Remove bar?".into(),
                yes: false
            })
        );
        let recent = [
            ":: There are 2 providers available for java-runtime:".to_string(),
            ":: Repository extra".into(),
            "   1) jre-openjdk  2) jre17-openjdk".into(),
            "".into(),
        ];
        assert_eq!(
            Prompt::detect("Enter a number (default=1): ", &recent),
            Some(Prompt::Choice {
                question: "There are 2 providers available for java-runtime:".into(),
                options: vec![
                    ("1".into(), "jre-openjdk".into()),
                    ("2".into(), "jre17-openjdk".into())
                ],
                default: "1".into()
            })
        );
    }

    #[test]
    fn recognises_sudo_and_helper_menus_and_nothing_else() {
        assert_eq!(
            Prompt::detect(&format!("x{}", sudo::PROMPT), &[]),
            Some(Prompt::Password)
        );
        let recent = [
            "==> Packages to cleanBuild?".to_string(),
            "==> [N]one [A]ll [Ab]ort".into(),
        ];
        assert_eq!(
            Prompt::detect("==> ", &recent),
            Some(Prompt::Text {
                question: "[N]one [A]ll [Ab]ort".into()
            })
        );
        let group = [
            ":: There are 3 members in group gnome:".to_string(),
            ":: Repository extra".into(),
            "   1) baobab  2) eog  3) evince".into(),
            String::new(),
        ];
        assert_eq!(
            Prompt::detect("Enter a selection (default=all): ", &group),
            Some(Prompt::Text {
                question: "There are 3 members in group gnome:".into()
            })
        );
        assert_eq!(
            Prompt::detect("( 3/10) upgrading firefox  [####--]  60%", &[]),
            None
        );
        assert_eq!(Prompt::detect("", &[]), None);
    }

    #[test]
    fn words_known_questions_plainly_and_keeps_the_rest() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let ask = |q: &str| {
            Prompt::YesNo {
                question: q.into(),
                yes: true,
            }
            .friendly()
        };
        assert_eq!(
            ask("Replace foo with extra/bar?"),
            "Replace foo with extra/bar?"
        );
        assert_eq!(
            ask("foo and bar are in conflict (baz). Remove bar?"),
            "foo and bar conflict. Remove bar?"
        );
        assert_eq!(
            ask("Import PGP key 1234ABCD, \"Jane <j@x>\"?"),
            "Import the PGP key 1234ABCD of Jane <j@x>?"
        );
        assert_eq!(ask("Something new?"), "Something new?");
    }

    #[test]
    fn recognises_what_git_and_ssh_ask() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let user = Prompt::detect("Username for 'https://github.com': ", &[]).unwrap();
        assert_eq!(user.friendly(), "The user name for https://github.com");
        let token = Prompt::detect("Password for 'https://tux@github.com': ", &[]).unwrap();
        assert!(matches!(token, Prompt::Secret { .. }));
        assert_eq!(
            token.friendly(),
            "The password or token for https://tux@github.com"
        );
        let key = Prompt::detect(
            "Enter passphrase for key '/home/tux/.ssh/id_ed25519': ",
            &[],
        );
        assert_eq!(
            key.unwrap().friendly(),
            "The passphrase of the key /home/tux/.ssh/id_ed25519"
        );
        let recent = [
            "The authenticity of host 'github.com (140.82.121.3)' can't be established."
                .to_string(),
            "ED25519 key fingerprint is SHA256:+DiY3wvvV6TuJJhbpZisF/zLDA0zPMSvHdkr4UvCOqU.".into(),
            "This key is not known by any other names.".into(),
        ];
        let host = Prompt::detect(
            "Are you sure you want to continue connecting (yes/no/[fingerprint])? ",
            &recent,
        )
        .unwrap();
        assert_eq!(
            host,
            Prompt::HostKey {
                host: "github.com (140.82.121.3)".into(),
                fingerprint: "SHA256:+DiY3wvvV6TuJJhbpZisF/zLDA0zPMSvHdkr4UvCOqU".into(),
            }
        );
        assert_eq!(host.answers(), Some(["yes", "no"]));
        assert_eq!(
            host.default_answer(),
            Some("no"),
            "a new key is trusted on purpose"
        );
        assert_eq!(host.automatic(), None);
        assert!(host.friendly().contains("github.com") && host.friendly().contains("SHA256:"));
        assert_eq!(
            Prompt::detect("Password: ", &[]),
            None,
            "a login of its own stays in the terminal"
        );
    }

    #[test]
    fn an_empty_answer_box_names_what_it_takes() {
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        for (line, shows) in [
            (format!("x{}", sudo::PROMPT), "Password"),
            (
                "Password for 'https://tux@github.com': ".into(),
                "Password or token",
            ),
            (
                "Enter passphrase for key '/home/tux/.ssh/id_ed25519': ".into(),
                "Passphrase",
            ),
            ("Username for 'https://github.com': ".into(), "User name"),
            ("==> ".into(), "Empty for default"),
        ] {
            let p = Prompt::detect(&line, &[]).unwrap();
            assert_eq!(p.placeholder(), shows, "{line}");
        }
        let chosen = Prompt::detect(":: Proceed with installation? [Y/n] ", &[]).unwrap();
        assert_eq!(chosen.placeholder(), "", "chosen, not typed");
    }

    #[test]
    fn answers_only_the_confirmations_already_given() {
        let proceed = Prompt::detect(":: Proceed with installation? [Y/n] ", &[]).unwrap();
        assert_eq!(proceed.automatic(), Some("y"));
        let conflict =
            Prompt::detect(":: foo and bar are in conflict. Remove bar? [y/N] ", &[]).unwrap();
        assert_eq!(conflict.automatic(), None);
    }
}
