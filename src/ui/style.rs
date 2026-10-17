//! The theme as styles, and the glyphs the target can show.

use ratatui::style::{Color, Modifier, Style as Cell};

use super::Tone;
use crate::theme::{Colors, Rgb};

#[derive(Clone, Copy, Debug)]
pub struct Style {
    pub background: Color,
    pub surface: Color,
    pub muted: Color,
    pub text: Color,
    pub accent: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub glyphs: &'static Glyphs,
}

#[derive(Debug)]
pub struct Glyphs {
    pub cursor: &'static str,
    pub dot: &'static str,
    pub ring: &'static str,
    /// Before an item of running text.
    pub bullet: &'static str,
    pub rule: &'static str,
    pub path: &'static str,
    pub arrow: &'static str,
    pub ellipsis: &'static str,
    pub separator: &'static str,
    pub bar: &'static str,
    pub track: &'static str,
    /// A scrollbar's track, then its thumb.
    pub scroll: [&'static str; 2],
    /// The mark of a chosen row.
    pub mark: &'static str,
    /// Whether the font has the icons in `icon`.
    pub icons: bool,
    /// Whether the font has the block elements the logo is drawn with.
    pub blocks: bool,
    /// Whether the font has braille, which the rings are drawn in.
    pub braille: bool,
    /// Corners top left, top right, bottom left, bottom right, then horizontal and vertical.
    pub border: [&'static str; 6],
    pub spinner: &'static [&'static str],
}

pub static UNICODE: Glyphs = Glyphs {
    cursor: "▶",
    dot: "●",
    ring: "○",
    bullet: "•",
    rule: "─",
    path: "›",
    arrow: "→",
    ellipsis: "…",
    separator: "·",
    bar: "━",
    track: "─",
    scroll: ["│", "┃"],
    mark: super::icon::CHECK,
    icons: true,
    blocks: true,
    braille: true,
    border: ["╭", "╮", "╰", "╯", "─", "│"],
    spinner: &["◐", "◓", "◑", "◒"],
};

/// The Linux console's font has none of the above.
pub static ASCII: Glyphs = Glyphs {
    cursor: ">",
    dot: "*",
    ring: "o",
    bullet: "-",
    rule: "-",
    path: ">",
    arrow: "->",
    ellipsis: "...",
    separator: "|",
    bar: "#",
    track: "-",
    scroll: ["|", "#"],
    mark: "+",
    icons: false,
    blocks: false,
    braille: false,
    border: ["+", "+", "+", "+", "-", "|"],
    spinner: &["|", "/", "-", "\\"],
};

fn rgb(c: Rgb) -> Color {
    Color::Rgb(c.0, c.1, c.2)
}

impl Style {
    pub fn new(colors: &Colors, console: bool) -> Style {
        if console {
            // Sixteen colours, by their slots: the console's own palette decides the look.
            return Style {
                background: Color::Reset,
                // Black is the console's background, so a track in it would not show.
                surface: Color::DarkGray,
                muted: Color::DarkGray,
                text: Color::Reset,
                accent: Color::Cyan,
                success: Color::Green,
                warning: Color::Yellow,
                error: Color::Red,
                glyphs: &ASCII,
            };
        }
        Style {
            background: rgb(colors.background),
            surface: rgb(colors.surface),
            muted: rgb(colors.muted),
            text: rgb(colors.text),
            accent: rgb(colors.accent),
            success: rgb(colors.success),
            warning: rgb(colors.warning),
            error: rgb(colors.error),
            glyphs: &UNICODE,
        }
    }

    pub fn color(&self, tone: Tone) -> Color {
        match tone {
            Tone::Normal => self.text,
            Tone::Muted => self.muted,
            Tone::Accent => self.accent,
            Tone::Ok => self.success,
            Tone::Warn => self.warning,
            Tone::Error => self.error,
        }
    }

    pub fn base(&self) -> Cell {
        Cell::new().fg(self.text).bg(self.background)
    }

    pub fn tone(&self, tone: Tone) -> Cell {
        self.base().fg(self.color(tone))
    }

    pub fn bold(&self, tone: Tone) -> Cell {
        self.tone(tone).add_modifier(Modifier::BOLD)
    }

    pub fn spinner(&self) -> &'static str {
        let frames = self.glyphs.spinner;
        let turns = super::beats(std::time::Instant::now(), super::BEAT) / 3;
        frames[turns as usize % frames.len()]
    }

    /// The Arch Linux mark, its blocks as `#` where the font has none.
    pub fn logo(&self) -> Vec<String> {
        LOGO.iter()
            .map(|l| match self.glyphs.blocks {
                true => l.to_string(),
                false => l.replace(['▄', '█', '▀'], "#"),
            })
            .collect()
    }
}

/// The Arch Linux mark of the Arch OS installer.
const LOGO: [&str; 5] = [
    "    ▄█▄    ",
    "   ▄███▄   ",
    "  ▄█████▄  ",
    " ▄██▀ ▀██▄ ",
    "▄██▀   ▀██▄",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_colour_drawn_shows_on_the_consoles_black() {
        let s = Style::new(&crate::theme::all().0[0].colors, true);
        for fg in [s.surface, s.muted, s.accent, s.success, s.warning, s.error] {
            assert_ne!(fg, Color::Black);
        }
    }

    #[test]
    fn the_logo_is_square_and_ascii_on_the_console() {
        let colors = &crate::theme::all().0[0].colors;
        let logo = Style::new(colors, false).logo();
        assert!(
            logo.iter()
                .all(|l| l.chars().count() == LOGO[0].chars().count())
        );
        assert!(Style::new(colors, true).logo().iter().all(|l| l.is_ascii()));
    }
}
