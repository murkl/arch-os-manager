//! The Arch Linux news, read here rather than in a browser.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::data::news::{Block as Html, Item, blocks};
use crate::i18n::t;
use crate::ui::detail::{Act, Block, Detail, loaded};
use crate::ui::icon;
use crate::ui::list::{Lead, List, Picked, Row};
use crate::ui::{Ctx, Flow, Key, Loaded, Page, Tone};

pub fn unread(ctx: &Ctx) -> usize {
    let read = ctx.state.news_read.as_deref().unwrap_or_default();
    ctx.shared.news.ready().map_or(0, |(items, _)| {
        items.iter().filter(|i| !read.contains(&i.link)).count()
    })
}

/// Remembers an item as read, or as new again. Only what the feed still lists is kept.
fn mark(ctx: &mut Ctx, link: &str, read: bool) {
    let Some((items, _)) = ctx.shared.news.ready() else {
        return;
    };
    let list = ctx.state.news_read.get_or_insert_with(Vec::new);
    if list.iter().any(|l| l == link) == read {
        return;
    }
    list.retain(|l| l != link && items.iter().any(|i| &i.link == l));
    if read {
        list.push(link.to_string());
    }
    ctx.state.save();
}

/// Whether an item was read.
fn is_read(ctx: &Ctx, link: &str) -> bool {
    ctx.state
        .news_read
        .as_deref()
        .unwrap_or_default()
        .iter()
        .any(|l| l == link)
}

pub struct NewsPage {
    list: List<String>,
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

    fn hints(&self, ctx: &Ctx) -> Vec<(&'static str, String)> {
        let mut h = super::list_hints(self.list.typing());
        if !self.list.typing()
            && let Some(link) = self.list.selected().and_then(|r| r.id.as_deref())
        {
            let label = if is_read(ctx, link) {
                t("unread")
            } else {
                t("read")
            };
            h.insert(2, ("space", label.into()));
        }
        h
    }

    fn key(&mut self, key: Key, ctx: &mut Ctx) -> Flow {
        if key == Key::Char(' ') && !self.list.typing() {
            if let Some(link) = self.list.selected().and_then(|r| r.id.clone()) {
                let read = is_read(ctx, &link);
                mark(ctx, &link, !read);
            }
            return Flow::Stay;
        }
        match self.list.key(key) {
            Picked::Open(link) => {
                let Some((items, _)) = ctx.shared.news.ready() else {
                    return Flow::Stay;
                };
                let Some(item) = items.iter().find(|i| i.link == link).cloned() else {
                    return Flow::Stay;
                };
                let read = is_read(ctx, &link);
                Flow::Push(Box::new(ArticlePage::new(item, read)))
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
    /// Reading changes nothing. The one action marks the item the other way and goes back.
    pub fn new(item: Item, read: bool) -> ArticlePage {
        let mut body = vec![
            Block::Heading(item.title.clone()),
            Block::Text(item.date.clone(), Tone::Muted),
        ];
        // The items of a list stand together. Everything else has a line around it.
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
        let link = item.link;
        let label = if read {
            t("Mark as unread")
        } else {
            t("Mark as read")
        };
        let acts = vec![
            Act::new(label, move |ctx: &mut Ctx| {
                mark(ctx, &link, !read);
                Flow::Pop
            })
            .icon(icon::NEWS),
        ];
        let mut detail = Detail::default();
        detail.set(body, acts);
        // The title and the date stay while the text scrolls.
        detail.pin_head();
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
    fn reading_changes_nothing_while_the_article_action_and_space_mark_an_item() {
        use crate::ui::app::tests::{app, example, open, press, screen};
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (mut app, mut term) = app();
        example(&mut app);
        open(&mut app, Box::new(NewsPage::new()));
        let first = app.ctx.shared.news.ready().unwrap().0[0].link.clone();
        assert!(screen(&mut app, &mut term).contains("space read"));
        press(&mut app, &[Key::Enter]);
        assert!(!is_read(&app.ctx, &first));
        assert!(screen(&mut app, &mut term).contains("Mark as read"));
        press(&mut app, &[Key::Esc]);
        assert!(!is_read(&app.ctx, &first));
        // The article's one action goes back.
        press(&mut app, &[Key::Enter, Key::Enter]);
        assert!(is_read(&app.ctx, &first));
        assert!(screen(&mut app, &mut term).contains("space unread"));
        press(&mut app, &[Key::Enter]);
        assert!(screen(&mut app, &mut term).contains("Mark as unread"));
        press(&mut app, &[Key::Enter]);
        assert!(!is_read(&app.ctx, &first));
        press(&mut app, &[Key::Char(' ')]);
        assert!(is_read(&app.ctx, &first));
        press(&mut app, &[Key::Char(' ')]);
        assert!(!is_read(&app.ctx, &first));
    }

    #[test]
    fn keeps_a_list_together_and_marks_the_date_a_command_and_the_link() {
        let body = ArticlePage::new(
            Item {
            title: "Title".into(),
            link: "https://archlinux.org/news/x/".into(),
            date: "2026-10-12".into(),
            html:
                "<p>Text.</p><ul><li>one</li><li>two</li></ul><pre><code>pacman -Syu</code></pre>"
                    .into(),
            },
            false,
        )
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
