//! The frame every page is drawn in, and the few primitives the pages share.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style as Cell;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::{Ctx, Tone};

pub fn width(s: &str) -> usize {
    s.width()
}

/// Cut to `max` columns at the end, marked.
pub fn cut(s: &str, max: usize, mark: &str) -> String {
    if s.width() <= max {
        return s.to_string();
    }
    let room = max.saturating_sub(mark.width());
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let w = c.width().unwrap_or(0);
        if used + w > room {
            break;
        }
        used += w;
        out.push(c);
    }
    out + if max >= mark.width() { mark } else { "" }
}

/// Cut at the front, where a path says least.
pub fn cut_front(s: &str, max: usize, mark: &str) -> String {
    if s.width() <= max {
        return s.to_string();
    }
    let room = max.saturating_sub(mark.width());
    let mut kept: Vec<char> = Vec::new();
    let mut used = 0;
    for c in s.chars().rev() {
        let w = c.width().unwrap_or(0);
        if used + w > room {
            break;
        }
        used += w;
        kept.push(c);
    }
    mark.to_string() + &kept.into_iter().rev().collect::<String>()
}

/// As many whole `pieces` as fit in `max` columns, joined by `sep`; those left out as `mark`.
pub fn fit(pieces: &[String], sep: &str, max: usize, mark: &str) -> String {
    let all = pieces.join(sep);
    if all.width() <= max {
        return all;
    }
    let mut kept = String::new();
    for piece in pieces {
        let next = match kept.is_empty() {
            true => piece.clone(),
            false => format!("{kept}{sep}{piece}"),
        };
        if next.width() + sep.width() + mark.width() > max {
            break;
        }
        kept = next;
    }
    match kept.is_empty() {
        true => cut(&all, max, mark),
        false => format!("{kept}{sep}{mark}"),
    }
}

/// Writes up to `max` columns and returns where it ended.
pub fn put(buf: &mut Buffer, x: u16, y: u16, s: &str, max: u16, style: Cell) -> u16 {
    if max == 0 || y >= buf.area.bottom() {
        return x;
    }
    buf.set_stringn(x, y, s, max as usize, style).0
}

/// Word wrap; a word wider than the line is broken after its last `/` that fits, else its last
/// `-`, as paths and links read.
pub fn wrap(s: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    for para in s.split('\n') {
        let mut line = String::new();
        for word in para.split_whitespace() {
            let mut word = word.to_string();
            while word.width() > width {
                let mut head: String = cut(&word, width, "");
                let at = head.rfind('/').or_else(|| head.rfind('-'));
                if let Some(at) = at.filter(|&at| at > 0) {
                    head.truncate(at + 1);
                }
                if head.is_empty() {
                    head = word.chars().next().map(String::from).unwrap_or_default();
                }
                word = word[head.len()..].to_string();
                if !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                }
                lines.push(head);
            }
            if line.is_empty() {
                line = word;
            } else if line.width() + 1 + word.width() <= width {
                line.push(' ');
                line.push_str(&word);
            } else {
                lines.push(std::mem::replace(&mut line, word));
            }
        }
        lines.push(line);
    }
    lines
}

/// Space around the content: more where there is room.
fn padding(width: u16) -> u16 {
    match width {
        0..40 => 1,
        40..70 => 2,
        _ => 4,
    }
}

const MAX_WIDTH: u16 = 100;

/// The column the page lives in: left edge, width, header row, footer row.
pub(super) fn geometry(area: Rect) -> Option<(u16, u16, u16, u16)> {
    let pad = padding(area.width);
    let width = area.width.saturating_sub(2 * pad).min(MAX_WIDTH);
    let x = area.x + (area.width - width) / 2;
    if area.height < 6 || width < 10 {
        return None;
    }
    let margin = u16::from(area.height > 16);
    Some((x, width, area.y + margin, area.bottom() - 1 - margin))
}

