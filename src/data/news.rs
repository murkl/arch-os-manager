//! The Arch Linux news feed, and its HTML as paragraphs for the terminal.

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub title: String,
    pub link: String,
    pub date: String,
    pub html: String,
}

pub fn parse(xml: &str) -> Result<Vec<Item>, String> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| e.to_string())?;
    let items = doc
        .descendants()
        .filter(|n| n.has_tag_name("item"))
        .map(|item| {
            let text = |tag: &str| {
                item.children()
                    .find(|c| c.has_tag_name(tag))
                    .and_then(|c| c.text())
                    .unwrap_or_default()
                    .trim()
                    .to_string()
            };
            let date = jiff::fmt::rfc2822::parse(&text("pubDate"))
                .map(|z| {
                    z.with_time_zone(jiff::tz::TimeZone::system())
                        .strftime("%Y-%m-%d")
                        .to_string()
                })
                .unwrap_or_default();
            Item {
                title: text("title"),
                link: text("link"),
                date,
                html: text("description"),
            }
        })
        .collect();
    Ok(items)
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Para(String),
    Item(String),
    /// Preformatted, one entry per line.
    Code(Vec<String>),
}

/// The few elements the news use, as blocks; links keep their address after the text.
pub fn blocks(html: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut item = false;
    let mut pre = false;
    let mut href: Option<String> = None;
    let mut link_text = String::new();
    let flush = |out: &mut Vec<Block>, text: &mut String, item: bool, pre: bool| {
        if pre {
            let lines: Vec<String> = text
                .trim_matches('\n')
                .lines()
                .map(str::to_string)
                .collect();
            if !lines.is_empty() {
                out.push(Block::Code(lines));
            }
        } else {
            let t = text.split_whitespace().collect::<Vec<_>>().join(" ");
            if !t.is_empty() {
                out.push(if item { Block::Item(t) } else { Block::Para(t) });
            }
        }
        text.clear();
    };
    let mut rest = html;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('<') {
            let end = after.find('>').unwrap_or(after.len());
            let tag = &after[..end];
            rest = after.get(end + 1..).unwrap_or("");
            let closing = tag.starts_with('/');
            let name: String = tag
                .trim_start_matches('/')
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect::<String>()
                .to_ascii_lowercase();
            match (name.as_str(), closing) {
                ("p" | "div" | "ul" | "ol" | "h1" | "h2" | "h3" | "h4" | "blockquote", _) => {
                    flush(&mut out, &mut text, item, pre);
                    item = false;
                }
                ("li", false) => {
                    flush(&mut out, &mut text, item, pre);
                    item = true;
                }
                ("li", true) => {
                    flush(&mut out, &mut text, item, pre);
                    item = false;
                }
                ("pre", open) => {
                    flush(&mut out, &mut text, item, pre);
                    pre = !open;
                }
                ("br", _) => text.push('\n'),
                ("a", false) => {
                    href = attribute(tag, "href");
                    link_text.clear();
                }
                ("a", true) => {
                    if let Some(url) = href.take()
                        && url != link_text.trim()
                        && url.starts_with("http")
                    {
                        text.push_str(&format!(" ({url})"));
                    }
                }
                _ => {}
            }
            continue;
        }
        let end = rest.find('<').unwrap_or(rest.len());
        let chunk = decode(&rest[..end]);
        if href.is_some() {
            link_text.push_str(&chunk);
        }
        text.push_str(&chunk);
        rest = &rest[end..];
    }
    flush(&mut out, &mut text, item, pre);
    out
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    let start = tag.find(&format!("{name}=\""))? + name.len() + 2;
    let end = tag[start..].find('"')? + start;
    Some(decode(&tag[start..end]))
}

fn decode(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';').filter(|e| *e < 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let c = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ => entity
                .strip_prefix("#x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match c {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_feed() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?><rss version="2.0"><channel><title>Arch Linux: Recent news updates</title>
<item><title>Manual intervention for foo</title><link>https://archlinux.org/news/foo/</link>
<description>&lt;p&gt;Run &lt;code&gt;pacman -Syu&lt;/code&gt;.&lt;/p&gt;</description><pubDate>Mon, 13 Oct 2025 07:22:45 +0000</pubDate></item>
</channel></rss>"#;
        let items = parse(xml).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Manual intervention for foo");
        assert!(items[0].date.starts_with("2025-10-1"));
        assert_eq!(
            blocks(&items[0].html),
            [Block::Para("Run pacman -Syu.".into())]
        );
    }

    #[test]
    fn turns_html_into_paragraphs_items_and_code() {
        let html = "<p>Read the <a href=\"https://wiki.archlinux.org/x\">wiki</a> &amp; act.</p>\
                    <ul><li>one</li><li>two&nbsp;words</li></ul><pre><code>pacman -Syu\npacman -Qk</code></pre><p>end</p>";
        assert_eq!(
            blocks(html),
            [
                Block::Para("Read the wiki (https://wiki.archlinux.org/x) & act.".into()),
                Block::Item("one".into()),
                Block::Item("two words".into()),
                Block::Code(vec!["pacman -Syu".into(), "pacman -Qk".into()]),
                Block::Para("end".into()),
            ]
        );
    }
}
