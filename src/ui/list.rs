//! The list every page selects from: headings, a cursor in the margin, a column for an icon
//! or a state, a filter, marks.

use std::collections::BTreeSet;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style as Cell;

use super::draw::{cut, put, width};
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
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: String,
    pub label: String,
    /// Right-aligned: a count, a state, a version.
    pub detail: String,
    pub tone: Tone,
    pub lead: Lead,
    /// Shown, but its action is not available here.
    pub dim: bool,
    pub heading: bool,
}

impl Row {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Row {
        Row {
            id: id.into(),
            label: label.into(),
            detail: String::new(),
            tone: Tone::Muted,
            lead: Lead::None,
            dim: false,
            heading: false,
        }
    }

    pub fn heading(label: impl Into<String>) -> Row {
        Row {
            heading: true,
            ..Row::new("", label)
        }
    }

    pub fn detail(mut self, detail: impl Into<String>, tone: Tone) -> Row {
        self.detail = detail.into();
        self.tone = tone;
        self
    }

    pub fn icon(mut self, icon: &'static str, tone: Tone) -> Row {
        self.lead = Lead::Icon(icon, tone);
        self
    }

    pub fn dot(mut self, tone: Tone) -> Row {
        self.lead = Lead::Dot(tone);
        self
    }

    pub fn dim(mut self, dim: bool) -> Row {
        self.dim = dim;
        self
    }
}

/// The columns of a row, as a list and a page of facts both draw it: lead, label, detail.
pub fn draw_row(
    buf: &mut Buffer,
    area: Rect,
    y: u16,
    lead: Lead,
    label: (&str, Cell),
    detail: &[(&str, Cell)],
    ctx: &Ctx,
) {
    let s = &ctx.style;
    let (glyph, tone) = match lead {
        Lead::None => ("", Tone::Muted),
        Lead::Icon(_, _) if !s.glyphs.icons => ("", Tone::Muted),
        Lead::Icon(icon, tone) => (icon, tone),
        Lead::Dot(Tone::Muted) => (s.glyphs.ring, Tone::Muted),
        Lead::Dot(tone) => (s.glyphs.dot, tone),
    };
    put(buf, area.x, y, glyph, 1, s.tone(tone));
    // Room for an icon a font draws wider than its cell, as Nerd Fonts without Mono do.
    let x = area.x + 3;
    let room = area.right().saturating_sub(x) as usize;
    // The detail cut at its end, across the pieces it is drawn in.
    let mut left = room / 2;
    let mut pieces = Vec::new();
    for (text, style) in detail {
        let piece = cut(text, left, s.glyphs.ellipsis);
        left -= width(&piece);
        let whole = piece == *text;
        pieces.push((piece, *style));
        if !whole {
            break;
        }
    }
    let detail_width = room / 2 - left;
    let label_room = room.saturating_sub(if detail_width == 0 {
        0
    } else {
        detail_width + 2
    });
    put(
        buf,
        x,
        y,
        &cut(label.0, label_room, s.glyphs.ellipsis),
        label_room as u16,
        label.1,
    );
    let mut x = area.right() - detail_width as u16;
    for (piece, style) in &pieces {
        x = put(buf, x, y, piece, width(piece) as u16, *style);
    }
}

/// Each `old → new` of a list padded to the widest, so the arrows stand in one column; a new
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

pub enum Picked {
    /// The list used the key.
    Used,
    Open(String),
    Ignored,
}

#[derive(Default)]
pub struct List {
    rows: Vec<Row>,
    cursor: usize,
    offset: usize,
    pub filter: Option<Input>,
    /// Marked rows by id, where the page allows marking.
    pub marked: BTreeSet<String>,
    height: usize,
}

impl List {
    pub fn new(rows: Vec<Row>) -> List {
        let mut l = List::default();
        l.set(rows);
        l
    }

