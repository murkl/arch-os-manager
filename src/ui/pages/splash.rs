//! The start of the interface and of every reload, until the machine is read: the Arch logo drawn
//! line by line, the name typed after it, a spinner naming a reading that takes longer. Any key but
//! **r** ends it, and nothing waits on it.

use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use unicode_width::UnicodeWidthStr;

use crate::i18n::t;
use crate::ui::draw::{cut, geometry, put};
use crate::ui::{Ctx, Does, Flow, Key, Nav, Page, Tone};

const NAME: &str = "System Manager";

/// How long each part takes, in milliseconds: a line of the logo begins every `LINE`, the name
/// is typed a letter every `LETTER` once the last line has begun, and the whole lasts `END` at
/// least. A reading still under way then turns a spinner, for `WAIT` at most.
const LINE: u64 = 70;
const FADE: u64 = 260;
const LETTER: u64 = 28;
pub(crate) const END: u64 = 3000;
const WAIT: u64 = 20_000;

/// Rows between the logo and the name.
const GAP: u16 = 1;

pub struct SplashPage {
    started: Instant,
    /// The generation before the reading it covers, which begins right after it opens.
    before: u64,
}

impl SplashPage {
    pub fn new(ctx: &Ctx) -> SplashPage {
        SplashPage {
            started: Instant::now(),
            before: ctx.generation,
        }
    }

    /// As it looks `ms` after the start, for the screenshots.
    #[cfg(test)]
    pub fn after(ms: u64, ctx: &Ctx) -> SplashPage {
        SplashPage {
            started: Instant::now() - Duration::from_millis(ms),
            ..SplashPage::new(ctx)
        }
    }

