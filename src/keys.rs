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
    OrderBy,
    FilterBy,
    Refresh,
    Tab,
    NewThread,
    OpenIn,
    Projects,
}

pub fn map_key(key: KeyEvent) -> Option<Action> {
    match (key.modifiers, key.code) {
        (KeyModifiers::CONTROL, KeyCode::Char('c')) => Some(Action::Quit),
        (KeyModifiers::NONE, KeyCode::Char('q')) => Some(Action::Quit),
        (_, KeyCode::Char('j')) | (_, KeyCode::Down) => Some(Action::MoveDown),
        (_, KeyCode::Char('k')) | (_, KeyCode::Up) => Some(Action::MoveUp),
        (_, KeyCode::Char('g')) => Some(Action::Top),
        (KeyModifiers::SHIFT, KeyCode::Char('G')) => Some(Action::Bottom),
        (_, KeyCode::Enter) => Some(Action::Select),
        (_, KeyCode::Esc) => Some(Action::Back),
        (_, KeyCode::Char('?')) => Some(Action::Help),
        (_, KeyCode::Char('/')) => Some(Action::Search),
        (_, KeyCode::Char('s')) => Some(Action::OrderBy),
        (_, KeyCode::Char('f')) => Some(Action::FilterBy),
        (_, KeyCode::Char('r')) => Some(Action::Refresh),
        (_, KeyCode::Tab) => Some(Action::Tab),
        (_, KeyCode::Char('a')) => Some(Action::NewThread),
        (_, KeyCode::Char('o')) => Some(Action::OpenIn),
        (_, KeyCode::Char('p')) => Some(Action::Projects),
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
    fn refresh_on_r() {
        assert!(matches!(map_key(key(KeyCode::Char('r'))), Some(Action::Refresh)));
    }

    #[test]
    fn tab_maps_to_tab() {
        assert!(matches!(map_key(key(KeyCode::Tab)), Some(Action::Tab)));
    }

    #[test]
    fn new_thread_on_a() {
        assert!(matches!(map_key(key(KeyCode::Char('a'))), Some(Action::NewThread)));
    }

    #[test]
    fn open_in_on_o() {
        assert!(matches!(map_key(key(KeyCode::Char('o'))), Some(Action::OpenIn)));
    }

    #[test]
    fn projects_on_p() {
        assert!(matches!(map_key(key(KeyCode::Char('p'))), Some(Action::Projects)));
    }

    #[test]
    fn unmapped_keys_return_none() {
        assert!(map_key(key(KeyCode::Char('x'))).is_none());
        assert!(map_key(key(KeyCode::F(1))).is_none());
    }
}
