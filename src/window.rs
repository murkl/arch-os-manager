//! The interface in a window of its own: the same cells as in a terminal, drawn with an
//! embedded font. The display libraries are loaded only here, so the binary starts without them.

use std::collections::HashMap;
use std::io;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use fontdue::{Font, FontSettings, Metrics};
use ratatui::Terminal;
use ratatui::backend::{Backend, ClearType, WindowSize};
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::{Position, Rect, Size};
use ratatui::style::{Color, Modifier};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key as Named, ModifiersState, NamedKey};
use winit::window::{CursorIcon, ResizeDirection, Window, WindowAttributes, WindowId};

use crate::ui::{App, FRAME, Key, Msg, Target, beats, next_beat};

const REGULAR: &[u8] = include_bytes!("../assets/fonts/FiraCode-6.2-Regular.ttf");
const BOLD: &[u8] = include_bytes!("../assets/fonts/FiraCode-6.2-Bold.ttf");
/// The icons src/ui/icon.rs names, cut from the Nerd Fonts symbols by tools/icons.sh: each one cell wide.
const SYMBOLS: &[u8] = include_bytes!("../assets/fonts/SymbolsNerdFontMono-3.4.0-Icons.ttf");
/// The cursor of a text box shows and hides on this beat, as a terminal's does.
const BLINK: Duration = Duration::from_millis(500);
/// Logical pixels: the font size, the margin around the cells, the band that resizes.
const FONT: f32 = 15.0;
const MARGIN: f64 = 24.0;
const EDGE: f64 = 6.0;

pub fn run(start: Option<Target>) -> io::Result<()> {
    let faces = Faces::new();
    let event_loop = EventLoop::<()>::with_user_event()
        .build()
        .map_err(|e| io::Error::other(format!("no display: {e}")))?;
    let proxy = Mutex::new(event_loop.create_proxy());
    let (tx, rx) = mpsc::sync_channel(crate::ui::QUEUE);
    let app = App::new(
        tx,
        Arc::new(move || {
            let _ = proxy.lock().unwrap().send_event(());
        }),
        false,
        start,
    );
    let mut win = Win::new(app, rx, faces);
    event_loop.run_app(&mut win).map_err(io::Error::other)?;
    win.error.map_or(Ok(()), Err)
}

