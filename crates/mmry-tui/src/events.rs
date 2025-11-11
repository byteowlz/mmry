use anyhow::Result;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use crossterm::event::{self};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub enum AppEvent {
    Key(KeyEvent),
    Resize(u16, u16),
    Tick,
}

pub struct EventHandler {
    tick_rate: Duration,
}

impl EventHandler {
    pub fn new() -> Self {
        Self {
            tick_rate: Duration::from_millis(250),
        }
    }

    pub fn next(&mut self) -> Result<Option<AppEvent>> {
        if event::poll(self.tick_rate)? {
            match event::read()? {
                Event::Key(key) => Ok(Some(AppEvent::Key(key))),
                Event::Resize(w, h) => Ok(Some(AppEvent::Resize(w, h))),
                _ => Ok(None),
            }
        } else {
            Ok(Some(AppEvent::Tick))
        }
    }
}

pub fn parse_key_event(key: KeyEvent) -> KeyAction {
    match (key.code, key.modifiers) {
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => KeyAction::Quit,
        (KeyCode::Esc, _) => KeyAction::Escape,
        (KeyCode::Enter, _) => KeyAction::Select,
        (KeyCode::Backspace, _) => KeyAction::Backspace,
        (KeyCode::Char(' '), KeyModifiers::NONE) => KeyAction::ToggleSelect,

        (KeyCode::Char('q'), KeyModifiers::NONE) => KeyAction::Char('q'),
        (KeyCode::Char('?'), KeyModifiers::SHIFT) => KeyAction::Char('?'),

        (KeyCode::Char('j'), KeyModifiers::NONE) => KeyAction::Char('j'),
        (KeyCode::Char('k'), KeyModifiers::NONE) => KeyAction::Char('k'),
        (KeyCode::Char('h'), KeyModifiers::NONE) => KeyAction::Char('h'),
        (KeyCode::Char('l'), KeyModifiers::NONE) => KeyAction::Char('l'),
        (KeyCode::Down, _) => KeyAction::Down,
        (KeyCode::Up, _) => KeyAction::Up,
        (KeyCode::Left, _) => KeyAction::Left,
        (KeyCode::Right, _) => KeyAction::Right,

        (KeyCode::Char('g'), KeyModifiers::NONE) => KeyAction::Char('g'),
        (KeyCode::Char('G'), KeyModifiers::SHIFT) => KeyAction::Char('G'),
        (KeyCode::Char('V'), KeyModifiers::SHIFT) => KeyAction::Char('V'),

        (KeyCode::Char('a'), KeyModifiers::CONTROL) => KeyAction::SelectAll,

        (KeyCode::Char('d'), KeyModifiers::CONTROL) => KeyAction::PageDown,
        (KeyCode::Char('u'), KeyModifiers::CONTROL) => KeyAction::PageUp,
        (KeyCode::Tab, _) => KeyAction::CycleSearchMode,

        (KeyCode::Char(c), _) => KeyAction::Char(c),

        _ => KeyAction::Noop,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum KeyAction {
    Quit,
    Up,
    Down,
    Left,
    Right,
    PageDown,
    PageUp,
    Select,
    Escape,
    Backspace,
    Char(char),
    ToggleSelect,
    SelectAll,
    CycleSearchMode,
    Noop,
}
