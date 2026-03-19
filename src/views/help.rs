use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

/// All keybindings shown in the help overlay.
/// Each entry is (key_label, description).
pub const KEYBINDINGS: &[(&str, &str)] = &[
    ("?", "Toggle this help"),
    ("q", "Quit (press twice)"),
    ("Ctrl+c", "Quit immediately"),
    ("j / ↓", "Move down / scroll down"),
    ("k / ↑", "Move up / scroll up"),
    ("g", "Go to top"),
    ("G", "Go to bottom"),
    ("Enter", "Select / confirm"),
    (
        "l",
        "Open selected thread's latest run log (detail threads)",
    ),
    (
        "x",
        "Mark selected thread's latest run stale (detail threads)",
    ),
    ("Esc", "Back / dismiss"),
    ("/", "Search"),
    ("s", "Sort by column"),
    ("f", "Filter by column"),
    ("r", "Refresh"),
    ("o", "Open in Linear / GitHub"),
    ("Tab", "Switch section (detail)"),
    ("a", "New Amp thread (detail)"),
    ("p", "Projects"),
    ("m", "Change issue state"),
];

pub fn render(frame: &mut Frame, area: Rect) {
    let key_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let desc_style = Style::default().fg(Color::White);

    let lines: Vec<Line> = KEYBINDINGS
        .iter()
        .map(|(key, desc)| {
            Line::from(vec![
                Span::styled(format!("  {:<12}", key), key_style),
                Span::styled(*desc, desc_style),
            ])
        })
        .collect();

    let content_height = (lines.len() as u16) + 2; // +2 for borders
    let content_width = 40_u16;

    let popup_height = content_height.min(area.height.saturating_sub(4));
    let popup_width = content_width.min(area.width.saturating_sub(4));

    let x = area.x + (area.width.saturating_sub(popup_width)) / 2;
    let y = area.y + (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(x, y, popup_width, popup_height);

    frame.render_widget(Clear, popup_area);

    let popup = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Help — Keybindings")
            .border_style(Style::default().fg(Color::Cyan)),
    );
    frame.render_widget(popup, popup_area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keybindings_is_non_empty() {
        assert!(!KEYBINDINGS.is_empty());
    }

    #[test]
    fn keybindings_contains_help_toggle() {
        assert!(KEYBINDINGS.iter().any(|(key, _)| *key == "?"));
    }

    #[test]
    fn keybindings_contains_quit() {
        assert!(KEYBINDINGS.iter().any(|(key, _)| *key == "q"));
    }

    #[test]
    fn keybindings_have_descriptions() {
        for (key, desc) in KEYBINDINGS {
            assert!(!key.is_empty(), "key should not be empty");
            assert!(
                !desc.is_empty(),
                "description should not be empty for key '{}'",
                key
            );
        }
    }

    #[test]
    fn render_does_not_panic() {
        use ratatui::{Terminal, backend::TestBackend};
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render(frame, frame.area());
            })
            .unwrap();
    }

    #[test]
    fn render_handles_small_area() {
        use ratatui::{Terminal, backend::TestBackend};
        let backend = TestBackend::new(10, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render(frame, frame.area());
            })
            .unwrap();
    }
}
