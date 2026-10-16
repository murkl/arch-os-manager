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
    /// Numbered choices, such as pacman's providers; the answer is the number.
    Choice {
        question: String,
        options: Vec<(String, String)>,
        default: String,
    },
    /// Free input, such as the menus of AUR helpers; enter keeps their default.
    Text {
        question: String,
    },
}

impl Prompt {
    /// Finds the question on the unfinished line, with the finished lines before it.
    pub fn detect(current: &str, recent: &[String]) -> Option<Prompt> {
        if current.ends_with(sudo::PROMPT) {
            return Some(Prompt::Password);
        }
        let line = current.trim_end();
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
                .find(|l| l.starts_with(":: ") || l.starts_with("==> "))
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
            Prompt::Password => t("The password for sudo").to_string(),
            Prompt::YesNo { question, .. } | Prompt::Choice { question, .. } => translate(question),
            Prompt::Text { question } => question.clone(),
        }
    }
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
            "{a} and {b} cannot be installed together. Remove {remove}?",
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
            "foo and bar cannot be installed together. Remove bar?"
        );
        assert_eq!(
            ask("Import PGP key 1234ABCD, \"Jane <j@x>\"?"),
            "Import the PGP key 1234ABCD of Jane <j@x>?"
        );
        assert_eq!(ask("Something new?"), "Something new?");
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
