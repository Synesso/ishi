use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub enum Action {
    Quit,
    MoveDown,
    MoveUp,
    Top,
    Bottom,
    Select,
    Back,
    Help,
    Search,
}

pub fn map_key(key: KeyEvent) -> Option<Action> {
    match (key.modifiers, key.code) {
        (KeyModifiers::CONTROL, KeyCode::Char('c')) => Some(Action::Quit),
        (_, KeyCode::Char('q')) => Some(Action::Quit),
        (_, KeyCode::Char('j')) | (_, KeyCode::Down) => Some(Action::MoveDown),
        (_, KeyCode::Char('k')) | (_, KeyCode::Up) => Some(Action::MoveUp),
        (_, KeyCode::Char('g')) => Some(Action::Top),
        (KeyModifiers::SHIFT, KeyCode::Char('G')) => Some(Action::Bottom),
        (_, KeyCode::Enter) => Some(Action::Select),
        (_, KeyCode::Esc) => Some(Action::Back),
        (_, KeyCode::Char('?')) => Some(Action::Help),
        (_, KeyCode::Char('/')) => Some(Action::Search),
        _ => None,
    }
}
