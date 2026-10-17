//! The theme as styles, and the glyphs the target can show.

use ratatui::style::{Color, Modifier, Style as Cell};

use super::Tone;
use crate::theme::{Colors, Rgb};

#[derive(Clone, Copy, Debug)]
pub struct Style {
    pub background: Color,
    pub surface: Color,
    /// Behind the row under the cursor.
    pub selection: Color,
    pub muted: Color,
    pub text: Color,
    pub accent: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub glyphs: &'static Glyphs,
    /// Muted text is also dim: the console shows dark grey by a brightness bit that only the
    /// return to normal intensity clears, which a dropped dim sends.
    dim_muted: bool,
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
    /// The keys as the footer names them: up and down, enter, page up and down.
    pub up_down: &'static str,
    pub enter: &'static str,
    pub page_keys: &'static str,
    /// The caret of a text box, and what stands for a typed letter of a password.
    pub caret: &'static str,
    pub mask: &'static str,
    /// Whether the font has the icons in `icon`.
    pub icons: bool,
    /// Whether the font has the block elements the logo is drawn with.
    pub blocks: bool,
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
    up_down: "↑↓",
    enter: "⏎",
    page_keys: "⇞⇟",
    caret: "▏",
    mask: "•",
    icons: true,
    blocks: true,
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
    up_down: "up/down",
    enter: "enter",
    page_keys: "pgup/pgdn",
    caret: "_",
    mask: "*",
    icons: false,
    blocks: false,
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
                // Behind text the console takes only its first eight colours; a bright one,
                // such as dark grey, blinks or vanishes.
                selection: Color::Blue,
                muted: Color::DarkGray,
                text: Color::Reset,
                accent: Color::Cyan,
                success: Color::Green,
                warning: Color::Yellow,
                error: Color::Red,
                glyphs: &ASCII,
                dim_muted: true,
            };
        }
        Style {
            background: rgb(colors.background),
            surface: rgb(colors.surface),
            selection: rgb(colors.surface),
            muted: rgb(colors.muted),
            text: rgb(colors.text),
            accent: rgb(colors.accent),
            success: rgb(colors.success),
            warning: rgb(colors.warning),
            error: rgb(colors.error),
            glyphs: &UNICODE,
            dim_muted: false,
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
        let cell = self.base().fg(self.color(tone));
        match tone == Tone::Muted && self.dim_muted {
            true => cell.add_modifier(Modifier::DIM),
            false => cell,
        }
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

/// The Arch Linux mark in half blocks, which the window draws to the pixel.
const LOGO: [&str; 9] = [
    "          ▄█▄          ",
    "         ▄███▄         ",
    "        ▄█████▄        ",
    "       ▄███████▄       ",
    "      ███████████      ",
    "    ▄█████▀ ▀█████▄    ",
    "   ▄█████▀   ▀█████▄   ",
    "  ████▀▀▀     ▀▀▀████  ",
    "▄█▀▀▀             ▀▀▀█▄",
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
    fn the_console_dims_muted_text_so_the_next_cell_returns_to_normal() {
        let colors = &crate::theme::all().0[0].colors;
        let console = Style::new(colors, true);
        assert!(
            console
                .tone(Tone::Muted)
                .add_modifier
                .contains(Modifier::DIM)
        );
        assert!(
            !console
                .tone(Tone::Normal)
                .add_modifier
                .contains(Modifier::DIM)
        );
        assert!(
            !Style::new(colors, false)
                .tone(Tone::Muted)
                .add_modifier
                .contains(Modifier::DIM)
        );
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
