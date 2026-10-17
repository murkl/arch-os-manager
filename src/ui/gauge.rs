//! An instrument: a ring of dots as full as a share, the share in percent inside it.

use std::f64::consts::TAU;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::draw::{cut, put, width};
use super::{Ctx, Tone};

pub const HEIGHT: u16 = 6;

/// A tenth each. Not a multiple of four, so no dot falls on the circle's middle line, which runs
/// between the two rows of text.
const DOTS: usize = 10;
const _: () = assert!(DOTS % 4 == 2);

/// The cell aspects a test holds the ring round and even for.
const ASPECTS: (f64, f64) = (1.8, 2.8);

/// The braille point of a column and a row in its cell, as U+2800 counts them.
const BRAILLE: [[u8; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];

/// A braille point, two across and four down a cell: its column and its row.
type Point = (i32, i32);

/// The ring for cells `aspect` times as high as wide, on braille points: the dots evenly round a
/// circle, clockwise from the top, each on the point nearest to it. Its axis is a column of
/// points, the only place a single dot at the top can stand, and its middle the line between
/// the rows of text.
pub struct Ring {
    points: [Point; DOTS],
    pub width: u16,
}

impl Ring {
    pub fn new(aspect: f32) -> Ring {
        let a = f64::from(aspect).clamp(ASPECTS.0, ASPECTS.1);
        let middle = f64::from(HEIGHT) * a / 2.0;
        // The top dot in the first row of points.
        let radius = middle - a / 8.0;
        let half = radius.ceil() as i32;
        let axis = 2 * half;
        // A quarter, mirrored into the others, so the ring stays symmetric whatever rounds.
        let quarter: [Point; DOTS / 4 + 1] = std::array::from_fn(|m| {
            let t = TAU * m as f64 / DOTS as f64;
            let x = (f64::from(axis) + 0.5) / 2.0 + radius * t.sin();
            let y = middle - radius * t.cos();
            (
                (x * 2.0 - 0.5).round() as i32,
                (y * 4.0 / a - 0.5).round() as i32,
            )
        });
        let rows = 4 * i32::from(HEIGHT);
        let points = std::array::from_fn(|k| {
            let right = k <= DOTS / 2;
            let k = if right { k } else { DOTS - k };
            let (j, i) = match k <= DOTS / 4 {
                true => quarter[k],
                false => {
                    let (j, i) = quarter[DOTS / 2 - k];
                    (j, rows - 1 - i)
                }
            };
            (if right { j } else { 2 * axis - j }, i)
        });
        Ring {
            points,
            width: (2 * half + 1) as u16,
        }
    }

    /// Where the axis stands, in cells from the left edge.
    fn axis(&self) -> f64 {
        f64::from(self.width / 2) + 0.25
    }

    /// The cell of a dot, and its point among the cell's braille points.
    fn cell(&self, k: usize) -> (u16, u16, u8) {
        let (j, i) = self.points[k];
        let bit = BRAILLE[(j % 2) as usize][(i % 4) as usize];
        ((j / 2) as u16, (i / 4) as u16, bit)
    }

    /// The columns free on a row between its dots, a cell clear of them on each side.
    fn inside(&self, row: u16) -> u16 {
        let cols = || {
            (0..DOTS)
                .map(|k| self.cell(k))
                .filter(|c| c.1 == row)
                .map(|c| c.0)
        };
        match (cols().min(), cols().max()) {
            (Some(left), Some(right)) => right.saturating_sub(left + 3),
            _ => self.width,
        }
    }
}

/// What an instrument shows: how full, and that in percent.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    share: Option<f64>,
    value: String,
}

impl Reading {
    /// Before the first reading.
    pub fn waiting(ctx: &Ctx) -> Reading {
        Reading {
            share: None,
            value: ctx.style.glyphs.ellipsis.into(),
        }
    }

    /// In whole percent, without a space, so two digits stand exactly in the middle; `?` where
    /// unknown.
    pub fn of(share: Option<f64>) -> Reading {
        let share = share.map(|s| s.clamp(0.0, 1.0));
        Reading {
            share,
            value: share.map_or("?".into(), |s| format!("{:.0}%", s * 100.0)),
        }
    }

