//! What the machine uses, on one row: per resource an icon, its name, a bar as full as its share
//! and the share in percent.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::draw::{bar, put, width};
use super::{Ctx, Tone, icon};

/// The bar to the rest of its block, icon to value: 8 to 13, the golden ratio in whole cells.
const BAR: u16 = 8;
/// An icon and the space after it, as in the menu.
const ICON: u16 = 3;
/// The widest name.
const NAME: u16 = 4;
/// The widest value, `100%`.
const VALUE: u16 = 4;
/// Between blocks: twice the space after an icon, so a block reads as one.
const GAP: u16 = 4;

/// What a block shows, in cells.
#[derive(Debug)]
struct Block {
    icon: bool,
    name: bool,
    /// None where 0.
    bar: u16,
    /// What the value stands in, at its right.
    value: u16,
    /// From one block's start to the next.
    step: u16,
}

impl Block {
    fn width(&self) -> u16 {
        let bar = match self.bar {
            0 => 0,
            b => b + 1,
        };
        ICON * u16::from(self.icon) + (NAME + 1) * u16::from(self.name) + bar + self.value
    }
}

/// The fullest three blocks that fit `width` with gaps of at least `GAP`, the first at its left
/// edge and the last at its right. The bar says what the value says and goes first, then the name
/// where an icon says it. A cell an odd width leaves over comes off the bar, or without one goes
/// to the value, so both gaps stay equal.
fn layout(width: u16, icons: bool) -> Option<Block> {
    [(true, BAR), (true, 0), (false, 0)]
        .into_iter()
        .filter(|&(name, _)| name || icons)
        .find_map(|(name, bar)| {
            let mut block = Block {
                icon: icons,
                name,
                bar,
                value: VALUE,
                step: 0,
            };
            if width.checked_sub(3 * block.width())? % 2 == 1 {
                match bar {
                    0 => block.value += 1,
                    _ => block.bar -= 1,
                }
            }
            let gap = width.checked_sub(3 * block.width())? / 2;
            block.step = block.width() + gap;
            (gap >= GAP).then_some(block)
        })
}

/// What a block shows: how full, and that in percent.
struct Reading {
    share: Option<f64>,
    value: String,
}

impl Reading {
    /// Before the first reading.
    fn waiting(ctx: &Ctx) -> Reading {
        Reading {
            share: None,
            value: ctx.style.glyphs.ellipsis.into(),
        }
    }

    /// In whole percent, `?` where unknown.
    fn of(share: Option<f64>) -> Reading {
        let share = share.map(|s| s.clamp(0.0, 1.0));
        Reading {
            share,
            value: share.map_or("?".into(), |s| format!("{:.0}%", s * 100.0)),
        }
    }

    /// The state of its share, muted where unknown.
    fn tone(&self) -> Tone {
        self.share.map_or(Tone::Muted, tone)
    }
}

/// The state a share is in: success below 75 %, warning below 90 %, error from there.
fn tone(share: f64) -> Tone {
    match share {
        s if s < 0.75 => Tone::Ok,
        s if s < 0.90 => Tone::Warn,
        _ => Tone::Error,
    }
}

/// The usage on the top row of `area`: bar and value in the state of the share, the value bold,
/// icon and name muted. False where it does not fit.
pub fn draw(buf: &mut Buffer, area: Rect, ctx: &Ctx) -> bool {
    let s = &ctx.style;
    let Some(block) = layout(area.width, s.glyphs.icons) else {
        return false;
    };
    let readings = match ctx.usage {
        Some(u) => [u.cpu, u.memory, u.disk].map(Reading::of),
        None => std::array::from_fn(|_| Reading::waiting(ctx)),
    };
    let names = [
        (icon::CPU, "CPU"),
        (icon::MEMORY, "RAM"),
        (icon::DISK, "Disk"),
    ];
    for (i, ((glyph, name), reading)) in (0..).zip(names.iter().zip(&readings)) {
        let tone = reading.tone();
        let mut x = area.x + i * block.step;
        if block.icon {
            put(buf, x, area.y, glyph, 1, s.tone(Tone::Muted));
            x += ICON;
        }
        if block.name {
            put(buf, x, area.y, name, NAME, s.tone(Tone::Muted));
            x += NAME + 1;
        }
        if block.bar > 0 {
            let share = Some(reading.share.unwrap_or(0.0));
            bar(buf, x, area.y, block.bar, share, tone, ctx);
            x += block.bar + 1;
        }
        let used = width(&reading.value) as u16;
        let x = x + block.value.saturating_sub(used);
        put(buf, x, area.y, &reading.value, block.value, s.bold(tone));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reading_is_whole_percent_and_unknown_as_a_question_mark() {
        let value = |share| Reading::of(share).value;
        assert_eq!(value(Some(0.183)), "18%");
        assert_eq!(value(Some(1.0)), "100%");
        assert_eq!(value(Some(1.2)), "100%");
        assert_eq!(value(None), "?");
    }

    #[test]
    fn a_share_turns_from_success_to_warning_to_error() {
        assert_eq!(tone(0.0), Tone::Ok);
        assert_eq!(tone(0.749), Tone::Ok);
        assert_eq!(tone(0.75), Tone::Warn);
        assert_eq!(tone(0.9), Tone::Error);
        assert_eq!(tone(1.0), Tone::Error);
    }

    #[test]
    fn three_blocks_span_the_width_with_equal_gaps() {
        for icons in [true, false] {
            for w in 0..=120 {
                let Some(block) = layout(w, icons) else {
                    continue;
                };
                assert_eq!(2 * block.step + block.width(), w, "{w} {block:?}");
                assert!(block.step - block.width() >= GAP, "{w} {block:?}");
                assert!(
                    block.icon == icons && (block.name || icons),
                    "{w} {block:?}"
                );
            }
        }
        let bar = |w, icons| layout(w, icons).map(|b| (b.name, b.bar));
        assert_eq!(bar(71, true), Some((true, BAR)), "the golden ratio");
        assert_eq!(bar(72, true), Some((true, BAR - 1)), "a cell less");
        assert_eq!(bar(56, true), Some((true, 0)));
        assert_eq!(bar(36, true), Some((false, 0)), "40 columns");
        assert_eq!(bar(56, false), Some((true, 0)), "a name without an icon");
        assert_eq!(bar(20, true), None);
    }
}
