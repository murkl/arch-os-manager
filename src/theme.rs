//! Themes: a name and eight colours, from `themes/` in the binary and `~/.arch-os/themes`.

use serde::{Deserialize, Deserializer};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Rgb, D::Error> {
        let s = String::deserialize(d)?;
        parse_hex(&s).ok_or_else(|| serde::de::Error::custom(format!("{s} is not #RRGGBB")))
    }
}

fn parse_hex(s: &str) -> Option<Rgb> {
    let hex = s.strip_prefix('#')?;
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some(Rgb(byte(0)?, byte(2)?, byte(4)?))
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Colors {
    pub background: Rgb,
    pub surface: Rgb,
    pub muted: Rgb,
    pub text: Rgb,
    pub accent: Rgb,
    pub success: Rgb,
    pub warning: Rgb,
    pub error: Rgb,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    name: String,
    colors: Colors,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    /// The file name without `.yaml`, which settings.yaml stores.
    pub id: String,
    pub name: String,
    pub colors: Colors,
}

pub const DEFAULT: &str = "nord";

fn parse(id: &str, src: &str) -> Result<Theme, String> {
    let f: File = serde_saphyr::from_str(src).map_err(|e| e.to_string())?;
    Ok(Theme {
        id: id.to_string(),
        name: f.name,
        colors: f.colors,
    })
}

/// Built-in themes and the user's, by name. A user file with a built-in's id replaces it.
/// The second list says which user files were skipped and why.
pub fn all() -> (Vec<Theme>, Vec<String>) {
    let mut themes: Vec<Theme> = crate::embedded::THEMES
        .iter()
        .filter_map(|(file, bytes)| {
            let id = file.strip_suffix(".yaml")?;
            Some(parse(id, std::str::from_utf8(bytes).ok()?).expect("built-in theme"))
        })
        .collect();
    let mut skipped = Vec::new();
    let mut files: Vec<_> = std::fs::read_dir(crate::paths::themes())
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "yaml"))
        .collect();
    files.sort();
    for path in files {
        let id = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|s| parse(&id, &s))
        {
            Ok(theme) => {
                themes.retain(|t| t.id != theme.id);
                themes.push(theme);
            }
            Err(e) => skipped.push(format!("{}: {e}", path.display())),
        }
    }
    themes.sort_by_key(|t| t.name.to_lowercase());
    (themes, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_ins_parse_and_nord_is_one() {
        let (themes, _) = all();
        assert!(themes.iter().any(|t| t.id == DEFAULT));
    }

    /// WCAG's contrast ratio, from 1 to 21.
    fn contrast(a: Rgb, b: Rgb) -> f64 {
        let luminance = |c: Rgb| {
            let linear = |v: u8| {
                let v = f64::from(v) / 255.0;
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(c.0) + 0.7152 * linear(c.1) + 0.0722 * linear(c.2)
        };
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    /// WCAG AA: 4.5 for text, 3 for what is told by its colour, on the background and on the
    /// selection, where muted turns to text. Muted only has to stand apart.
    #[test]
    fn every_built_in_reads_on_its_background_and_its_selection() {
        for (file, bytes) in crate::embedded::THEMES {
            let theme = parse(file, std::str::from_utf8(bytes).unwrap()).unwrap();
            let c = theme.colors;
            let tones = [
                ("text", c.text, 4.5),
                ("muted", c.muted, 2.0),
                ("accent", c.accent, 3.0),
                ("success", c.success, 3.0),
                ("warning", c.warning, 3.0),
                ("error", c.error, 3.0),
            ];
            for (slot, colour, least) in tones {
                let ratio = contrast(colour, c.background);
                assert!(
                    ratio >= least,
                    "{file}: {slot} has {ratio:.2}, needs {least}"
                );
                let ratio = contrast(colour, c.surface);
                assert!(
                    slot == "muted" || ratio >= least,
                    "{file}: {slot} has {ratio:.2} on the selection, needs {least}"
                );
            }
        }
    }

    #[test]
    fn built_ins_are_listed_by_name() {
        let names: Vec<String> = all().0.into_iter().map(|t| t.name.to_lowercase()).collect();
        assert!(names.is_sorted(), "{names:?}");
    }

    #[test]
    fn rejects_missing_unknown_and_malformed_colours() {
        let ok = "name: X\ncolors: {background: '#000000', surface: '#000000', muted: '#000000', text: '#000000', accent: '#000000', success: '#000000', warning: '#000000', error: '#000000'}";
        assert!(parse("x", ok).is_ok());
        assert!(parse("x", &ok.replace("error: '#000000'", "error: '#00000'")).is_err());
        assert!(parse("x", &ok.replace(", error: '#000000'", "")).is_err());
        assert!(parse("x", &ok.replace("name: X", "name: X\nauthor: me")).is_err());
    }
}
