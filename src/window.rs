//! The interface in a window of its own: the same cells as in a terminal, drawn with an
//! embedded font. The display libraries are loaded only here, so the binary starts without them.

use std::collections::HashMap;
use std::io;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fontdue::{Font, FontSettings, Metrics};
use ratatui::Terminal;
use ratatui::backend::{Backend, ClearType, WindowSize};
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::{Position, Rect, Size};
use ratatui::style::{Color, Modifier};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key as Named, ModifiersState, NamedKey};
use winit::window::{CursorIcon, ResizeDirection, Window, WindowAttributes, WindowId};

use crate::ui::{App, Key, Msg, Target};

const REGULAR: &[u8] = include_bytes!("../assets/fonts/FiraCode-6.2-Regular.ttf");
const BOLD: &[u8] = include_bytes!("../assets/fonts/FiraCode-6.2-Bold.ttf");
/// The icons, where Fira Code has none: each one cell wide.
const SYMBOLS: &[u8] = include_bytes!("../assets/fonts/SymbolsNerdFontMono-3.4.0-Regular.ttf");
/// Logical pixels: the font size, the margin around the cells, the band that resizes.
const FONT: f32 = 15.0;
const MARGIN: f64 = 24.0;
const EDGE: f64 = 6.0;

pub fn run(start: Option<Target>) -> io::Result<()> {
    let event_loop = EventLoop::<()>::with_user_event()
        .build()
        .map_err(|e| io::Error::other(format!("no display: {e}")))?;
    let proxy = Mutex::new(event_loop.create_proxy());
    let (tx, rx) = mpsc::channel();
    let app = App::new(
        tx,
        Arc::new(move || {
            let _ = proxy.lock().unwrap().send_event(());
        }),
        false,
        start,
    );
    let mut win = Win::new(app, rx);
    event_loop.run_app(&mut win).map_err(io::Error::other)?;
    win.error.map_or(Ok(()), Err)
}

/// The cell grid ratatui draws into.
struct Grid {
    buffer: Buffer,
}