    /// Replaces the rows. The cursor stays on its row, or where its row was when it is gone,
    /// so taking a row out does not send the cursor back to the top.
    pub fn set(&mut self, rows: Vec<Row>) {
        let id = self.selected().map(|r| r.id.clone());
        let at = self
            .shown()
            .iter()
            .position(|&i| i == self.cursor)
            .unwrap_or(0);
        self.rows = rows;
        self.marked.retain(|m| self.rows.iter().any(|r| &r.id == m));
        if id.as_deref().is_some_and(|id| self.focus(id)) {
            return;
        }
        let shown = self.shown();
        self.cursor = shown
            .get(at.min(shown.len().saturating_sub(1)))
            .copied()
            .unwrap_or(0);
    }

    pub fn focus(&mut self, id: &str) -> bool {
        match self.shown().into_iter().find(|&i| self.rows[i].id == id) {
            Some(i) => {
                self.cursor = i;
                true
            }
            None => false,
        }
    }

    pub fn rows(&self) -> &[Row] {
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

    pub fn selected(&self) -> Option<&Row> {
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

    pub fn key(&mut self, key: Key) -> Picked {
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
                    .map_or(Picked::Used, |r| Picked::Open(r.id.clone()));
            }
            Some(Nav::Filter) => self.open_filter(),
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
        if let Some(id) = self.selected().map(|r| r.id.clone())
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
        // Headings stay while a filter is empty; a filtered list is flat.
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
        // Each heading after the first gets a blank line before it; an empty one is only that.
        let mut lines: Vec<Option<usize>> = Vec::new();
        for &i in &visible {
            if self.rows[i].heading && !lines.is_empty() {
                lines.push(None);
            }
            if !(self.rows[i].heading && self.rows[i].label.is_empty()) {
                lines.push(Some(i));
            }
        }
        let at = lines
            .iter()
            .position(|l| *l == Some(self.cursor))
            .unwrap_or(0);
        if at < self.offset {
            self.offset = at.saturating_sub(usize::from(
                at > 0 && lines[at - 1].is_some_and(|i| self.rows[i].heading),
            ));
        } else if at >= self.offset + height {
            self.offset = at + 1 - height;
        }
        self.offset = self.offset.min(lines.len().saturating_sub(height.max(1)));
        for (n, line) in lines.iter().skip(self.offset).take(height).enumerate() {
            let Some(i) = *line else { continue };
            let row = &self.rows[i];
            let ly = y + n as u16;
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
            let lead = if self.marked.contains(&row.id) {
                Lead::Icon(s.glyphs.mark, Tone::Accent)
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
            draw_row(
                buf,
                area,
                ly,
                lead,
                (&row.label, style),
                &[(&row.detail, s.tone(row.tone))],
                ctx,
            );
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

    fn list() -> List {
        List::new(vec![
            Row::heading("A"),
            Row::new("1", "Updates"),
            Row::new("2", "Pakete"),
            Row::heading("B"),
            Row::new("3", "Dienste"),
        ])
    }

    #[test]
    fn skips_headings_and_stops_at_the_ends() {
        let mut l = list();
        assert_eq!(l.selected().unwrap().id, "1");
        l.key(Key::Down);
        l.key(Key::Char('j'));
        assert_eq!(l.selected().unwrap().id, "3");
        l.key(Key::Down);
        assert_eq!(l.selected().unwrap().id, "3");
        l.key(Key::Char('g'));
        assert_eq!(l.selected().unwrap().id, "1");
    }

    #[test]
    fn filters_while_letters_are_typed_and_esc_closes_the_filter() {
        let mut l = list();
        l.key(Key::Char('f'));
        assert!(l.typing());
        for c in "dien".chars() {
            assert!(matches!(l.key(Key::Char(c)), Picked::Used));
        }
        assert_eq!(l.selected().unwrap().id, "3");
        assert!(matches!(l.key(Key::Enter), Picked::Open(id) if id == "3"));
        l.key(Key::Esc);
        assert!(!l.typing());
        assert!(
            matches!(l.key(Key::Esc), Picked::Ignored),
            "without a filter esc is the page's"
        );
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
        assert_eq!(l.selected().unwrap().id, "2");
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
        assert_eq!(l.selected().unwrap().id, "3");
        l.key(Key::Char('G'));
        l.set(
            [0, 1, 3]
                .iter()
                .map(|i| Row::new(i.to_string(), format!("r{i}")))
                .collect(),
        );
        assert_eq!(l.selected().unwrap().id, "3");
    }
}
