use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

pub fn render(frame: &mut Frame, area: Rect, input: &str, loading: bool) {
    let modal_width = 60u16.min(area.width.saturating_sub(4));
    let modal_height = 10u16.min(area.height.saturating_sub(4));

    let x = area.x + (area.width.saturating_sub(modal_width)) / 2;
    let y = area.y + (area.height.saturating_sub(modal_height)) / 2;
    let popup_area = Rect::new(x, y, modal_width, modal_height);

    frame.render_widget(Clear, popup_area);

    let title = if loading {
        "Quick Create — extracting …"
    } else {
        "Quick Create"
    };
    let outer = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::Magenta));
    let inner = outer.inner(popup_area);
    frame.render_widget(outer, popup_area);

    let inner_height = inner.height.saturating_sub(1); // reserve 1 for hints

    let mut lines: Vec<Line> = input.lines().map(|l| Line::raw(l.to_string())).collect();
    if input.is_empty() || input.ends_with('\n') {
        lines.push(Line::raw(""));
    }
    if !loading {
        if let Some(last) = lines.last_mut() {
            let mut spans = last.spans.clone();
            spans.push(Span::raw("▏"));
            *last = Line::from(spans);
        }
    }

    let line_count = lines.len() as u16;
    let scroll = line_count.saturating_sub(inner_height);

    let text_area = Rect::new(inner.x, inner.y, inner.width, inner_height);
    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(paragraph, text_area);

    // Hints bar
    let key_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let hints = if loading {
        Line::from(Span::styled(
            "  Analysing with AI …",
            Style::default().fg(Color::DarkGray),
        ))
    } else {
        Line::from(vec![
            Span::styled("Enter", key_style),
            Span::raw(" submit  "),
            Span::styled("Esc", key_style),
            Span::raw(" cancel"),
        ])
    };
    let hints_area = Rect::new(inner.x, inner.y + inner_height, inner.width, 1);
    frame.render_widget(Paragraph::new(hints), hints_area);
}
