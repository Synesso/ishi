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

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn key_with(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn quit_on_q() {
        assert!(matches!(map_key(key(KeyCode::Char('q'))), Some(Action::Quit)));
    }

    #[test]
    fn quit_on_ctrl_c() {
        assert!(matches!(
            map_key(key_with(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(Action::Quit)
        ));
    }

    #[test]
    fn navigation_keys() {
        assert!(matches!(map_key(key(KeyCode::Char('j'))), Some(Action::MoveDown)));
        assert!(matches!(map_key(key(KeyCode::Down)), Some(Action::MoveDown)));
        assert!(matches!(map_key(key(KeyCode::Char('k'))), Some(Action::MoveUp)));
        assert!(matches!(map_key(key(KeyCode::Up)), Some(Action::MoveUp)));
        assert!(matches!(map_key(key(KeyCode::Char('g'))), Some(Action::Top)));
        assert!(matches!(
            map_key(key_with(KeyCode::Char('G'), KeyModifiers::SHIFT)),
            Some(Action::Bottom)
        ));
    }

    #[test]
    fn action_keys() {
        assert!(matches!(map_key(key(KeyCode::Enter)), Some(Action::Select)));
        assert!(matches!(map_key(key(KeyCode::Esc)), Some(Action::Back)));
        assert!(matches!(map_key(key(KeyCode::Char('?'))), Some(Action::Help)));
        assert!(matches!(map_key(key(KeyCode::Char('/'))), Some(Action::Search)));
    }

    #[test]
    fn unmapped_keys_return_none() {
        assert!(map_key(key(KeyCode::Char('x'))).is_none());
        assert!(map_key(key(KeyCode::Tab)).is_none());
        assert!(map_key(key(KeyCode::F(1))).is_none());
    }
}
