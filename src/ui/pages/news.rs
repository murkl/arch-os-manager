//! The Arch Linux news, read here rather than in a browser.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::data::news::{Block as Html, Item, blocks};
use crate::i18n::t;
use crate::ui::detail::{Block, Detail, loaded};
use crate::ui::icon;
use crate::ui::list::{Lead, List, Picked, Row};
use crate::ui::{Ctx, Flow, Key, Loaded, Page, Tone};

pub fn unread(ctx: &Ctx) -> usize {
    let read = ctx.state.news_read.as_deref().unwrap_or_default();
    ctx.shared.news.ready().map_or(0, |(items, _)| {
        items.iter().filter(|i| !read.contains(&i.link)).count()
    })
}

pub struct NewsPage {
    list: List,
}

impl NewsPage {
    pub fn new() -> NewsPage {
        NewsPage {
            list: List::default(),
        }
    }
}

impl Page for NewsPage {
    fn title(&self) -> String {
        t("News").into()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        let Some((items, offline)) = loaded(&ctx.shared.news, area, buf, ctx) else {
            return;
        };
        let read = ctx.state.news_read.as_deref().unwrap_or_default();
        let rows = items
            .iter()
            .map(|i| {
                let new = !read.contains(&i.link);
                Row::new(i.link.clone(), i.title.clone())
                    .detail(i.date.clone(), Tone::Muted)
                    .dot(if new { Tone::Warn } else { Tone::Muted })
            })
            .collect();
        self.list.set(rows);
        let mut list_area = area;
        if *offline {
            crate::ui::draw::put(
                buf,
                area.x,
                area.y,
                &crate::ui::draw::cut(
                    t("Offline: the news as last loaded."),
                    area.width.into(),
                    ctx.style.glyphs.ellipsis,
                ),
                area.width,
                ctx.style.tone(Tone::Muted),
            );
            list_area = Rect::new(
                area.x,
                area.y + 2,
                area.width,
                area.height.saturating_sub(2),
            );
        }
        self.list.draw(list_area, buf, ctx);
    }

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, String)> {
        super::list_hints(self.list.typing())
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        match self.list.key(key) {
            Picked::Open(link) => {
                let Some((items, _)) = ctx.shared.news.ready() else {
                    return Flow::Stay;
                };
                let Some(item) = items.iter().find(|i| i.link == link).cloned() else {
                    return Flow::Stay;
                };
                let read = ctx.state.news_read.get_or_insert_with(Vec::new);
                if !read.contains(&link) {
                    read.push(link);
                    // Only what the feed still lists needs remembering.
                    read.retain(|l| items.iter().any(|i| &i.link == l));
                    ctx.state.save();
                }
                Flow::Push(Box::new(ArticlePage::new(item)))
            }
            Picked::Used => Flow::Stay,
            Picked::Ignored => Flow::Ignored,
        }
    }

    fn typing(&self) -> bool {
        self.list.typing()
    }

    fn animating(&self, ctx: &Ctx) -> bool {
        matches!(ctx.shared.news, Loaded::Loading)
    }
}

pub struct ArticlePage {
    detail: Detail,
}

impl ArticlePage {
    pub fn new(item: Item) -> ArticlePage {
        let mut body = vec![
            Block::Heading(item.title.clone()),
            Block::Text(item.date.clone(), Tone::Muted),
        ];
        // The items of a list stand together; everything else has a line around it.
        let mut listing = false;
        for b in blocks(&item.html) {
            let listed = matches!(b, Html::Item(_));
            if !(listed && listing) {
                body.push(Block::Blank);
            }
            listing = listed;
            match b {
                Html::Para(text) => body.push(Block::Text(text, Tone::Normal)),
                Html::Item(text) => body.push(Block::Note(Lead::Bullet, text, Tone::Normal)),
                Html::Code(lines) => body.extend(lines.into_iter().enumerate().map(|(n, l)| {
                    let lead = match n {
                        0 => Lead::Icon(icon::COMMAND, Tone::Muted),
                        _ => Lead::None,
                    };
                    Block::Note(lead, l, Tone::Accent)
                })),
            }
        }
        body.push(Block::Blank);
        body.push(Block::Text(item.link.clone(), Tone::Muted));
        let mut detail = Detail::default();
        detail.set(body, Vec::new());
        // The title and the date stay while the text scrolls.
        detail.pin(3);
        ArticlePage { detail }
    }
}

impl Page for ArticlePage {
    fn title(&self) -> String {
        t("Article").into()
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        self.detail.draw(area, buf, ctx);
    }

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, String)> {
        self.detail.hints()
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        self.detail.key(key, ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_a_list_together_and_marks_the_date_a_command_and_the_link() {
        let body = ArticlePage::new(Item {
            title: "Title".into(),
            link: "https://archlinux.org/news/x/".into(),
            date: "2026-10-12".into(),
            html:
                "<p>Text.</p><ul><li>one</li><li>two</li></ul><pre><code>pacman -Syu</code></pre>"
                    .into(),
        })
        .detail
        .body;
        let note = |lead: Lead, text: &str, tone: Tone| Block::Note(lead, text.into(), tone);
        assert_eq!(body[1], Block::Text("2026-10-12".into(), Tone::Muted));
        let one = body
            .iter()
            .position(|b| *b == note(Lead::Bullet, "one", Tone::Normal))
            .unwrap();
        assert_eq!(body[one + 1], note(Lead::Bullet, "two", Tone::Normal));
        assert!(body.contains(&note(
            Lead::Icon(icon::COMMAND, Tone::Muted),
            "pacman -Syu",
            Tone::Accent
        )));
        assert_eq!(
            body.last(),
            Some(&Block::Text(
                "https://archlinux.org/news/x/".into(),
                Tone::Muted
            ))
        );
    }
}
