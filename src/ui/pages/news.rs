//! The Arch Linux news, read here rather than in a browser.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::data::news::{Block as Html, Item, blocks};
use crate::i18n::t;
use crate::ui::detail::{Block, Detail, loaded};
use crate::ui::list::{List, Picked, Row};
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
                t("Offline: the news as last loaded."),
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
    item: Item,
    detail: Detail,
}

impl ArticlePage {
    pub fn new(item: Item) -> ArticlePage {
        let mut body = vec![
            Block::Heading(item.title.clone()),
            Block::Text(item.date.clone(), Tone::Muted),
            Block::Blank,
        ];
        for b in blocks(&item.html) {
            match b {
                Html::Para(text) => body.push(Block::Text(text, Tone::Normal)),
                Html::Item(text) => body.push(Block::Text(format!("- {text}"), Tone::Normal)),
                Html::Code(lines) => body.push(Block::Code(
                    lines
                        .into_iter()
                        .map(|l| (format!("  {l}"), Tone::Accent))
                        .collect(),
                )),
            }
            body.push(Block::Blank);
        }
        body.push(Block::Text(item.link.clone(), Tone::Muted));
        let mut detail = Detail::default();
        detail.set(body, Vec::new());
        ArticlePage { item, detail }
    }
}

impl Page for ArticlePage {
    fn title(&self) -> String {
        self.item.title.clone()
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