impl Backend for Grid {
    type Error = io::Error;

    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        for (x, y, cell) in content {
            if let Some(c) = self.buffer.cell_mut((x, y)) {
                *c = cell.clone();
            }
        }
        Ok(())
    }
    fn hide_cursor(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn show_cursor(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn get_cursor_position(&mut self) -> io::Result<Position> {
        Ok(Position::ORIGIN)
    }
    fn set_cursor_position<P: Into<Position>>(&mut self, _: P) -> io::Result<()> {
        Ok(())
    }
    fn clear(&mut self) -> io::Result<()> {
        self.buffer.reset();
        Ok(())
    }
    fn clear_region(&mut self, _: ClearType) -> io::Result<()> {
        self.clear()
    }
    fn size(&self) -> io::Result<Size> {
        Ok(self.buffer.area.as_size())
    }
    fn window_size(&mut self) -> io::Result<WindowSize> {
        Ok(WindowSize {
            columns_rows: self.buffer.area.as_size(),
            pixels: Size::default(),
        })
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct Fonts {
    regular: Font,
    bold: Font,
    symbols: Font,
    px: f32,
    cell: (u32, u32),
    baseline: f32,
    cache: HashMap<(char, bool), (Metrics, Vec<u8>)>,
}

impl Fonts {
    fn new(scale: f64) -> Fonts {
        let load = |bytes| Font::from_bytes(bytes, FontSettings::default()).expect("embedded font");
        let (regular, bold, symbols) = (load(REGULAR), load(BOLD), load(SYMBOLS));
        let px = (FONT as f64 * scale) as f32;
        let width = regular.metrics('M', px).advance_width.round().max(1.0) as u32;
        let line = regular
            .horizontal_line_metrics(px)
            .expect("horizontal metrics");
        let glyph_height = line.ascent - line.descent;
        let height = (glyph_height * 1.25).round().max(1.0) as u32;
        let baseline = (height as f32 - glyph_height) / 2.0 + line.ascent;
        Fonts {
            regular,
            bold,
            symbols,
            px,
            cell: (width, height),
            baseline,
            cache: HashMap::new(),
        }
    }

    fn glyph(&mut self, c: char, bold: bool) -> &(Metrics, Vec<u8>) {
        let (regular, heavy, symbols, px) = (&self.regular, &self.bold, &self.symbols, self.px);
        let cell_width = self.cell.0 as f32;
        self.cache.entry((c, bold)).or_insert_with(|| {
            if regular.lookup_glyph_index(c) == 0 && symbols.lookup_glyph_index(c) != 0 {
                // An icon has two cells before its label; one wider is scaled down to them.
                let wide = symbols.metrics(c, px).width as f32;
                let fit = if wide > 2.0 * cell_width {
                    px * 2.0 * cell_width / wide
                } else {
                    px
                };
                symbols.rasterize(c, fit)
            } else if bold {
                heavy.rasterize(c, px)
            } else {
                regular.rasterize(c, px)
            }
        })
    }
}

struct Win {
    app: App,
    rx: Receiver<Msg>,
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    terminal: Terminal<Grid>,
    fonts: Fonts,
    modifiers: ModifiersState,
    pointer: PhysicalPosition<f64>,
    error: Option<io::Error>,
}

impl Win {
    fn new(app: App, rx: Receiver<Msg>) -> Win {
        let terminal = Terminal::new(Grid {
            buffer: Buffer::empty(Rect::new(0, 0, 80, 24)),
        })
        .expect("an in-memory grid");
        Win {
            app,
            rx,
            window: None,
            surface: None,
            terminal,
            fonts: Fonts::new(1.0),
            modifiers: ModifiersState::empty(),
            pointer: PhysicalPosition::new(0.0, 0.0),
            error: None,
        }
    }

    fn handle(&mut self, msg: Msg, event_loop: &ActiveEventLoop) {
        self.app.handle(msg);
        if self.app.done() {
            event_loop.exit();
        } else if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn redraw(&mut self) -> Result<(), String> {
        let (Some(window), Some(surface)) = (&self.window, self.surface.as_mut()) else {
            return Ok(());
        };
        let size = window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return Ok(());
        };
        surface.resize(w, h).map_err(|e| e.to_string())?;
        let (_, _, area) = layout(size.width, size.height, &self.fonts, window.scale_factor());
        if self.terminal.backend().buffer.area != area {
            self.terminal.backend_mut().buffer = Buffer::empty(area);
        }
        let app = &mut self.app;
        self.terminal
            .draw(|f| app.draw(f))
            .map_err(|e| e.to_string())?;
        let mut pixels = surface.buffer_mut().map_err(|e| e.to_string())?;
        paint(
            &self.terminal.backend().buffer,
            &mut self.fonts,
            &self.app.ctx.style,
            size.width,
            size.height,
            window.scale_factor(),
            &mut pixels,
        );
        pixels.present().map_err(|e| e.to_string())
    }

    /// The edge the pointer is on, for resizing a window without decorations.
    fn edge(&self) -> Option<ResizeDirection> {
        let window = self.window.as_ref()?;
        let size = window.inner_size();
        let band = EDGE * window.scale_factor();
        let (x, y) = (self.pointer.x, self.pointer.y);
        let (w, h) = (size.width as f64, size.height as f64);
        let (left, right, top, bottom) = (x < band, x > w - band, y < band, y > h - band);
        Some(match (left, right, top, bottom) {
            (true, _, true, _) => ResizeDirection::NorthWest,
            (_, true, true, _) => ResizeDirection::NorthEast,
            (true, _, _, true) => ResizeDirection::SouthWest,
            (_, true, _, true) => ResizeDirection::SouthEast,
            (true, ..) => ResizeDirection::West,
            (_, true, ..) => ResizeDirection::East,
            (_, _, true, _) => ResizeDirection::North,
            (_, _, _, true) => ResizeDirection::South,
            _ => return None,
        })
    }

    fn key(&self, key: &Named, text: Option<&str>) -> Vec<Key> {
        let ctrl = self.modifiers.control_key();
        let shift = self.modifiers.shift_key();
        let named = match key {
            Named::Named(n) => Some(match n {
                NamedKey::Enter => Key::Enter,
                NamedKey::Escape => Key::Esc,
                NamedKey::Backspace => Key::Backspace,
                NamedKey::Delete => Key::Delete,
                NamedKey::Tab if shift => Key::BackTab,
                NamedKey::Tab => Key::Tab,
                NamedKey::ArrowUp => Key::Up,
                NamedKey::ArrowDown => Key::Down,
                NamedKey::ArrowLeft => Key::Left,
                NamedKey::ArrowRight => Key::Right,
                NamedKey::PageUp => Key::PageUp,
                NamedKey::PageDown => Key::PageDown,
                NamedKey::Home => Key::Home,
                NamedKey::End => Key::End,
                NamedKey::Space => Key::Char(' '),
                _ => return Vec::new(),
            }),
            Named::Character(s) if ctrl => {
                s.chars().next().map(|c| Key::Ctrl(c.to_ascii_lowercase()))
            }
            Named::Character(_) => None,
            _ => return Vec::new(),
        };
        match named {
            Some(k) => vec![k],
            None => text
                .unwrap_or_default()
                .chars()
                .filter(|c| !c.is_control())
                .map(Key::Char)
                .collect(),
        }
    }
}

impl ApplicationHandler<()> for Win {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = WindowAttributes::default()
            .with_title("System Manager")
            .with_decorations(false)
            .with_inner_size(LogicalSize::new(1080.0, 720.0))
            .with_min_inner_size(LogicalSize::new(400.0, 280.0));
        let attributes = winit::platform::wayland::WindowAttributesExtWayland::with_name(
            attributes,
            "system-manager",
            "system-manager",
        );
        let created = event_loop
            .create_window(attributes)
            .map_err(|e| e.to_string())
            .and_then(|w| {
                let w = Rc::new(w);
                let context = softbuffer::Context::new(w.clone()).map_err(|e| e.to_string())?;
                let surface =
                    softbuffer::Surface::new(&context, w.clone()).map_err(|e| e.to_string())?;
                Ok((w, surface))
            });
        match created {
            Ok((w, surface)) => {
                self.fonts = Fonts::new(w.scale_factor());
                self.window = Some(w);
                self.surface = Some(surface);
            }
            Err(e) => {
                self.error = Some(io::Error::other(format!("no window: {e}")));
                event_loop.exit();
            }
        }
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        if let StartCause::ResumeTimeReached { .. } = cause {
            self.handle(Msg::Tick, event_loop);
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, (): ()) {
        while let Ok(msg) = self.rx.try_recv() {
            self.handle(msg, event_loop);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            // A run asks before it is stopped, as with ctrl+c.
            WindowEvent::CloseRequested => self.handle(Msg::Key(Key::Ctrl('c')), event_loop),
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.redraw() {
                    crate::error!("drawing the window: {e}");
                }
            }
            WindowEvent::Resized(_) => self.handle(Msg::Resize, event_loop),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.fonts = Fonts::new(scale_factor);
                self.handle(Msg::Resize, event_loop);
            }
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                for key in self.key(&event.logical_key, event.text.as_deref()) {
                    self.handle(Msg::Key(key), event_loop);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer = position;
                let icon = match self.edge() {
                    Some(ResizeDirection::East | ResizeDirection::West) => CursorIcon::EwResize,
                    Some(ResizeDirection::North | ResizeDirection::South) => CursorIcon::NsResize,
                    Some(ResizeDirection::NorthWest | ResizeDirection::SouthEast) => {
                        CursorIcon::NwseResize
                    }
                    Some(_) => CursorIcon::NeswResize,
                    None => CursorIcon::Default,
                };
                if let Some(w) = &self.window {
                    w.set_cursor(icon);
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                let Some(w) = self.window.clone() else { return };
                let header = MARGIN * w.scale_factor() + 2.0 * self.fonts.cell.1 as f64;
                // Without decorations the window moves by its header and resizes at its edges.
                let _ = match self.edge() {
                    Some(direction) => w.drag_resize_window(direction),
                    None if self.pointer.y < header => w.drag_window(),
                    None => Ok(()),
                };
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y.round() as i32,
                    MouseScrollDelta::PixelDelta(p) => {
                        (p.y / self.fonts.cell.1 as f64).round() as i32
                    }
                };
                let key = if lines > 0 { Key::Up } else { Key::Down };
                for _ in 0..lines.unsigned_abs() {
                    self.handle(Msg::Key(key), event_loop);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let tick = if self.app.ticking() {
            Duration::from_millis(80)
        } else {
            Duration::from_millis(500)
        };
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + tick));
    }
}

/// Where the grid starts inside the margin, centred, and how many cells fit.
fn layout(width: u32, height: u32, fonts: &Fonts, scale: f64) -> (u32, u32, Rect) {
    let margin = (MARGIN * scale) as u32;
    let (cw, ch) = fonts.cell;
    let cols = (width.saturating_sub(2 * margin) / cw).clamp(1, u16::MAX as u32);
    let rows = (height.saturating_sub(2 * margin) / ch).clamp(1, u16::MAX as u32);
    (
        (width.saturating_sub(cols * cw)) / 2,
        (height.saturating_sub(rows * ch)) / 2,
        Rect::new(0, 0, cols as u16, rows as u16),
    )
}

/// The grid as pixels, 0RGB.
fn paint(
    grid: &Buffer,
    fonts: &mut Fonts,
    style: &crate::ui::Style,
    width: u32,
    height: u32,
    scale: f64,
    pixels: &mut [u32],
) {
    let (ox, oy, _) = layout(width, height, fonts, scale);
    let (cw, ch) = fonts.cell;
    let (background, text) = (rgb(style.background, 0x2e3440), rgb(style.text, 0xd8dee9));
    pixels.fill(background);
    let mut canvas = Canvas {
        pixels,
        width,
        height,
    };
    for y in 0..grid.area.height {
        for x in 0..grid.area.width {
            let cell = &grid[(x, y)];
            let (mut fg, mut bg) = (rgb(cell.fg, text), rgb(cell.bg, background));
            if cell.modifier.contains(Modifier::REVERSED) {
                std::mem::swap(&mut fg, &mut bg);
            }
            let (px, py) = (ox + x as u32 * cw, oy + y as u32 * ch);
            if bg != background {
                canvas.fill(px, py, cw, ch, bg);
            }
            let Some(c) = cell.symbol().chars().next().filter(|c| *c != ' ') else {
                continue;
            };
            if !canvas.drawn(c, (px, py, cw, ch), (fg, bg), scale) {
                let baseline = fonts.baseline;
                let (m, coverage) = fonts.glyph(c, cell.modifier.contains(Modifier::BOLD));
                let gx = px as i32 + m.xmin;
                let gy = py as i32 + baseline.round() as i32 - m.height as i32 - m.ymin;
                canvas.blend((gx, gy), (m.width, m.height), coverage, (fg, bg));
            }
        }
    }
}

fn rgb(color: Color, default: u32) -> u32 {
    match color {
        Color::Rgb(r, g, b) => (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b),
        _ => default,
    }
}

struct Canvas<'a> {
    pixels: &'a mut [u32],
    width: u32,
    height: u32,
}

impl Canvas<'_> {
    fn fill(&mut self, x: u32, y: u32, w: u32, h: u32, color: u32) {
        for yy in y..(y + h).min(self.height) {
            for xx in x..(x + w).min(self.width) {
                self.pixels[(yy * self.width + xx) as usize] = color;
            }
        }
    }

    fn mix(&mut self, x: i32, y: i32, alpha: f32, fg: u32, bg: u32) {
        if x < 0 || y < 0 || x as u32 >= self.width || y as u32 >= self.height || alpha <= 0.0 {
            return;
        }
        let a = alpha.min(1.0);
        let channel = |shift: u32| {
            let (f, b) = ((fg >> shift) & 0xff, (bg >> shift) & 0xff);
            ((b as f32 + (f as f32 - b as f32) * a).round() as u32) << shift
        };
        self.pixels[(y as u32 * self.width + x as u32) as usize] =
            channel(16) | channel(8) | channel(0);
    }

    fn blend(
        &mut self,
        (x, y): (i32, i32),
        (w, h): (usize, usize),
        coverage: &[u8],
        (fg, bg): (u32, u32),
    ) {
        for row in 0..h {
            for col in 0..w {
                let a = coverage[row * w + col];
                if a > 0 {
                    self.mix(x + col as i32, y + row as i32, a as f32 / 255.0, fg, bg);
                }
            }
        }
    }

    /// Lines, corners and blocks drawn to the pixel, so they meet across cells as a font's do not.
    fn drawn(
        &mut self,
        c: char,
        (x, y, w, h): (u32, u32, u32, u32),
        (fg, bg): (u32, u32),
        scale: f64,
    ) -> bool {
        let thin = scale.round().max(1.0) as u32;
        let (cx, cy) = (x + w / 2, y + h / 2);
        match c {
            '─' => self.fill(x, cy - thin / 2, w, thin, fg),
            '━' => self.fill(x, cy - thin, w, thin * 2, fg),
            '│' => self.fill(cx - thin / 2, y, thin, h, fg),
            '▏' => self.fill(x, y + h / 8, thin.max(w / 8), h - h / 4, fg),
            '█' => self.fill(x, y, w, h, fg),
            '▀' => self.fill(x, y, w, h / 2, fg),
            '▄' => self.fill(x, y + h / 2, w, h - h / 2, fg),
            '╭' | '╮' | '╰' | '╯' => {
                // Toward the sides the corner joins: right or left, down or up.
                let hx = if matches!(c, '╭' | '╰') {
                    1.0
                } else {
                    -1.0
                };
                let vy = if matches!(c, '╭' | '╮') {
                    1.0
                } else {
                    -1.0
                };
                let t = thin as f32;
                // The centre lines of the straight pieces, as '─' and '│' lie in their cells.
                let lx = (cx - thin / 2) as f32 + t / 2.0;
                let ly = (cy - thin / 2) as f32 + t / 2.0;
                let r = (w.min(h) / 2) as f32;
                let (ax, ay) = (lx + hx * r, ly + vy * r);
                for yy in y..y + h {
                    for xx in x..x + w {
                        let (px, py) = (xx as f32 + 0.5, yy as f32 + 0.5);
                        let alpha = if (px - ax) * hx > 0.0 {
                            t / 2.0 + 0.5 - (py - ly).abs()
                        } else if (py - ay) * vy > 0.0 {
                            t / 2.0 + 0.5 - (px - lx).abs()
                        } else {
                            let d = ((px - ax).powi(2) + (py - ay).powi(2)).sqrt();
                            t / 2.0 + 0.5 - (d - r).abs()
                        };
                        self.mix(xx as i32, yy as i32, alpha, fg, bg);
                    }
                }
            }
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::ui::Style;

    #[test]
    fn the_fonts_have_every_glyph_the_interface_draws() {
        let font = Font::from_bytes(REGULAR, FontSettings::default()).unwrap();
        let symbols = Font::from_bytes(SYMBOLS, FontSettings::default()).unwrap();
        let style = Style::new(&crate::theme::all().0[0].colors, false);
        let (g, logo) = (style.glyphs, style.logo());
        let text = [
            g.cursor,
            g.dot,
            g.ring,
            g.rule,
            g.path,
            g.arrow,
            g.ellipsis,
            g.separator,
            g.bar,
            g.track,
            "↑↓⏎-",
        ]
        .into_iter()
        .chain(logo.iter().map(String::as_str))
        .chain(g.border)
        .chain(g.spinner.iter().copied());
        for s in text {
            for c in s.chars() {
                let drawn = matches!(
                    c,
                    '─' | '━' | '│' | '╭' | '╮' | '╰' | '╯' | '▏' | '█' | '▀' | '▄'
                );
                assert!(
                    font.lookup_glyph_index(c) != 0 || drawn,
                    "Fira Code has no {c:?}"
                );
            }
        }
        for icon in crate::ui::icon::ALL.iter().chain([&g.mark]) {
            for c in icon.chars() {
                assert!(
                    symbols.lookup_glyph_index(c) != 0,
                    "the symbols font has no {c:?}"
                );
            }
        }
    }

    /// Writes what a page looks like in the window, as PNG, from the same painter.
    pub fn png(app: &mut App, width: u32, height: u32, scale: f64, path: &std::path::Path) {
        let mut fonts = Fonts::new(scale);
        let (_, _, area) = layout(width, height, &fonts, scale);
        let mut term = Terminal::new(Grid {
            buffer: Buffer::empty(area),
        })
        .unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        let mut pixels = vec![0u32; (width * height) as usize];
        paint(
            &term.backend().buffer,
            &mut fonts,
            &app.ctx.style,
            width,
            height,
            scale,
            &mut pixels,
        );
        let rgb: Vec<u8> = pixels
            .iter()
            .flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8])
            .collect();
        let file = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
        let mut encoder = png::Encoder::new(file, width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&rgb)
            .unwrap();
    }

    #[test]
    fn the_grid_fills_the_window_inside_the_margin() {
        let fonts = Fonts::new(1.0);
        assert!(fonts.cell.0 >= 7 && fonts.cell.1 > fonts.cell.0);
    }
}