/// Paints the background and the header, and returns the area left for the page.
pub fn frame(area: Rect, buf: &mut Buffer, ctx: &Ctx, path: &[String]) -> Rect {
    let s = &ctx.style;
    buf.set_style(area, s.base());
    let Some((x, width, top, bottom)) = geometry(area) else {
        return area;
    };

    // Header: where we are on the left; on the right whether something loads, and the machine.
    let busy = if ctx.busy() { s.spinner() } else { "" };
    let mut host = ctx
        .shared
        .system
        .ready()
        .map(|s| s.hostname.clone())
        .unwrap_or_default();
    let sep = format!(" {} ", s.glyphs.path);
    let brand = if s.glyphs.icons {
        format!("{}  System Manager", super::icon::ARCH)
    } else {
        "System Manager".to_string()
    };
    let mut parts = vec![(brand, s.bold(Tone::Accent))];
    for (i, p) in path.iter().enumerate() {
        parts.push((sep.clone(), s.tone(Tone::Muted)));
        let last = i + 1 == path.len();
        parts.push((
            p.clone(),
            if last {
                s.bold(Tone::Normal)
            } else {
                s.tone(Tone::Normal)
            },
        ));
    }
    let full: usize = parts.iter().map(|(t, _)| t.width()).sum();
    let right = |host: &str| {
        [busy, host]
            .iter()
            .filter(|t| !t.is_empty())
            .map(|t| t.width() + 1)
            .sum::<usize>()
            + 2
    };
    // Short of room the machine's name goes first: it says less than where we are.
    if full + right(&host) > width as usize {
        host.clear();
    }
    let room = (width as usize).saturating_sub(right(&host));
    let fixed: usize = parts[..parts.len() - 1]
        .iter()
        .map(|(t, _)| t.width())
        .sum();
    if full > room && fixed + 4 <= room {
        // A long title, such as a news item's, is cut at its end: its start says what it is.
        let last = parts.len() - 1;
        parts[last].0 = cut(&parts[last].0, room - fixed, s.glyphs.ellipsis);
    } else if full > room {
        // Narrower still, whole parts go from the front, where the path says least.
        let mark = (format!("{}{sep}", s.glyphs.ellipsis), s.tone(Tone::Muted));
        let width_of = |p: &[(String, Cell)]| p.iter().map(|(t, _)| t.width()).sum::<usize>();
        let last = parts.len() - 1;
        let from = (2..last)
            .step_by(2)
            .find(|&i| mark.0.width() + width_of(&parts[i..]) <= room)
            .unwrap_or(last);
        let mut kept = vec![mark];
        kept.extend(parts.split_off(from));
        let fixed = width_of(&kept[..kept.len() - 1]);
        let end = kept.len() - 1;
        kept[end].0 = cut(&kept[end].0, room.saturating_sub(fixed), s.glyphs.ellipsis);
        parts = kept;
    }
    let mut cx = x;
    for (text, style) in &parts {
        cx = put(buf, cx, top, text, (x + width).saturating_sub(cx), *style);
    }
    let mut rx = x + width;
    if !host.is_empty() {
        rx -= host.width() as u16;
        put(
            buf,
            rx,
            top,
            &host,
            host.width() as u16,
            s.tone(Tone::Muted),
        );
        rx -= 1;
    }
    if !busy.is_empty() {
        rx -= busy.width() as u16;
        put(
            buf,
            rx,
            top,
            busy,
            busy.width() as u16,
            s.tone(Tone::Accent),
        );
    }
    put(
        buf,
        x,
        top + 1,
        &s.glyphs.rule.repeat(width as usize),
        width,
        s.tone(Tone::Muted),
    );

    let content_top = top + 3;
    let content_bottom = bottom.saturating_sub(1);
    Rect::new(
        x,
        content_top,
        width,
        content_bottom.saturating_sub(content_top),
    )
}