/// The cell grid ratatui draws into.
struct Grid {
    buffer: Buffer,
    /// Where ratatui put the cursor of a text box, none while no box is typed into.
    cursor: Option<Position>,
    /// A cell changed since the window last showed the grid.
    changed: bool,
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
                self.changed = true;
            }
        }
        Ok(())
    }
    fn hide_cursor(&mut self) -> io::Result<()> {
        self.changed |= self.cursor.take().is_some();
        Ok(())
    }
    // Shown wherever ratatui put it, so this has nothing left to do.
    fn show_cursor(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn get_cursor_position(&mut self) -> io::Result<Position> {
        Ok(self.cursor.unwrap_or(Position::ORIGIN))
    }
    fn set_cursor_position<P: Into<Position>>(&mut self, at: P) -> io::Result<()> {
        let at = Some(at.into());
        self.changed |= self.cursor != at;
        self.cursor = at;
        Ok(())
    }
    fn clear(&mut self) -> io::Result<()> {
        self.buffer.reset();
        self.changed = true;
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

/// A face parsed on a thread of its own, taken over when first drawn with.
struct Face {
    bytes: &'static [u8],
    parsing: Option<JoinHandle<Font>>,
    font: Option<Font>,
}

impl Face {
    fn new(bytes: &'static [u8]) -> Face {
        Face {
            bytes,
            parsing: None,
            font: None,
        }
    }

    fn start(&mut self) {
        if self.parsing.is_none() && self.font.is_none() {
            let bytes = self.bytes;
            // A cell holds one glyph, so no ligature is ever drawn.
            let settings = FontSettings {
                load_substitutions: false,
                ..FontSettings::default()
            };
            self.parsing = Some(std::thread::spawn(move || {
                Font::from_bytes(bytes, settings).expect("embedded font")
            }));
        }
    }

    fn get(&mut self) -> &Font {
        self.start();
        if let Some(parsing) = self.parsing.take() {
            self.font = Some(parsing.join().expect("the font thread"));
        }
        self.font.as_ref().expect("parsed")
    }
}

/// Fira Code parses from the first moment of the start, for the first frame. The icons, which
/// the splash has none of, from `start`, so they hold no frame back.
struct Faces {
    regular: Face,
    bold: Face,
    symbols: Face,
}

impl Faces {
    fn new() -> Faces {
        let mut faces = Faces {
            regular: Face::new(REGULAR),
            bold: Face::new(BOLD),
            symbols: Face::new(SYMBOLS),
        };
        faces.regular.start();
        faces.bold.start();
        faces
    }

    fn start(&mut self) {
        self.symbols.start();
    }
}

/// The faces and the cells they are drawn in, which `rescale` sets with the window, so the
/// start never waits for a face before there is one.
struct Fonts {
    faces: Faces,
    px: f32,
    cell: (u32, u32),
    baseline: f32,
    cache: HashMap<(char, bool), (Metrics, Vec<u8>)>,
}

impl Fonts {
    fn new(faces: Faces) -> Fonts {
        Fonts {
            faces,
            px: 0.0,
            cell: (1, 1),
            baseline: 0.0,
            cache: HashMap::new(),
        }
    }

    /// The cells for this scale.
    fn rescale(&mut self, scale: f64) {
        let px = (FONT as f64 * scale) as f32;
        let regular = self.faces.regular.get();
        let width = regular.metrics('M', px).advance_width.round().max(1.0) as u32;
        let line = regular
            .horizontal_line_metrics(px)
            .expect("horizontal metrics");
        let glyph_height = line.ascent - line.descent;
        let height = (glyph_height * 1.25).round().max(1.0) as u32;
        self.px = px;
        self.cell = (width, height);
        self.baseline = (height as f32 - glyph_height) / 2.0 + line.ascent;
        self.cache.clear();
    }

    fn glyph(&mut self, c: char, bold: bool) -> &(Metrics, Vec<u8>) {
        let (faces, px, cell_width) = (&mut self.faces, self.px, self.cell.0 as f32);
        self.cache.entry((c, bold)).or_insert_with(|| {
            let regular = faces.regular.get();
            let icon = regular.lookup_glyph_index(c) == 0
                && faces.symbols.get().lookup_glyph_index(c) != 0;
            if icon {
                let symbols = faces.symbols.get();
                // An icon has two cells before its label. One wider is scaled down to them.
                let wide = symbols.metrics(c, px).width as f32;
                let fit = if wide > 2.0 * cell_width {
                    px * 2.0 * cell_width / wide
                } else {
                    px
                };
                symbols.rasterize(c, fit)
            } else if bold {
                faces.bold.get().rasterize(c, px)
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
    /// When the next tick is due.
    wake: Instant,
    /// The cursor was shown, not hidden, when the window was last painted.
    blinked: bool,
    error: Option<io::Error>,
    /// The first frame is on the screen and the reading has begun.
    started: bool,
    /// What the last two frames showed: a buffer the compositor hands back shows one of them,
    /// so only the rows that changed since are painted.
    shown: [Option<Shown>; 2],
    /// A touchpad scrolls a little at a time: what is short of a line waits for more.
    wheel: f64,
    /// A message arrived since the grid was last composed.
    dirty: bool,
}

/// A frame as painted, with what decides its pixels besides the cells.
#[derive(Clone)]
struct Shown {
    grid: Buffer,
    cursor: Option<Position>,
    /// Window width and height, and the scale as bits.
    size: (u32, u32, u64),
}

impl Win {
    fn new(app: App, rx: Receiver<Msg>, faces: Faces) -> Win {
        let terminal = Terminal::new(Grid {
            buffer: Buffer::empty(Rect::new(0, 0, 80, 24)),
            cursor: None,
            changed: true,
        })
        .expect("an in-memory grid");
        Win {
            app,
            rx,
            window: None,
            surface: None,
            terminal,
            fonts: Fonts::new(faces),
            modifiers: ModifiersState::empty(),
            pointer: PhysicalPosition::new(0.0, 0.0),
            wake: Instant::now(),
            blinked: false,
            error: None,
            started: false,
            shown: [None, None],
            wheel: 0.0,
            dirty: true,
        }
    }

    fn handle(&mut self, msg: Msg, event_loop: &ActiveEventLoop) {
        self.dirty = true;
        self.app.handle(msg);
        if self.app.done() {
            event_loop.exit();
        }
    }

    /// Draws the app into the grid, sized to the window. Returns whether a cell changed since it was
    /// last shown. Cheap, so it runs after every batch of events and the window paints only then.
    fn compose(&mut self) -> Result<bool, String> {
        let Some(window) = &self.window else {
            return Ok(false);
        };
        let size = window.inner_size();
        let (_, _, area) = layout(size.width, size.height, &self.fonts, window.scale_factor());
        if self.terminal.backend().buffer.area != area {
            self.terminal.backend_mut().buffer = Buffer::empty(area);
            self.terminal.backend_mut().changed = true;
        }
        let app = &mut self.app;
        self.terminal
            .draw(|f| app.draw(f))
            .map_err(|e| e.to_string())?;
        Ok(self.terminal.backend().changed)
    }

    fn redraw(&mut self) -> Result<(), String> {
        self.compose()?;
        let (Some(window), Some(surface)) = (&self.window, self.surface.as_mut()) else {
            return Ok(());
        };
        let size = window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return Ok(());
        };
        surface.resize(w, h).map_err(|e| e.to_string())?;
        let mut pixels = surface.buffer_mut().map_err(|e| e.to_string())?;
        let cursor = self
            .terminal
            .backend()
            .cursor
            .filter(|_| blink_on(Instant::now()));
        self.blinked = cursor.is_some();
        let scale = window.scale_factor();
        let key = (size.width, size.height, scale.to_bits());
        let shown = Shown {
            grid: self.terminal.backend().buffer.clone(),
            cursor,
            size: key,
        };
        let rows = repaint(&self.shown, pixels.age(), &shown);
        paint(
            &shown.grid,
            cursor,
            &mut self.fonts,
            &self.app.ctx.style,
            (size.width, size.height),
            scale,
            &mut pixels,
            rows.as_deref(),
        );
        let damage = rows.as_deref().map(|rows| {
            let (_, oy, _) = layout(size.width, size.height, &self.fonts, scale);
            damage(rows, oy, self.fonts.cell.1, size.width)
        });
        self.shown = [Some(shown), self.shown[0].take()];
        self.terminal.backend_mut().changed = false;
        // Lets winit hold the next redraw for the compositor's frame on Wayland.
        window.pre_present_notify();
        match damage {
            Some(damage) => pixels.present_with_damage(&damage),
            None => pixels.present(),
        }
        .map_err(|e| e.to_string())?;
        if !self.started {
            self.app.start();
            self.fonts.faces.start();
            self.started = true;
        }
        Ok(())
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
            .with_inner_size(LogicalSize::new(1080.0, 720.0));
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
                self.fonts.rescale(w.scale_factor());
                w.set_min_inner_size(Some(least(&self.fonts, w.scale_factor())));
                self.window = Some(w);
                self.surface = Some(surface);
            }
            Err(e) => {
                self.error = Some(io::Error::other(format!("no window: {e}")));
                event_loop.exit();
            }
        }
    }

    // Due by the clock rather than by the cause: a stream of events, such as the pointer
    // moving, wakes the loop before the deadline every time.
    fn new_events(&mut self, event_loop: &ActiveEventLoop, _: StartCause) {
        if Instant::now() >= self.wake {
            self.handle(Msg::Tick, event_loop);
        }
    }

    // Each message brings a wake-up of its own, so one left for the next is not lost.
    fn user_event(&mut self, event_loop: &ActiveEventLoop, (): ()) {
        let budget = Instant::now() + FRAME;
        while Instant::now() < budget
            && let Ok(msg) = self.rx.try_recv()
        {
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
            WindowEvent::Resized(_) => {
                self.handle(Msg::Resize, event_loop);
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.fonts.rescale(scale_factor);
                self.handle(Msg::Resize, event_loop);
                if let Some(w) = &self.window {
                    w.set_min_inner_size(Some(least(&self.fonts, scale_factor)));
                    w.request_redraw();
                }
            }
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                for key in self.key(&event.logical_key, event.text.as_deref()) {
                    let msg = if event.repeat {
                        Msg::Held(key)
                    } else {
                        Msg::Key(key)
                    };
                    self.handle(msg, event_loop);
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
                self.wheel += match delta {
                    MouseScrollDelta::LineDelta(_, y) => f64::from(y),
                    MouseScrollDelta::PixelDelta(p) => p.y / f64::from(self.fonts.cell.1),
                };
                let lines = self.wheel.trunc();
                self.wheel -= lines;
                let key = if lines > 0.0 {
                    Key::WheelUp
                } else {
                    Key::WheelDown
                };
                for _ in 0..lines.abs() as u32 {
                    self.handle(Msg::Key(key), event_loop);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let cursor = self.terminal.backend().cursor.is_some();
        // The pointer moving changes nothing drawn.
        let composed = match self.dirty && self.window.is_some() {
            true => {
                self.dirty = false;
                self.compose()
            }
            false => Ok(false),
        };
        match composed {
            // A cursor that is shown blinks: its beat redraws without a cell changing.
            Ok(changed) => {
                if (changed || (cursor && blink_on(now) != self.blinked))
                    && let Some(w) = &self.window
                {
                    w.request_redraw();
                }
            }
            Err(e) => crate::error!("drawing the window: {e}"),
        }
        self.wake = self.app.wake(now);
        if cursor {
            self.wake = self.wake.min(next_beat(now, BLINK));
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.wake));
    }
}

/// Whether the cursor is in the shown half of its beat.
fn blink_on(now: Instant) -> bool {
    beats(now, BLINK).is_multiple_of(2)
}

/// The smallest window: the 40 columns and 12 rows every page fits, inside the margin.
fn least(fonts: &Fonts, scale: f64) -> PhysicalSize<u32> {
    let margin = 2 * (MARGIN * scale) as u32;
    PhysicalSize::new(40 * fonts.cell.0 + margin, 12 * fonts.cell.1 + margin)
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

/// The rows to paint over a buffer that showed the frame `age` frames back, and to name to the
/// compositor: what differs from the buffer and from the last frame. The compositor takes over
/// only these from the buffer, so a row the buffer already has must still be named where the
/// screen shows another. None where all of it is painted.
fn repaint(shown: &[Option<Shown>; 2], age: u8, now: &Shown) -> Option<Vec<u16>> {
    let buffer = match age {
        1 => shown[0].as_ref(),
        2 => shown[1].as_ref(),
        _ => None,
    }?;
    let mut rows = Vec::new();
    for before in [buffer, shown[0].as_ref()?] {
        if before.size != now.size {
            return None;
        }
        rows.extend(changed_rows(
            &before.grid,
            before.cursor,
            &now.grid,
            now.cursor,
        )?);
    }
    rows.sort_unstable();
    rows.dedup();
    Some(rows)
}

/// The rows of `grid` that differ from `before`, each with the rows beside it, which a glyph
/// may reach into. None where the grid has another size and all of it is painted.
fn changed_rows(
    before: &Buffer,
    was: Option<Position>,
    grid: &Buffer,
    cursor: Option<Position>,
) -> Option<Vec<u16>> {
    if before.area != grid.area {
        return None;
    }
    let (w, h) = (grid.area.width as usize, grid.area.height);
    let mut changed: Vec<bool> = (0..h as usize)
        .map(|y| before.content[y * w..(y + 1) * w] != grid.content[y * w..(y + 1) * w])
        .collect();
    if was != cursor {
        for at in [was, cursor].into_iter().flatten() {
            if let Some(row) = changed.get_mut(at.y as usize) {
                *row = true;
            }
        }
    }
    Some(
        (0..h)
            .filter(|&y| (y.saturating_sub(1)..=(y + 1).min(h - 1)).any(|n| changed[n as usize]))
            .collect(),
    )
}

/// The rows as bands across the window, neighbours joined, for the compositor.
fn damage(rows: &[u16], oy: u32, ch: u32, width: u32) -> Vec<softbuffer::Rect> {
    let mut bands: Vec<(u32, u32)> = Vec::new();
    for &row in rows {
        match bands.last_mut() {
            Some((first, count)) if *first + *count == u32::from(row) => *count += 1,
            _ => bands.push((u32::from(row), 1)),
        }
    }
    bands
        .into_iter()
        .filter_map(|(first, count)| {
            Some(softbuffer::Rect {
                x: 0,
                y: oy + first * ch,
                width: NonZeroU32::new(width)?,
                height: NonZeroU32::new(count * ch)?,
            })
        })
        .collect()
}

/// The grid as pixels, 0RGB: all of it, or only the rows named, over what the pixels showed.
#[allow(clippy::too_many_arguments)]
fn paint(
    grid: &Buffer,
    cursor: Option<Position>,
    fonts: &mut Fonts,
    style: &crate::ui::Style,
    (width, height): (u32, u32),
    scale: f64,
    pixels: &mut [u32],
    rows: Option<&[u16]>,
) {
    let (ox, oy, _) = layout(width, height, fonts, scale);
    let (cw, ch) = fonts.cell;
    let (background, text) = (rgb(style.background, 0x2e3440), rgb(style.text, 0xd8dee9));
    let mut canvas = Canvas {
        pixels,
        width,
        height,
    };
    let all: Vec<u16>;
    let rows = match rows {
        Some(rows) => {
            for &y in rows {
                canvas.fill(0, oy + u32::from(y) * ch, width, ch, background);
            }
            rows
        }
        None => {
            canvas.pixels.fill(background);
            all = (0..grid.area.height).collect();
            &all
        }
    };
    let colors = |cell: &ratatui::buffer::Cell| {
        let (fg, bg) = (rgb(cell.fg, text), rgb(cell.bg, background));
        match cell.modifier.contains(Modifier::REVERSED) {
            true => (bg, fg),
            false => (fg, bg),
        }
    };
    // Every background first: an icon reaches into the cell after it, which must not cover it.
    for &y in rows {
        for x in 0..grid.area.width {
            let (_, bg) = colors(&grid[(x, y)]);
            if bg != background {
                canvas.fill(ox + x as u32 * cw, oy + y as u32 * ch, cw, ch, bg);
            }
        }
    }
    for &y in rows {
        for x in 0..grid.area.width {
            let cell = &grid[(x, y)];
            let (fg, bg) = colors(cell);
            let (px, py) = (ox + x as u32 * cw, oy + y as u32 * ch);
            let Some(c) = cell.symbol().chars().next().filter(|c| *c != ' ') else {
                continue;
            };
            let bold = cell.modifier.contains(Modifier::BOLD);
            if !canvas.drawn(c, (px, py, cw, ch), (fg, bg), scale) {
                let baseline = fonts.baseline;
                let (m, coverage) = fonts.glyph(c, bold);
                let gx = px as i32 + m.xmin;
                let gy = py as i32 + baseline.round() as i32 - m.height as i32 - m.ymin;
                canvas.blend((gx, gy), (m.width, m.height), coverage, (fg, bg));
            }
        }
    }
    // The cursor of a text box: a bar at the start of its cell, in the cell's text colour.
    if let Some(at) = cursor.filter(|at| grid.area.contains(*at) && rows.contains(&at.y)) {
        let (fg, _) = colors(&grid[(at.x, at.y)]);
        let thin = (scale.round() as u32).max(1);
        canvas.fill(
            ox + u32::from(at.x) * cw,
            oy + u32::from(at.y) * ch + ch / 8,
            thin.max(cw / 8),
            ch - ch / 4,
            fg,
        );
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
            '┃' => self.fill(cx - thin, y, thin * 2, h, fg),
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

    /// The grid keeps the cursor where ratatui puts it, and forgets it when hidden.
    #[test]
    fn the_grid_keeps_the_cursor_until_it_is_hidden() {
        let mut grid = Grid {
            buffer: Buffer::empty(Rect::new(0, 0, 8, 2)),
            cursor: None,
            changed: false,
        };
        grid.set_cursor_position((3, 1)).unwrap();
        assert!(grid.changed);
        assert_eq!(grid.get_cursor_position().unwrap(), Position::new(3, 1));
        grid.changed = false;
        grid.hide_cursor().unwrap();
        assert!(grid.changed && grid.cursor.is_none());
    }
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
            g.bullet,
            g.rule,
            g.path,
            g.arrow,
            g.ellipsis,
            g.separator,
            g.bar,
            g.track,
            g.scroll[0],
            g.scroll[1],
            "↑↓⏎⇞⇟-",
        ]
        .into_iter()
        .chain(logo.iter().map(String::as_str))
        .chain(g.border)
        .chain(g.spinner.iter().copied());
        for s in text {
            for c in s.chars() {
                let drawn = matches!(
                    c,
                    '─' | '━' | '│' | '┃' | '╭' | '╮' | '╰' | '╯' | '█' | '▀' | '▄'
                );
                assert!(
                    font.lookup_glyph_index(c) != 0 || drawn,
                    "Fira Code has no {c:?}"
                );
            }
        }
        // Every icon icon.rs spells, as tools/icons.sh cuts the font from it.
        let icons: Vec<char> = include_str!("ui/icon.rs")
            .split("\\u{")
            .skip(1)
            .filter_map(|rest| u32::from_str_radix(rest.split('}').next()?, 16).ok())
            .filter_map(char::from_u32)
            .collect();
        assert!(icons.len() > 30, "{icons:?}");
        for c in icons.into_iter().chain(g.mark.chars()) {
            assert!(
                symbols.lookup_glyph_index(c) != 0,
                "the symbols font has no {:x}",
                c as u32
            );
        }
    }

    /// Writes what a page looks like in the window, as PNG, from the same painter.
    pub fn png(app: &mut App, width: u32, height: u32, scale: f64, path: &std::path::Path) {
        let mut fonts = Fonts::new(Faces::new());
        fonts.rescale(scale);
        let (_, _, area) = layout(width, height, &fonts, scale);
        let mut term = Terminal::new(Grid {
            cursor: None,
            buffer: Buffer::empty(area),
            changed: true,
        })
        .unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        let mut pixels = vec![0u32; (width * height) as usize];
        paint(
            &term.backend().buffer,
            None,
            &mut fonts,
            &app.ctx.style,
            (width, height),
            scale,
            &mut pixels,
            None,
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

    /// Painted over the frame before, the rows that changed give what painting all of it does.
    #[test]
    fn painting_the_changed_rows_gives_the_whole_picture() {
        use crate::ui::Key;
        use crate::ui::app_tests::{app, example, press};
        let (mut app, _) = app();
        example(&mut app);
        let mut fonts = Fonts::new(Faces::new());
        fonts.rescale(1.0);
        let (w, h) = (900, 560);
        let (_, _, area) = layout(w, h, &fonts, 1.0);
        let frame = |app: &mut App| {
            let mut term = Terminal::new(Grid {
                cursor: None,
                buffer: Buffer::empty(area),
                changed: true,
            })
            .unwrap();
            term.draw(|f| app.draw(f)).unwrap();
            (term.backend().buffer.clone(), term.backend().cursor)
        };
        let style = app.ctx.style;
        let mut steps = vec![frame(&mut app)];
        for keys in [
            vec![Key::Down],
            vec![Key::Down, Key::Down],
            vec![Key::Char('f'), Key::Char('u')],
            vec![Key::Esc, Key::Enter],
        ] {
            press(&mut app, &keys);
            steps.push(frame(&mut app));
        }
        for pair in steps.windows(2) {
            let [(a, at), (b, bt)] = pair else {
                unreachable!()
            };
            let mut whole = vec![0u32; (w * h) as usize];
            paint(b, *bt, &mut fonts, &style, (w, h), 1.0, &mut whole, None);
            let mut over = vec![0u32; (w * h) as usize];
            paint(a, *at, &mut fonts, &style, (w, h), 1.0, &mut over, None);
            let rows = changed_rows(a, *at, b, *bt).unwrap();
            assert!(!rows.is_empty());
            paint(
                b,
                *bt,
                &mut fonts,
                &style,
                (w, h),
                1.0,
                &mut over,
                Some(&rows),
            );
            assert!(whole == over, "rows {rows:?}");
        }
        assert_eq!(
            damage(&[2, 3, 4, 9], 10, 20, 900)
                .iter()
                .map(|r| (r.y, r.height.get()))
                .collect::<Vec<_>>(),
            [(50, 60), (190, 20)]
        );
    }

    /// The compositor takes over only the damage, from two buffers in turn as softbuffer swaps
    /// them on Wayland. It shows every frame whole: a page left and opened again, a blinking cursor.
    #[test]
    fn the_compositor_shows_every_frame_whole() {
        use crate::ui::Key;
        use crate::ui::Target;
        use crate::ui::app_tests::{app, example, open, press};
        let (mut app, _) = app();
        example(&mut app);
        let area = Rect::new(0, 0, 60, 20);
        let frame = |app: &mut App| {
            let mut term = Terminal::new(Grid {
                cursor: None,
                buffer: Buffer::empty(area),
                changed: true,
            })
            .unwrap();
            term.draw(|f| app.draw(f)).unwrap();
            Shown {
                grid: term.backend().buffer.clone(),
                cursor: term.backend().cursor,
                size: (1, 1, 0),
            }
        };
        let home = frame(&mut app);
        for page in Target::Settings.open(&mut app.ctx) {
            open(&mut app, page);
        }
        let settings = frame(&mut app);
        press(&mut app, &[Key::Esc, Key::Char('f')]);
        let typing = frame(&mut app);
        assert!(typing.cursor.is_some());
        let blinked = Shown {
            cursor: None,
            ..typing.clone()
        };
        let frames = [
            &home, &settings, &home, &settings, &home, &typing, &blinked, &typing, &blinked,
            &settings,
        ];
        let picture = |s: &Shown| -> Vec<(Vec<Cell>, Option<u16>)> {
            let w = usize::from(area.width);
            (0..area.height)
                .map(|y| {
                    let at = usize::from(y) * w;
                    let cursor = s.cursor.filter(|c| c.y == y).map(|c| c.x);
                    (s.grid.content[at..at + w].to_vec(), cursor)
                })
                .collect()
        };
        let blank = picture(&Shown {
            grid: Buffer::empty(area),
            cursor: None,
            size: (1, 1, 0),
        });
        let (mut front, mut back) = ((blank.clone(), 0u8), (blank.clone(), 0u8));
        let mut screen = blank;
        let mut shown: [Option<Shown>; 2] = [None, None];
        for (i, now) in frames.into_iter().enumerate() {
            let rows = repaint(&shown, back.1, now).unwrap_or_else(|| (0..area.height).collect());
            let whole = picture(now);
            for &y in &rows {
                back.0[usize::from(y)] = whole[usize::from(y)].clone();
            }
            for &y in &rows {
                screen[usize::from(y)] = back.0[usize::from(y)].clone();
            }
            assert!(screen == whole, "frame {i} shows rows of another");
            std::mem::swap(&mut front, &mut back);
            front.1 = 1;
            if back.1 != 0 {
                back.1 += 1;
            }
            shown = [Some(now.clone()), shown[0].take()];
        }
    }

    #[test]
    fn the_grid_fills_the_window_inside_the_margin() {
        let mut fonts = Fonts::new(Faces::new());
        fonts.rescale(1.0);
        assert!(fonts.cell.0 >= 7 && fonts.cell.1 > fonts.cell.0);
        for scale in [1.0, 1.25, 1.5, 2.0] {
            fonts.rescale(scale);
            let size = least(&fonts, scale);
            let (_, _, area) = layout(size.width, size.height, &fonts, scale);
            assert_eq!((area.width, area.height), (40, 12), "at {scale}");
        }
    }
}
