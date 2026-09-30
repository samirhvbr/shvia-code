//! The approval menu: a short list picked with the arrow keys, a number or a letter.
//!
//! Two halves. [`Menu`] is the state machine — keys in, a decision out — pure, so every key is
//! tested without a terminal. [`choose`] draws it on a real one, in raw mode, and turns crossterm's
//! events into [`Key`]s. Where there is no terminal on both ends (a pipe, `TERM=dumb`, the tests)
//! it answers [`Answer::Unavailable`] and the caller asks the old way, with a line on stdin.
//!
//! Nothing here decides what is safe: the default cursor and the labels come from the caller, and
//! a cancelled menu (Esc, Ctrl-C, a read error) is a "no" — never a "yes".

use crate::term;
use crossterm::cursor::MoveUp;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{self, Clear, ClearType};
use crossterm::queue;
use std::io::{self, Write};

pub struct Item<'a> {
    pub label: &'a str,
    /// Letters that pick this item directly (case-insensitive), besides its number.
    pub hotkeys: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Enter,
    Cancel,
    Char(char),
}

#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Stay,
    Picked(usize),
    Cancelled,
}

pub struct Menu {
    hotkeys: Vec<Vec<char>>,
    cursor: usize,
}

impl Menu {
    pub fn new(items: &[Item], default: usize) -> Self {
        let hotkeys = items.iter().map(|i| i.hotkeys.to_lowercase().chars().collect()).collect();
        Self { hotkeys, cursor: default.min(items.len().saturating_sub(1)) }
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn press(&mut self, key: Key) -> Step {
        let n = self.hotkeys.len();
        if n == 0 {
            return Step::Cancelled;
        }
        match key {
            Key::Up => self.cursor = (self.cursor + n - 1) % n,
            Key::Down => self.cursor = (self.cursor + 1) % n,
            Key::Enter => return Step::Picked(self.cursor),
            Key::Cancel => return Step::Cancelled,
            Key::Char(c) => {
                let c = c.to_ascii_lowercase();
                if let Some(d) = c.to_digit(10).map(|d| d as usize).filter(|d| (1..=n).contains(d)) {
                    return Step::Picked(d - 1);
                }
                if let Some(i) = self.hotkeys.iter().position(|h| h.contains(&c)) {
                    return Step::Picked(i);
                }
                match c {
                    'k' => self.cursor = (self.cursor + n - 1) % n,
                    'j' => self.cursor = (self.cursor + 1) % n,
                    'q' => return Step::Cancelled,
                    _ => {}
                }
            }
        }
        Step::Stay
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Answer {
    Picked(usize),
    /// Esc, Ctrl-C or a terminal error: the caller must treat it as a refusal.
    Cancelled,
    /// No interactive terminal: ask with a line on stdin instead.
    Unavailable,
}

/// Restores the terminal even if drawing panics or returns early.
struct RawGuard;

impl RawGuard {
    fn enable() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
}

/// Shows `question` and the items, and waits for a pick. The question stays in the scrollback;
/// the items are erased once answered, so the caller can leave one line saying what was chosen.
pub fn choose(question: &str, items: &[Item], default: usize) -> Answer {
    if !term::caps().interactive() || items.is_empty() {
        return Answer::Unavailable;
    }
    let Ok(guard) = RawGuard::enable() else {
        return Answer::Unavailable;
    };
    let answer = run(question, items, default).unwrap_or(Answer::Cancelled);
    drop(guard);
    answer
}

fn run(question: &str, items: &[Item], default: usize) -> io::Result<Answer> {
    let mut out = io::stdout();
    let mut menu = Menu::new(items, default);
    let n = items.len() as u16;
    // Raw mode turns off the terminal's newline translation: every line ends in \r\n.
    write!(out, "{}\r\n", term::bold(&format!("  {question}")))?;
    draw(&mut out, items, menu.cursor())?;
    loop {
        let key = match event::read()? {
            Event::Key(k) if k.kind == KeyEventKind::Press => {
                let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
                match k.code {
                    KeyCode::Char('c' | 'd') if ctrl => Key::Cancel,
                    KeyCode::Char(c) if !ctrl => Key::Char(c),
                    KeyCode::Up => Key::Up,
                    KeyCode::Down | KeyCode::Tab => Key::Down,
                    KeyCode::Enter => Key::Enter,
                    KeyCode::Esc => Key::Cancel,
                    _ => continue,
                }
            }
            Event::Resize(..) => {
                erase(&mut out, n)?;
                draw(&mut out, items, menu.cursor())?;
                continue;
            }
            _ => continue,
        };
        let step = menu.press(key);
        erase(&mut out, n)?;
        match step {
            Step::Stay => draw(&mut out, items, menu.cursor())?,
            Step::Picked(i) => return Ok(Answer::Picked(i)),
            Step::Cancelled => return Ok(Answer::Cancelled),
        }
    }
}

fn draw(out: &mut impl Write, items: &[Item], cursor: usize) -> io::Result<()> {
    for (i, item) in items.iter().enumerate() {
        if i == cursor {
            write!(out, "  {} {}. {}\r\n", term::accent("❯"), i + 1, term::bold(item.label))?;
        } else {
            write!(out, "    {}. {}\r\n", i + 1, item.label)?;
        }
    }
    out.flush()
}

fn erase(out: &mut impl Write, lines: u16) -> io::Result<()> {
    queue!(out, MoveUp(lines), Clear(ClearType::FromCursorDown))?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items() -> Vec<Item<'static>> {
        vec![
            Item { label: "Sim", hotkeys: "sy" },
            Item { label: "Sim, e não perguntar de novo", hotkeys: "t" },
            Item { label: "Não", hotkeys: "n" },
        ]
    }

    #[test]
    fn enter_picks_where_the_cursor_is() {
        let mut m = Menu::new(&items(), 0);
        assert_eq!(m.press(Key::Enter), Step::Picked(0));
        let mut m = Menu::new(&items(), 2);
        assert_eq!(m.press(Key::Enter), Step::Picked(2), "the default can be the safe option");
    }

    #[test]
    fn arrows_move_and_wrap() {
        let mut m = Menu::new(&items(), 0);
        assert_eq!(m.press(Key::Up), Step::Stay);
        assert_eq!(m.cursor(), 2, "up from the first wraps to the last");
        assert_eq!(m.press(Key::Down), Step::Stay);
        assert_eq!(m.cursor(), 0);
        m.press(Key::Down);
        assert_eq!(m.press(Key::Enter), Step::Picked(1));
    }

    #[test]
    fn a_number_picks_at_once() {
        let mut m = Menu::new(&items(), 0);
        assert_eq!(m.press(Key::Char('3')), Step::Picked(2));
        assert_eq!(m.press(Key::Char('1')), Step::Picked(0));
        assert_eq!(m.press(Key::Char('4')), Step::Stay, "there is no fourth item");
        assert_eq!(m.press(Key::Char('0')), Step::Stay);
    }

    #[test]
    fn letters_pick_case_insensitively() {
        let mut m = Menu::new(&items(), 0);
        assert_eq!(m.press(Key::Char('s')), Step::Picked(0));
        assert_eq!(m.press(Key::Char('Y')), Step::Picked(0));
        assert_eq!(m.press(Key::Char('T')), Step::Picked(1));
        assert_eq!(m.press(Key::Char('n')), Step::Picked(2));
    }

    #[test]
    fn escape_and_q_refuse() {
        let mut m = Menu::new(&items(), 0);
        assert_eq!(m.press(Key::Cancel), Step::Cancelled);
        assert_eq!(m.press(Key::Char('q')), Step::Cancelled);
    }

    #[test]
    fn vi_keys_move_but_never_pick() {
        let mut m = Menu::new(&items(), 0);
        assert_eq!(m.press(Key::Char('j')), Step::Stay);
        assert_eq!(m.cursor(), 1);
        assert_eq!(m.press(Key::Char('k')), Step::Stay);
        assert_eq!(m.cursor(), 0);
        assert_eq!(m.press(Key::Char('x')), Step::Stay, "an unknown key does nothing");
    }

    #[test]
    fn a_hotkey_wins_over_a_vi_key() {
        let mut m = Menu::new(&[Item { label: "Kill", hotkeys: "k" }, Item { label: "Não", hotkeys: "n" }], 1);
        assert_eq!(m.press(Key::Char('k')), Step::Picked(0));
    }

    #[test]
    fn a_default_past_the_end_is_clamped() {
        assert_eq!(Menu::new(&items(), 99).cursor(), 2);
    }

    #[test]
    fn an_empty_menu_can_only_be_refused() {
        assert_eq!(Menu::new(&[], 0).press(Key::Enter), Step::Cancelled);
    }

    #[test]
    fn without_a_terminal_the_menu_steps_aside() {
        // Under `cargo test` stdin/stdout are not a terminal.
        assert_eq!(choose("Aplicar?", &items(), 0), Answer::Unavailable);
    }
}
