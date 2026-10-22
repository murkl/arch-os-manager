//! The list every page selects from: headings, a cursor in the margin, a column for an icon
//! or a state, a filter, marks.

use std::collections::BTreeSet;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style as Cell;

use super::draw::{cut, cut_front, put, width, wrap};
use super::input::Input;
use super::{Ctx, Key, Nav, Tone};
use crate::i18n::t;

/// What stands in the column before a label: every row has it, so labels line up on every page.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Lead {
    #[default]
    None,
    Icon(&'static str, Tone),
    /// A state: a filled dot in its tone, a ring when muted.
    Dot(Tone),
    /// An item of running text.
    Bullet,
    /// A marked row.
    Mark,
}

impl Lead {
    /// Whether anything is drawn for it: an icon only where the font has icons.
    pub fn shows(self, s: &super::Style) -> bool {
        match self {
            Lead::None => false,
            Lead::Icon(..) => s.glyphs.icons,
            Lead::Dot(_) | Lead::Bullet | Lead::Mark => true,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row<I> {
    /// What choosing the row means to its page. A heading means nothing.
    pub id: Option<I>,
    pub label: String,
    /// Right-aligned: a count, a state, a version.
    pub detail: String,
    pub tone: Tone,
    pub lead: Lead,
    /// Shown, but its action is not available here.
    pub dim: bool,
    pub heading: bool,
    /// Lines under the label, wrapped where it starts. The cursor steps over them with the row.
    pub note: String,
}

impl<I> Row<I> {
    pub fn new(id: I, label: impl Into<String>) -> Row<I> {
        Row {
            id: Some(id),
            ..Row::blank(label)
        }
    }

    pub fn heading(label: impl Into<String>) -> Row<I> {
        Row {
            heading: true,
            ..Row::blank(label)
        }
    }

    fn blank(label: impl Into<String>) -> Row<I> {
        Row {
            id: None,
            label: label.into(),
            detail: String::new(),
            tone: Tone::Muted,
            lead: Lead::None,
            dim: false,
            heading: false,
            note: String::new(),
        }
    }

    pub fn detail(mut self, detail: impl Into<String>, tone: Tone) -> Row<I> {
        self.detail = detail.into();
        self.tone = tone;
        self
    }

    pub fn icon(mut self, icon: &'static str, tone: Tone) -> Row<I> {
        self.lead = Lead::Icon(icon, tone);
        self
    }

    pub fn dot(mut self, tone: Tone) -> Row<I> {
        self.lead = Lead::Dot(tone);
        self
    }

    pub fn dim(mut self, dim: bool) -> Row<I> {
        self.dim = dim;
        self
    }

    pub fn note(mut self, note: impl Into<String>) -> Row<I> {
        self.note = note.into();
        self
    }
}

/// A line a list draws: a row, a line of the note under it, or the space before a heading.
enum Line {
    Row(usize),
    Note(String),
    Blank,
}

/// The columns of a row, as a list and a page of facts both draw it: the lead where the page
/// has a column for it, the label and the detail, each in pieces of their own style. The detail
/// gives way at its end, or at its front where it is a change `old → new`.
pub fn draw_row(
    buf: &mut Buffer,
    area: Rect,
    y: u16,
    (lead, column): (Lead, bool),
    label: &[(&str, Cell)],
    (detail, front): (&[(&str, Cell)], bool),
    ctx: &Ctx,
) {
    let s = &ctx.style;
    let (glyph, tone) = match lead {
        Lead::None => ("", Tone::Muted),
        Lead::Icon(_, _) if !s.glyphs.icons => ("", Tone::Muted),
        Lead::Icon(icon, tone) => (icon, tone),
        Lead::Dot(Tone::Muted) => (s.glyphs.ring, Tone::Muted),
        Lead::Dot(tone) => (s.glyphs.dot, tone),
        Lead::Bullet => (s.glyphs.bullet, Tone::Accent),
        Lead::Mark => (s.glyphs.mark, Tone::Accent),
    };
    put(buf, area.x, y, glyph, 1, s.tone(tone));
    // Room for an icon a font draws wider than its cell, as Nerd Fonts without Mono do.
    let x = area.x + if column { 3 } else { 0 };
    let room = area.right().saturating_sub(x) as usize;
    // Pieces cut to a width across the pieces, from the first at their end or from the last at
    // their front. The detail gets what the label leaves, at least half the row.
    let fit = |pieces: &[(&str, Cell)], mut left: usize, front: bool| {
        let mut pieces = pieces.to_vec();
        if front {
            pieces.reverse();
        }
        let mut out = Vec::new();
        for (text, style) in pieces {
            let piece = match front {
                false => cut(text, left, s.glyphs.ellipsis),
                true if width(text) <= left => text.to_string(),
                // Padding gives way first, without a mark.
                true => cut_front(text.trim_start(), left, s.glyphs.ellipsis),
            };
            left = left.saturating_sub(width(&piece));
            let whole = piece == text;
            out.push((piece, style));
            if !whole {
                break;
            }
        }
        if front {
            out.reverse();
        }
        (out, left)
    };
    let label_width: usize = label.iter().map(|(t, _)| width(t)).sum();
    let budget = (room / 2).max(room.saturating_sub(label_width + 2));
    let (pieces, left) = fit(detail, budget, front);
    let detail_width = budget - left;
    let label_room = room.saturating_sub(if detail_width == 0 {
        0
    } else {
        detail_width + 2
    });
    let (name, _) = fit(label, label_room, false);
    let mut lx = x;
    for (piece, style) in &name {
        lx = put(
            buf,
            lx,
            y,
            piece,
            (x + label_room as u16).saturating_sub(lx),
            *style,
        );
    }
    let mut x = area.right() - detail_width as u16;
    for (piece, style) in &pieces {
        x = put(buf, x, y, piece, width(piece) as u16, *style);
    }
}

/// Each `old → new` of a list padded to the widest, so the arrows stand in one column. A new
/// value without an old one stands under the other new ones. Each comes as the part up to the
/// new value, and the new value.
pub fn changes(pairs: &[(&str, &str)], arrow: &str) -> Vec<(String, String)> {
    let old = pairs.iter().map(|(o, _)| width(o)).max().unwrap_or(0);
    let new = pairs.iter().map(|(_, n)| width(n)).max().unwrap_or(0);
    let pad = |s: &str, to: usize| " ".repeat(to - width(s));
    pairs
        .iter()
        .map(|(o, n)| {
            let head = match (old, o.is_empty()) {
                (0, _) => String::new(),
                (_, true) => pad("", old + width(arrow) + 4),
                (_, false) => format!("{}{o}  {arrow}  ", pad(o, old)),
            };
            (head, format!("{n}{}", pad(n, new)))
        })
        .collect()
}

pub enum Picked<I> {
    /// The list used the key.
    Used,
    Open(I),
    Ignored,
}

pub struct List<I> {
    rows: Vec<Row<I>>,
    cursor: usize,
    offset: usize,
    pub filter: Option<Input>,
    /// Marked rows, where the page allows marking.
    pub marked: BTreeSet<I>,
    /// The page marks rows, so the column for the mark stays, marked or not.
    pub markable: bool,
    /// The page names f in its keys. Elsewhere, as under a question, f is no filter.
    filterable: bool,
    height: usize,
}

impl<I> Default for List<I> {
    fn default() -> Self {
        List {
            rows: Vec::new(),
            cursor: 0,
            offset: 0,
            filter: None,
            marked: BTreeSet::new(),
            markable: false,
            filterable: false,
            height: 0,
        }
    }
}

impl<I: Clone + Ord> List<I> {
    pub fn new(rows: Vec<Row<I>>) -> List<I> {
        let mut l = List::default();
        l.set(rows);
        l
    }

    /// A list of a page that offers its filter on f.
    pub fn filterable() -> List<I> {
        List {
            filterable: true,
            ..List::default()
        }
    }

    /// Replaces the rows. The cursor stays on its row, or where its row was when it is gone,
    /// so taking a row out does not send the cursor back to the top.
    pub fn set(&mut self, rows: Vec<Row<I>>) {
        let id = self.selected().and_then(|r| r.id.clone());
        let at = self
            .shown()
            .iter()
            .position(|&i| i == self.cursor)
            .unwrap_or(0);
        self.rows = rows;
        self.marked
            .retain(|m| self.rows.iter().any(|r| r.id.as_ref() == Some(m)));
        if id.as_ref().is_some_and(|id| self.focus(id)) {
            return;
        }
        let shown = self.shown();
        self.cursor = shown
            .get(at.min(shown.len().saturating_sub(1)))
            .copied()
            .unwrap_or(0);
    }

    pub fn focus(&mut self, id: &I) -> bool {
        match self
            .shown()
            .into_iter()
            .find(|&i| self.rows[i].id.as_ref() == Some(id))
        {
            Some(i) => {
                self.cursor = i;
                true
            }
            None => false,
        }
    }

    pub fn rows(&self) -> &[Row<I>] {
        &self.rows
    }

    /// Indices of the selectable rows the filter lets through.
    fn shown(&self) -> Vec<usize> {
        let query = self
            .filter
            .as_ref()
            .map(|f| f.text.to_lowercase())
            .unwrap_or_default();
        (0..self.rows.len())
            .filter(|&i| {
                let r = &self.rows[i];
                !r.heading
                    && (query.is_empty()
                        || r.label.to_lowercase().contains(&query)
                        || r.detail.to_lowercase().contains(&query))
            })
            .collect()
    }

    pub fn selected(&self) -> Option<&Row<I>> {
        self.shown()
            .contains(&self.cursor)
            .then(|| &self.rows[self.cursor])
    }

    pub fn typing(&self) -> bool {
        self.filter.is_some()
    }

    pub fn open_filter(&mut self) {
        if self.filter.is_none() {
            self.filter = Some(Input::new(t("Filter")));
        }
    }

    fn step(&mut self, by: isize) {
        let shown = self.shown();
        if shown.is_empty() {
            return;
        }
        let at = shown.iter().position(|&i| i == self.cursor).unwrap_or(0) as isize;
        let next = (at + by).clamp(0, shown.len() as isize - 1);
        self.cursor = shown[next as usize];
    }

    pub fn key(&mut self, key: Key) -> Picked<I> {
        let typing = self.filter.is_some();
        match key.nav(typing) {
            Some(Nav::Up) => self.step(-1),
            Some(Nav::Down) => self.step(1),
            Some(Nav::PageUp) => self.step(-(self.height.max(2) as isize - 1)),
            Some(Nav::PageDown) => self.step(self.height.max(2) as isize - 1),
            Some(Nav::Top) => self.step(isize::MIN / 2),
            Some(Nav::Bottom) => self.step(isize::MAX / 2),
            Some(Nav::Open) => {
                return self
                    .selected()
                    .and_then(|r| r.id.clone())
                    .map_or(Picked::Used, Picked::Open);
            }
            Some(Nav::Filter) if self.filterable => self.open_filter(),
            Some(Nav::Back) if typing => self.filter = None,
            _ => {
                let Some(filter) = self.filter.as_mut() else {
                    return Picked::Ignored;
                };
                if key == Key::Backspace && filter.text.is_empty() {
                    self.filter = None;
                } else if !filter.key(key) {
                    return Picked::Ignored;
                }
                let shown = self.shown();
                if !shown.contains(&self.cursor) {
                    self.cursor = shown.first().copied().unwrap_or(self.cursor);
                }
            }
        }
        Picked::Used
    }

    /// Marks or unmarks the row under the cursor.
    pub fn toggle_mark(&mut self) {
        if let Some(id) = self.selected().and_then(|r| r.id.clone())
            && !self.marked.remove(&id)
        {
            self.marked.insert(id);
        }
    }

    pub fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &Ctx) {
        let s = &ctx.style;
        let mut y = area.y;
        if let Some(filter) = &self.filter {
            filter.draw(buf, area.x, y, area.width, ctx);
            y += 2;
        }
        let height = area.bottom().saturating_sub(y) as usize;
        self.height = height;
        let shown = self.shown();
        let query_active = self.filter.as_ref().is_some_and(|f| !f.text.is_empty());
        // Headings stay while a filter is empty. A filtered list is flat.
        let visible: Vec<usize> = (0..self.rows.len())
            .filter(|&i| shown.contains(&i) || (self.rows[i].heading && !query_active))
            .collect();
        if shown.is_empty() {
            if query_active {
                put(
                    buf,
                    area.x,
                    y,
                    t("No matches"),
                    area.width,
                    s.tone(Tone::Muted),
                );
            }
            return;
        }
        // A column for the leads only where one shows. The console has no icons.
        let column = self.markable || self.rows.iter().any(|r| r.lead.shows(s));
        let indent = if column { 3 } else { 0 };
        // Each heading after the first gets a blank line before it. An empty one is only that.
        let mut lines: Vec<Line> = Vec::new();
        for &i in &visible {
            let row = &self.rows[i];
            if row.heading && !lines.is_empty() {
                lines.push(Line::Blank);
            }
            if !(row.heading && row.label.is_empty()) {
                lines.push(Line::Row(i));
            }
            if !row.note.is_empty() {
                let room = area.width.saturating_sub(indent) as usize;
                lines.extend(wrap(&row.note, room).into_iter().map(Line::Note));
            }
        }
        let at = lines
            .iter()
            .position(|l| matches!(l, Line::Row(i) if *i == self.cursor))
            .unwrap_or(0);
        // The row under the cursor comes into sight with its note, as far as there is room.
        let notes = lines[at + 1..]
            .iter()
            .take_while(|l| matches!(l, Line::Note(_)))
            .count();
        if at < self.offset {
            self.offset = at.saturating_sub(usize::from(
                at > 0 && matches!(lines[at - 1], Line::Row(i) if self.rows[i].heading),
            ));
        } else if at + 1 + notes > self.offset + height {
            self.offset = (at + 1 + notes - height).min(at);
        }
        self.offset = self.offset.min(lines.len().saturating_sub(height.max(1)));
        let rows = Rect::new(area.x, y, area.width, height as u16);
        super::draw::scrollbar(buf, rows, lines.len(), self.offset, ctx);
        for (n, line) in lines.iter().skip(self.offset).take(height).enumerate() {
            let ly = y + n as u16;
            let i = match line {
                Line::Row(i) => *i,
                Line::Note(text) => {
                    let room = area.width.saturating_sub(indent);
                    put(buf, area.x + indent, ly, text, room, s.tone(Tone::Muted));
                    continue;
                }
                Line::Blank => continue,
            };
            let row = &self.rows[i];
            if row.heading {
                put(
                    buf,
                    area.x,
                    ly,
                    &cut(&row.label, area.width as usize, s.glyphs.ellipsis),
                    area.width,
                    s.bold(Tone::Muted),
                );
                continue;
            }
            let current = i == self.cursor;
            if current && area.x >= 2 {
                put(
                    buf,
                    area.x - 2,
                    ly,
                    s.glyphs.cursor,
                    1,
                    s.tone(Tone::Accent),
                );
            }
            let lead = if row.id.as_ref().is_some_and(|id| self.marked.contains(id)) {
                Lead::Mark
            } else {
                match row.lead {
                    Lead::Icon(icon, _) if current => Lead::Icon(icon, Tone::Accent),
                    Lead::Icon(icon, _) if row.dim => Lead::Icon(icon, Tone::Muted),
                    other => other,
                }
            };
            let style = match (current, row.dim) {
                (true, _) => s.bold(Tone::Accent),
                (false, true) => s.tone(Tone::Muted),
                (false, false) => s.tone(Tone::Normal),
            };
            // Muted reads badly on the selection. The row under the cursor is the one being read.
            let detail_tone = match (current, row.tone) {
                (true, Tone::Muted) => Tone::Normal,
                (_, tone) => tone,
            };
            draw_row(
                buf,
                area,
                ly,
                (lead, column),
                &[(&row.label, style)],
                (&[(&row.detail, s.tone(detail_tone))], false),
                ctx,
            );
            if current {
                // The whole row on the selection, from the cursor in the margin to the right
                // edge, so the eye finds its value.
                let from = area.x.saturating_sub(2);
                buf.set_style(
                    Rect::new(from, ly, area.right() - from, 1),
                    Cell::new().bg(s.selection),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_put_every_arrow_in_one_column() {
        let all: Vec<String> = changes(
            &[
                ("157.0.1-1", "158.0-1"),
                ("2.2.2-1", "2.2.3-1"),
                ("", "1.4.0"),
            ],
            "→",
        )
        .into_iter()
        .map(|(head, new)| head + &new)
        .collect();
        let column = |s: &str| s.chars().take_while(|&c| c != '→').count();
        assert_eq!(column(&all[0]), column(&all[1]));
        assert!(all.iter().all(|c| width(c) == width(&all[0])));
        let at = |s: &str, v: &str| width(&s[..s.find(v).unwrap()]);
        assert_eq!(at(&all[2], "1.4.0"), at(&all[1], "2.2.3-1"));
        assert!(!all[2].contains('→'));
        assert_eq!(
            changes(&[("1-1", "2-1")], "->"),
            [("1-1  ->  ".to_string(), "2-1".to_string())]
        );
        assert_eq!(
            changes(&[("", "2-1"), ("", "10-1")], "->"),
            [
                (String::new(), "2-1 ".to_string()),
                (String::new(), "10-1".to_string())
            ]
        );
    }

    fn list() -> List<&'static str> {
        let mut l = List::filterable();
        l.set(vec![
            Row::heading("A"),
            Row::new("1", "Updates"),
            Row::new("2", "Pakete"),
            Row::heading("B"),
            Row::new("3", "Dienste"),
        ]);
        l
    }

    #[test]
    fn f_opens_no_filter_where_the_page_offers_none() {
        let mut l = List::new(vec![Row::new(false, "No"), Row::new(true, "Yes")]);
        assert!(matches!(l.key(Key::Char('f')), Picked::Ignored));
        assert!(matches!(l.key(Key::Char('/')), Picked::Ignored));
        assert!(!l.typing());
    }

    #[test]
    fn a_list_longer_than_its_room_has_a_thumb_where_the_cursor_is() {
        let (app, _) = crate::ui::app::tests::app();
        let mut l = List::new((0..30).map(|i| Row::new(i.to_string(), "row")).collect());
        let thumb = |l: &mut List<String>| {
            let mut buf = Buffer::empty(Rect::new(0, 0, 14, 10));
            l.draw(Rect::new(2, 0, 10, 10), &mut buf, &app.ctx);
            (0..10)
                .filter(|&y| buf[(13, y)].symbol() == app.ctx.style.glyphs.scroll[1])
                .collect::<Vec<u16>>()
        };
        assert_eq!(thumb(&mut l), [0, 1, 2]);
        l.key(Key::Char('G'));
        assert_eq!(thumb(&mut l), [7, 8, 9]);
        let mut short = List::new(vec![Row::new("1".to_string(), "row")]);
        assert_eq!(thumb(&mut short), Vec::<u16>::new());
    }

    #[test]
    fn a_detail_takes_the_room_a_short_label_leaves_and_half_beside_a_long_one() {
        let (app, _) = crate::ui::app::tests::app();
        let row = |label: &str, front: bool| {
            let area = Rect::new(0, 0, 43, 1);
            let mut buf = Buffer::empty(area);
            let style = app.ctx.style.base();
            let detail = [("    1.0.10-1  →  ", style), ("1.0.11-1", style)];
            draw_row(
                &mut buf,
                area,
                0,
                (Lead::None, true),
                &[(label, style)],
                (&detail, front),
                &app.ctx,
            );
            (0..43).map(|x| buf[(x, 0)].symbol()).collect::<String>()
        };
        assert!(row("package-10", false).ends_with("    1.0.10-1  →  1.0.11-1"));
        let long = "a-package-with-a-really-long-name";
        let end = row(long, false);
        assert!(
            end.contains("a-package-with-a-…  ") && end.ends_with("→  1.…"),
            "{end}"
        );
        let front = row(long, true);
        assert!(
            front.contains("a-package-with-a-…  ") && front.ends_with("…0.10-1  →  1.0.11-1"),
            "{front}"
        );
        // Padding is not content.
        assert!(row("package-100000", true).ends_with("  1.0.10-1  →  1.0.11-1"));
    }

    #[test]
    fn skips_headings_and_stops_at_the_ends() {
        let mut l = list();
        assert_eq!(l.selected().unwrap().id, Some("1"));
        l.key(Key::Down);
        l.key(Key::Char('j'));
        assert_eq!(l.selected().unwrap().id, Some("3"));
        l.key(Key::Down);
        assert_eq!(l.selected().unwrap().id, Some("3"));
        l.key(Key::Char('g'));
        assert_eq!(l.selected().unwrap().id, Some("1"));
    }

    #[test]
    fn filters_while_letters_are_typed_and_esc_closes_the_filter() {
        let mut l = list();
        l.key(Key::Char('f'));
        assert!(l.typing());
        for c in "dien".chars() {
            assert!(matches!(l.key(Key::Char(c)), Picked::Used));
        }
        assert_eq!(l.selected().unwrap().id, Some("3"));
        assert!(matches!(l.key(Key::Enter), Picked::Open(id) if id == "3"));
        l.key(Key::Esc);
        assert!(!l.typing());
        assert!(
            matches!(l.key(Key::Esc), Picked::Ignored),
            "without a filter esc is the page's"
        );
    }

    #[test]
    fn the_row_under_the_cursor_lies_on_the_selection_with_its_value_readable() {
        let (app, _) = crate::ui::app::tests::app();
        let s = &app.ctx.style;
        let mut l = List::new(vec![
            Row::new("a", "Packages").detail("1284", Tone::Muted),
            Row::new("b", "Services").detail("2", Tone::Muted),
        ]);
        let area = Rect::new(2, 0, 30, 2);
        let mut buf = Buffer::empty(Rect::new(0, 0, 32, 2));
        l.draw(area, &mut buf, &app.ctx);
        assert!(
            (0..32).all(|x| buf[(x, 0)].bg == s.selection),
            "the whole row, cursor included"
        );
        assert!((0..32).all(|x| buf[(x, 1)].bg != s.selection));
        assert_eq!(
            buf[(31, 0)].fg,
            s.text,
            "muted turns to text on the selection"
        );
        assert_eq!(buf[(31, 1)].fg, s.muted);
    }

    #[test]
    fn a_note_stands_under_its_row_where_the_label_starts_and_the_cursor_steps_over_it() {
        let (app, _) = crate::ui::app::tests::app();
        let line = |y: u16, buf: &Buffer| -> String {
            let text: String = (0..14).map(|x| buf[(x, y)].symbol()).collect();
            text.trim_end().to_string()
        };
        let mut l = List::new(vec![
            Row::new("a", "Repair")
                .icon("x", Tone::Normal)
                .note("one two three"),
            Row::heading(""),
            Row::new("b", "Check").icon("x", Tone::Normal).note("four"),
        ]);
        let area = Rect::new(2, 0, 12, 4);
        let mut buf = Buffer::empty(Rect::new(0, 0, 14, 4));
        l.draw(area, &mut buf, &app.ctx);
        assert_eq!(line(1, &buf), "     one two");
        assert_eq!(line(2, &buf), "     three");
        l.key(Key::Down);
        assert_eq!(l.selected().unwrap().id, Some("b"));
        let mut buf = Buffer::empty(Rect::new(0, 0, 14, 4));
        l.draw(area, &mut buf, &app.ctx);
        assert_eq!(line(2, &buf), "▶ x  Check", "the cursor skips the note");
        assert_eq!(
            line(3, &buf),
            "     four",
            "and its own note stays in sight"
        );
    }

    #[test]
    fn without_icons_the_labels_start_at_the_edge_unless_a_mark_or_dot_needs_the_column() {
        let (mut app, _) = crate::ui::app::tests::app();
        let colors = app.ctx.theme().colors;
        app.ctx.style = crate::ui::Style::new(&colors, true);
        let line = |y: u16, buf: &Buffer| -> String {
            (0..20).map(|x| buf[(x, y)].symbol()).collect::<String>()
        };
        let area = Rect::new(2, 0, 18, 2);
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 2));
        let mut l = List::new(vec![
            Row::new("a", "paru").icon("x", Tone::Normal),
            Row::new("b", "yay").icon("x", Tone::Normal),
        ]);
        l.draw(area, &mut buf, &app.ctx);
        assert_eq!(line(0, &buf), "> paru              ");
        l.markable = true;
        l.toggle_mark();
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 2));
        l.draw(area, &mut buf, &app.ctx);
        assert_eq!(line(0, &buf), "> +  paru           ");
        assert_eq!(line(1, &buf), "     yay            ");
        let mut dots = List::new(vec![Row::new("a", "sshd").dot(Tone::Ok)]);
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 2));
        dots.draw(area, &mut buf, &app.ctx);
        assert_eq!(line(0, &buf), "> *  sshd           ");
    }

    #[test]
    fn backspace_deletes_and_only_then_closes_the_filter() {
        let mut l = list();
        l.key(Key::Char('/'));
        l.key(Key::Char('x'));
        l.key(Key::Backspace);
        assert!(l.typing());
        l.key(Key::Backspace);
        assert!(!l.typing());
    }

    #[test]
    fn keeps_the_cursor_on_its_row_when_rows_change() {
        let mut l = list();
        l.key(Key::Down);
        l.set(vec![Row::new("0", "Neu"), Row::new("2", "Pakete")]);
        assert_eq!(l.selected().unwrap().id, Some("2"));
    }

    #[test]
    fn a_row_taken_out_leaves_the_cursor_where_it_was() {
        let mut l = List::new(
            (0..5)
                .map(|i| Row::new(i.to_string(), format!("r{i}")))
                .collect(),
        );
        l.key(Key::Down);
        l.key(Key::Down);
        l.set(
            [0, 1, 3, 4]
                .iter()
                .map(|i| Row::new(i.to_string(), format!("r{i}")))
                .collect(),
        );
        assert_eq!(l.selected().unwrap().id.as_deref(), Some("3"));
        l.key(Key::Char('G'));
        l.set(
            [0, 1, 3]
                .iter()
                .map(|i| Row::new(i.to_string(), format!("r{i}")))
                .collect(),
        );
        assert_eq!(l.selected().unwrap().id.as_deref(), Some("3"));
    }
}