    /// The state of its share; muted where unknown.
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

/// The ring at the top of `cell`, in its middle: the dots as full as the share lit in its state
/// and bold, which the window draws larger, the rest muted, which every theme sets apart from its
/// background; the percent and the name under it in its middle.
pub fn ring(buf: &mut Buffer, cell: Rect, name: &str, reading: &Reading, ctx: &Ctx) {
    let s = &ctx.style;
    let ring = Ring::new(ctx.aspect);
    let x = cell.x + cell.width.saturating_sub(ring.width) / 2;
    let full = reading
        .share
        .map_or(0, |share| (share * DOTS as f64).round() as usize);
    for k in 0..DOTS {
        let style = match k < full {
            true => s.bold(reading.tone()),
            false => s.tone(Tone::Muted),
        };
        let (col, row, bit) = ring.cell(k);
        let glyph = char::from_u32(0x2800 + u32::from(bit)).map(String::from);
        put(
            buf,
            x + col,
            cell.y + row,
            &glyph.unwrap_or_default(),
            1,
            style,
        );
    }
    let lines = [
        (reading.value.as_str(), s.bold(Tone::Normal)),
        (name, s.tone(Tone::Muted)),
    ];
    for (row, (text, style)) in (HEIGHT / 2 - 1..).zip(lines) {
        let room = ring.inside(row);
        let text = cut(text, room.into(), s.glyphs.ellipsis);
        let left = (ring.axis() - width(&text) as f64 / 2.0).round() as u16;
        put(buf, x + left, cell.y + row, &text, room, style);
    }
}

/// The name and the value on the top row of `cell`, in its middle, the value in the state of
/// its share.
pub fn line(buf: &mut Buffer, cell: Rect, name: &str, reading: &Reading, ctx: &Ctx) {
    let s = &ctx.style;
    let room = usize::from(cell.width).saturating_sub(width(name) + 2);
    let value = cut(&reading.value, room, s.glyphs.ellipsis);
    let used = (width(name) + 1 + width(&value)) as u16;
    let x = cell.x + cell.width.saturating_sub(used) / 2;
    let x = put(buf, x, cell.y, name, cell.width, s.tone(Tone::Muted));
    put(
        buf,
        x + 1,
        cell.y,
        &value,
        cell.right() - x - 1,
        s.bold(reading.tone()),
    );
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
    fn the_ring_turns_from_success_to_warning_to_error() {
        assert_eq!(tone(0.0), Tone::Ok);
        assert_eq!(tone(0.749), Tone::Ok);
        assert_eq!(tone(0.75), Tone::Warn);
        assert_eq!(tone(0.9), Tone::Error);
        assert_eq!(tone(1.0), Tone::Error);
    }

    #[test]
    fn the_ring_is_round_even_and_symmetric_with_one_dot_at_top_and_bottom() {
        let mut a = ASPECTS.0;
        while a <= ASPECTS.1 {
            let ring = Ring::new(a as f32);
            let (cx, cy) = (ring.axis(), f64::from(HEIGHT) * a / 2.0);
            let polar: Vec<(f64, f64)> = ring
                .points
                .iter()
                .map(|&(j, i)| {
                    let (x, y) = ((f64::from(j) + 0.5) / 2.0, (f64::from(i) + 0.5) / 4.0 * a);
                    let angle = (x - cx).atan2(cy - y).to_degrees().rem_euclid(360.0);
                    ((x - cx).hypot(y - cy), angle)
                })
                .collect();
            // Round and even to within a braille point, the grid's finest step.
            let (near, far) = polar
                .iter()
                .fold((f64::MAX, 0.0_f64), |(n, f), (r, _)| (n.min(*r), f.max(*r)));
            assert!(far - near < 0.5, "round at {a}: {near}..{far}");
            for k in 0..DOTS {
                let gap = (polar[(k + 1) % DOTS].1 - polar[k].1).rem_euclid(360.0);
                assert!((gap - 36.0).abs() < 3.0, "even at {a}: dot {k} {gap}");
            }
            let top = ring.points[0];
            assert_eq!(f64::from(top.0) / 2.0 + 0.25, cx, "on the axis: {a}");
            assert!(
                ring.points[1..].iter().all(|p| p.1 > top.1),
                "alone at the top: {a}"
            );
            // A cell has one colour, so it belongs to one dot.
            let mut taken = std::collections::BTreeSet::new();
            for k in 0..DOTS {
                let (col, row, _) = ring.cell(k);
                assert!(col < ring.width && row < HEIGHT, "inside: {a}");
                assert!(taken.insert((col, row)), "dot {k} shares a cell: {a}");
            }
            assert!(ring.inside(HEIGHT / 2 - 1) >= 4 && ring.inside(HEIGHT / 2) >= 4);
            a += 0.05;
        }
    }
}