/// The keys of the page, dimmed under it, with the version on the right. Drawn after the page,
/// which knows its keys only once it has drawn.
pub fn footer(area: Rect, buf: &mut Buffer, ctx: &Ctx, hints: &[(&'static str, String)]) {
    let s = &ctx.style;
    let Some((x, width, _, bottom)) = geometry(area) else {
        return;
    };
    let version = crate::version::version();
    // The pages name their keys in the glyphs of the window; the console gets them as words.
    let keys: Vec<String> = hints
        .iter()
        .map(|(k, label)| {
            let key = match *k {
                "↑↓" => s.glyphs.up_down,
                "⏎" => s.glyphs.enter,
                "⇞⇟" => s.glyphs.page_keys,
                other => other,
            };
            format!("{key} {label}")
        })
        .collect();
    let sep = format!(" {} ", s.glyphs.separator);
    let line = keys.join(&sep);
    let room = (width as usize).saturating_sub(version.width() + 3);
    // Short of room the version goes first, then whole keys from the end.
    let shown = fit(&keys, &sep, width as usize, s.glyphs.ellipsis);
    put(buf, x, bottom, &shown, width, s.tone(Tone::Muted));
    if line.width() <= room {
        put(
            buf,
            x + width - version.width() as u16,
            bottom,
            version,
            version.width() as u16,
            s.tone(Tone::Muted),
        );
    }
}

/// A bar filled in `tone`; without a fraction a segment travels along it.
pub fn bar(buf: &mut Buffer, x: u16, y: u16, w: u16, fraction: Option<f64>, tone: Tone, ctx: &Ctx) {
    let s = &ctx.style;
    put(
        buf,
        x,
        y,
        &s.glyphs.track.repeat(w as usize),
        w,
        s.base().fg(s.surface),
    );
    let (from, len) = match fraction {
        Some(f) => (0, ((f.clamp(0.0, 1.0)) * w as f64).round() as u16),
        None => {
            let len = (w / 5).max(1);
            let span = u64::from(w + len);
            let pos = (super::beats(std::time::Instant::now(), super::BEAT) % span) as u16;
            (
                pos.saturating_sub(len),
                pos.min(w) - pos.saturating_sub(len),
            )
        }
    };
    put(
        buf,
        x + from,
        y,
        &s.glyphs.bar.repeat(len as usize),
        len,
        s.tone(tone),
    );
}

/// Where `area` stands in a column of `lines` from `offset`: a track in the right margin, as far
/// out as the cursor stands in the left, and a thumb as long and as far down as what is shown.
/// Nothing where everything shows.
pub fn scrollbar(buf: &mut Buffer, area: Rect, lines: usize, offset: usize, ctx: &Ctx) {
    let s = &ctx.style;
    let shown = usize::from(area.height);
    let Some(x) = [area.right() + 1, area.right()]
        .into_iter()
        .find(|&x| x < buf.area.right())
    else {
        return;
    };
    if lines <= shown || shown == 0 {
        return;
    }
    let thumb = (shown * shown / lines).max(1);
    let hidden = lines - shown;
    let top = ((shown - thumb) * offset.min(hidden) + hidden / 2) / hidden;
    for i in 0..shown {
        let (glyph, style) = match i >= top && i < top + thumb {
            true => (s.glyphs.scroll[1], s.tone(Tone::Muted)),
            false => (s.glyphs.scroll[0], s.base().fg(s.surface)),
        };
        put(buf, x, area.y + i as u16, glyph, 1, style);
    }
}

/// A bordered card in the middle of `area`, cleared; returns its inside.
pub fn card(area: Rect, buf: &mut Buffer, w: u16, h: u16, ctx: &Ctx) -> Rect {
    let s = &ctx.style;
    let w = w.min(area.width);
    let h = h.min(area.height);
    let x = area.x + (area.width - w) / 2;
    let y = area.y + (area.height - h) / 2;
    if w < 2 || h < 2 {
        return Rect::new(x, y, 0, 0);
    }
    let outer = Rect::new(x, y, w, h);
    buf.set_style(outer, s.base());
    for row in outer.rows() {
        put(buf, row.x, row.y, &" ".repeat(w as usize), w, s.base());
    }
    let [tl, tr, bl, br, hz, vt] = s.glyphs.border;
    let line = |l: &str, r: &str| format!("{l}{}{r}", hz.repeat(w.saturating_sub(2) as usize));
    put(buf, x, y, &line(tl, tr), w, s.tone(Tone::Muted));
    put(buf, x, y + h - 1, &line(bl, br), w, s.tone(Tone::Muted));
    for yy in y + 1..y + h - 1 {
        put(buf, x, yy, vt, 1, s.tone(Tone::Muted));
        put(buf, x + w - 1, yy, vt, 1, s.tone(Tone::Muted));
    }
    Rect::new(x + 3, y + 1, w.saturating_sub(6), h.saturating_sub(2))
}

/// Writes centred text, cut to the width.
pub fn center(buf: &mut Buffer, area: Rect, y: u16, text: &str, style: Cell, mark: &str) {
    let text = cut(text, area.width as usize, mark);
    let x = area.x + (area.width.saturating_sub(text.width() as u16)) / 2;
    put(buf, x, y, &text, area.width, style);
}

/// Something that takes a while, in the middle of the page: a spinner, the title, a step under
/// it and a bar, which runs back and forth while the share is unknown. Returns the row under
/// the bar.
pub fn waiting(
    buf: &mut Buffer,
    area: Rect,
    title: &str,
    step: Option<&str>,
    fraction: Option<f64>,
    ctx: &Ctx,
) -> u16 {
    let s = &ctx.style;
    let top = area.y + area.height.saturating_sub(8) / 2;
    center(buf, area, top, s.spinner(), s.bold(Tone::Accent), "");
    center(
        buf,
        area,
        top + 2,
        title,
        s.bold(Tone::Normal),
        s.glyphs.ellipsis,
    );
    if let Some(step) = step {
        center(
            buf,
            area,
            top + 3,
            step,
            s.tone(Tone::Muted),
            s.glyphs.ellipsis,
        );
    }
    let w = area.width.min(48);
    bar(
        buf,
        area.x + (area.width - w) / 2,
        top + 5,
        w,
        fraction,
        Tone::Accent,
        ctx,
    );
    top + 6
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cuts_at_the_side_that_says_less() {
        assert_eq!(cut("Aktualisieren", 8, "…"), "Aktuali…");
        assert_eq!(
            cut_front("System Manager › Pakete › firefox", 12, "…"),
            "…e › firefox"
        );
        assert_eq!(cut("kurz", 8, "…"), "kurz");
    }

    #[test]
    fn keeps_whole_pieces_and_marks_the_rest() {
        let keys = ["↑↓ choose", "⏎ open", "f search", "q quit"].map(String::from);
        assert_eq!(
            fit(&keys, " · ", 40, "…"),
            "↑↓ choose · ⏎ open · f search · q quit"
        );
        assert_eq!(fit(&keys, " · ", 30, "…"), "↑↓ choose · ⏎ open · …");
        assert_eq!(fit(&keys, " · ", 6, "…"), "↑↓ ch…");
    }

    #[test]
    fn a_narrow_header_and_footer_keep_whole_parts() {
        use crate::ui::app::tests::{app, open, screen};
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (mut app, _) = app();
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(40, 20)).unwrap();
        let home = screen(&mut app, &mut term);
        assert!(
            home.trim_end()
                .ends_with("↑↓ choose · ⏎ open · f search · …"),
            "{home}"
        );
        for page in crate::ui::Target::Language.open(&mut app.ctx) {
            open(&mut app, page);
        }
        let shown = screen(&mut app, &mut term);
        assert_eq!(
            shown.lines().nth(1),
            Some("  … › Settings › Language"),
            "{shown}"
        );
    }

    #[test]
    fn the_console_names_the_keys_in_words() {
        use crate::ui::app::tests::{app, screen};
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (mut app, mut term) = app();
        let colors = app.ctx.theme().colors;
        app.ctx.style = crate::ui::Style::new(&colors, true);
        let shown = screen(&mut app, &mut term);
        assert!(
            shown.contains("up/down choose | enter open | f search"),
            "{shown}"
        );
        assert!(shown.is_ascii(), "{shown}");
    }

    #[test]
    fn a_card_and_a_bar_fit_any_area() {
        let (app, _) = crate::ui::app::tests::app();
        for (w, h) in [(0, 0), (1, 1), (2, 2), (3, 1), (1, 3), (10, 4)] {
            let area = Rect::new(0, 0, w, h);
            let mut buf = Buffer::empty(area);
            let inner = card(area, &mut buf, 60, 8, &app.ctx);
            assert!(
                area.union(inner) == area || inner.is_empty(),
                "{w}x{h}: {inner:?}"
            );
            for fraction in [None, Some(0.5)] {
                bar(&mut buf, 0, 0, w, fraction, Tone::Accent, &app.ctx);
            }
        }
    }

    #[test]
    fn wraps_on_words_and_breaks_long_ones() {
        assert_eq!(wrap("eins zwei drei", 9), ["eins zwei", "drei"]);
        assert_eq!(wrap("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        assert_eq!(
            wrap("https://github.com/murkl/arch-os-manager", 31),
            ["https://github.com/murkl/", "arch-os-manager"]
        );
        assert_eq!(wrap("a\n\nb", 4), ["a", "", "b"]);
    }
}