    fn at(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    /// Its reading has not begun, or the machine has not said everything yet.
    fn waiting(&self, ctx: &Ctx) -> bool {
        ctx.generation == self.before || ctx.settling()
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
        let logo = s.logo(area.width, area.height);
        let rows = logo.len() as u16;
        // The logo, the gap, the name and the version as one block in the middle.
        let top = area.y + area.height.saturating_sub(rows + GAP + 2) / 2;
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
        let type_at = u64::from(rows) * LINE;
        if at >= type_at {
            let letters = (((at - type_at) / LETTER) as usize + 1).min(NAME.len());
            let shown = &NAME[..letters];
            let nx = area.x + area.width.saturating_sub(NAME.len() as u16) / 2;
            put(
                buf,
                nx,
                top + rows + GAP,
                shown,
                area.width,
                s.bold(Tone::Accent),
            );
            if letters == NAME.len() {
                let since = at - type_at - (NAME.len() as u64 - 1) * LETTER;
                let version = crate::version::version();
                let vx = area.x + area.width.saturating_sub(version.len() as u16) / 2;
                let color = mix(s.background, s.muted, since as f64 / FADE as f64);
                put(
                    buf,
                    vx,
                    top + rows + GAP + 1,
                    version,
                    area.width,
                    s.base().fg(color),
                );
            }
        }
        if at >= END && self.waiting(ctx) {
            let share = (at - END) as f64 / FADE as f64;
            let muted = s.base().fg(mix(s.background, s.muted, share));
            let sx = area.x + area.width.saturating_sub(1) / 2;
            let y = top + rows + GAP + 2 + GAP;
            let accent = s.base().fg(mix(s.background, s.accent, share));
            put(buf, sx, y, s.spinner(), area.width, accent);
            let centred = |buf: &mut Buffer, y, text: &str| {
                let text = cut(text, area.width.into(), s.glyphs.ellipsis);
                let x = area.x + area.width.saturating_sub(text.width() as u16) / 2;
                put(buf, x, y, &text, area.width, muted);
            };
            if let Some(doing) = ctx.doing() {
                centred(buf, y + 1, doing);
            }
            // Where the other pages name their keys.
            if let Some((.., bottom)) = geometry(area).filter(|g| g.3 > y + 1) {
                centred(buf, bottom, t("Press any key"));
            }
        }
    }

    fn hints(&self, _: &Ctx) -> Vec<(&'static str, Does)> {
        Vec::new()
    }

    fn key(&mut self, key: Key, _: &mut Ctx) -> Flow {
        match key.nav(false) {
            Some(Nav::Quit) => Flow::Quit,
            _ if key == Key::Char('r') => Flow::Stay,
            _ => Flow::Pop,
        }
    }

    fn tick(&mut self, ctx: &mut Ctx) -> Flow {
        let at = self.at();
        if at < END {
            return Flow::Stay;
        }
        if !self.waiting(ctx) {
            return Flow::Pop;
        }
        if at < WAIT {
            return Flow::Stay;
        }
        crate::warn!(
            "reading still under way after {} s, the start page shows what is in",
            WAIT / 1000
        );
        Flow::Pop
    }

    fn animating(&self, _: &Ctx) -> bool {
        true
    }

    /// Smooth until the spinner has faded in, then on the spinner's beat.
    fn pace(&self) -> Duration {
        match self.at() < END + FADE {
            true => crate::ui::FRAME,
            false => crate::ui::BEAT,
        }
    }

    fn typing(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::Msg;
    use crate::ui::app::tests::{app, open, screen};

    #[test]
    fn the_splash_ends_once_the_animation_has_run_and_the_machine_is_read_or_at_its_limit() {
        let (mut app, _) = app();
        let mut page = SplashPage::after(END, &app.ctx);
        assert!(matches!(page.tick(&mut app.ctx), Flow::Stay), "not begun");
        app.ctx.generation += 1;
        app.ctx.reload = true;
        assert!(matches!(page.tick(&mut app.ctx), Flow::Stay), "under way");
        app.ctx.reload = false;
        assert!(matches!(page.tick(&mut app.ctx), Flow::Pop), "in");

        let mut early = SplashPage::after(0, &app.ctx);
        app.ctx.generation += 1;
        assert!(matches!(early.tick(&mut app.ctx), Flow::Stay), "animating");

        let mut late = SplashPage::after(WAIT, &app.ctx);
        app.ctx.generation += 1;
        app.ctx.reload = true;
        assert!(matches!(late.tick(&mut app.ctx), Flow::Pop), "at its limit");
    }

    #[test]
    fn under_the_spinner_stands_the_oldest_job_of_the_machine_and_at_the_bottom_how_to_go_on() {
        use crate::script::op;
        let _lang = crate::i18n::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::i18n::set(crate::i18n::Lang::En);
        let (mut app, mut term) = app();
        let mut page = SplashPage::after(END + FADE, &app.ctx);
        app.ctx.generation += 1;
        for (op, remote) in [
            (op::UPGRADE_LIST, true),
            (op::SYSTEM_STATE, false),
            (op::SERVICES_LIST, false),
        ] {
            let _ = app.ctx.begin(Some(op), remote);
        }
        type Term = ratatui::Terminal<ratatui::backend::TestBackend>;
        let shown = |app: &mut crate::ui::App, term: &mut Term| {
            let drawn = SplashPage::after(END + FADE, &app.ctx);
            open(app, Box::new(drawn));
            screen(app, term)
        };
        let first = shown(&mut app, &mut term);
        let last = first.lines().rfind(|l| !l.trim().is_empty());
        assert!(first.contains("Reading the system state"), "{first}");
        assert!(!first.contains("Checking for updates"), "{first}");
        assert_eq!(last.map(str::trim), Some("Press any key"), "{first}");
        assert!(matches!(page.tick(&mut app.ctx), Flow::Stay), "the machine");
        let ids: Vec<u64> = app.ctx.loads.iter().map(|l| l.id).collect();
        app.handle(Msg::Loaded(ids[1]));
        let next = shown(&mut app, &mut term);
        assert!(next.contains("Listing the services"), "{next}");
        app.handle(Msg::Loaded(ids[2]));
        assert!(app.ctx.busy() && matches!(page.tick(&mut app.ctx), Flow::Pop));
    }

    #[test]
    fn mixes_colours() {
        assert_eq!(
            mix(Color::Rgb(0, 0, 0), Color::Rgb(200, 100, 50), 0.5),
            Color::Rgb(100, 50, 25)
        );
        assert_eq!(mix(Color::Reset, Color::Cyan, 0.2), Color::Reset);
    }
}
