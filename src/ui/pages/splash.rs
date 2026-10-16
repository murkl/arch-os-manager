//! The first three seconds of the interface, while the first values load: the Arch logo drawn
//! line by line in a sweep of the accent, the name typed after it. Any key ends it at once;
//! nothing waits on it.

use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;

use crate::ui::draw::put;
use crate::ui::{Ctx, Flow, Key, Page, Tone};

const NAME: &str = "System Manager";

/// When each part starts and how long the whole lasts, in milliseconds.
const LINE: u64 = 70;
const FADE: u64 = 260;
const TYPE_AT: u64 = 560;
const LETTER: u64 = 28;
pub(crate) const END: u64 = 3000;

pub struct SplashPage {
    started: Instant,
}

impl SplashPage {
    pub fn new() -> SplashPage {
        SplashPage {
            started: Instant::now(),
        }
    }

    /// As it looks `ms` after the start, for the screenshots.
    #[cfg(test)]
    pub fn after(ms: u64) -> SplashPage {
        SplashPage {
            started: Instant::now() - Duration::from_millis(ms),
        }
    }

    fn at(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }
}

/// `to` reached from `from` by `share`, where both are colours a terminal mixes.
fn mix(from: Color, to: Color, share: f64) -> Color {
    match (from, to) {
        (Color::Rgb(a, b, c), Color::Rgb(x, y, z)) => {
            let m = |p: u8, q: u8| {
                (p as f64 + (q as f64 - p as f64) * share.clamp(0.0, 1.0)).round() as u8
            };
            Color::Rgb(m(a, x), m(b, y), m(c, z))
        }
        _ if share >= 0.5 => to,
        _ => from,
    }
}

impl Page for SplashPage {
    fn title(&self) -> String {
        String::new()
    }

    fn bare(&self) -> bool {
        true
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, ctx: &mut Ctx) {
        let s = &ctx.style;
        let at = self.at();
        let logo = s.logo();
        let height = logo.len() as u16 + 4;
        let top = area.y + area.height.saturating_sub(height) / 2;
        let logo_width = logo[0].chars().count() as u16;
        let x = area.x + area.width.saturating_sub(logo_width) / 2;
        for (i, line) in logo.iter().enumerate() {
            let begin = i as u64 * LINE;
            if at < begin {
                break;
            }
            // Each line fades in from the background, the lower ones toward the text colour.
            let target = mix(s.accent, s.text, i as f64 / (logo.len() * 3) as f64);
            let color = mix(s.background, target, (at - begin) as f64 / FADE as f64);
            put(buf, x, top + i as u16, line, logo_width, s.base().fg(color));
        }
        if at >= TYPE_AT {
            let letters = (((at - TYPE_AT) / LETTER) as usize + 1).min(NAME.len());
            let shown = &NAME[..letters];
            let nx = area.x + area.width.saturating_sub(NAME.len() as u16) / 2;
            put(
                buf,
                nx,
                top + logo.len() as u16 + 2,
                shown,
                area.width,
                s.bold(Tone::Accent),
            );
            if letters == NAME.len() {
                let since = at - TYPE_AT - (NAME.len() as u64 - 1) * LETTER;
                let version = crate::version::version();
                let vx = area.x + area.width.saturating_sub(version.len() as u16) / 2;
                let color = mix(s.background, s.muted, since as f64 / FADE as f64);
                put(
                    buf,
                    vx,
                    top + logo.len() as u16 + 3,
                    version,
                    area.width,
                    s.base().fg(color),
                );
            }
        }
    }

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, String)> {
        Vec::new()
    }

    fn key(&mut self, _: Key, _: &mut Ctx) -> Flow {
        Flow::Pop
    }

    fn tick(&mut self, _: &mut Ctx) -> Flow {
        if self.started.elapsed() >= Duration::from_millis(END) {
            Flow::Pop
        } else {
            Flow::Stay
        }
    }

    fn animating(&self, _: &Ctx) -> bool {
        true
    }

    fn typing(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixes_colours() {
        assert_eq!(
            mix(Color::Rgb(0, 0, 0), Color::Rgb(200, 100, 50), 0.5),
            Color::Rgb(100, 50, 25)
        );
        assert_eq!(mix(Color::Reset, Color::Cyan, 0.2), Color::Reset);
    }
}
