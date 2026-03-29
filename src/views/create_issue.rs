use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::app::{CreateIssueField, CreateIssueForm, IssuePriority};

pub fn render(frame: &mut Frame, area: Rect, form: &CreateIssueForm) {
    let modal_width = 60u16.min(area.width.saturating_sub(4));
    let modal_height = 28u16.min(area.height.saturating_sub(4));

    let x = area.x + (area.width.saturating_sub(modal_width)) / 2;
    let y = area.y + (area.height.saturating_sub(modal_height)) / 2;
    let popup_area = Rect::new(x, y, modal_width, modal_height);

    frame.render_widget(Clear, popup_area);

    let outer = Block::default()
        .borders(Borders::ALL)
        .title("Create Issue")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = outer.inner(popup_area);
    frame.render_widget(outer, popup_area);

    let picker_rows = 3u16;

    let chunks = Layout::vertical([
        Constraint::Length(3),               // Title
        Constraint::Length(picker_rows + 2), // Team
        Constraint::Length(picker_rows + 2), // Project
        Constraint::Length(3),               // Priority
        Constraint::Length(1),               // Assign to me
        Constraint::Min(3),                  // Description
        Constraint::Length(1),               // Submit button
        Constraint::Length(1),               // Hints
    ])
    .split(inner);

    // Title
    render_text_field(
        frame,
        chunks[0],
        "Title",
        &form.title,
        form.focus == CreateIssueField::Title,
    );

    // Team picker
    render_picker(
        frame,
        chunks[1],
        "Team",
        &form.filtered_team_options(),
        form.team_selected,
        &form.team_type_ahead,
        form.selected_team_name(),
        form.focus == CreateIssueField::Team,
        picker_rows as usize,
    );

    // Project picker
    render_picker(
        frame,
        chunks[2],
        "Project",
        &form.filtered_project_options(),
        form.project_selected,
        &form.project_type_ahead,
        form.selected_project_name(),
        form.focus == CreateIssueField::Project,
        picker_rows as usize,
    );

    // Priority
    render_priority_picker(frame, chunks[3], form);

    // Assign to me
    let assign_focused = form.focus == CreateIssueField::AssignToMe;
    let check = if form.assign_to_me { "✓" } else { " " };
    let assign_style = if assign_focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let assign_line = Line::from(vec![
        Span::styled(format!(" [{}] ", check), assign_style),
        Span::styled("Assign to me", assign_style),
    ]);
    frame.render_widget(Paragraph::new(assign_line), chunks[4]);

    // Description
    render_text_field(
        frame,
        chunks[5],
        "Description",
        &form.description,
        form.focus == CreateIssueField::Description,
    );

    // Submit button
    let submit_focused = form.focus == CreateIssueField::Submit;
    let submit_style = if submit_focused {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let submit_label = if submit_focused {
        " ▸ Create Issue ◂ "
    } else {
        "   Create Issue   "
    };
    let submit = Line::from(Span::styled(submit_label, submit_style));
    frame.render_widget(Paragraph::new(submit), chunks[6]);

    // Hints bar
    let key_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let hints = Line::from(vec![
        Span::styled("Tab", key_style),
        Span::raw(" next  "),
        Span::styled("S-Tab", key_style),
        Span::raw(" prev  "),
        Span::styled("Esc", key_style),
        Span::raw(" cancel"),
    ]);
    frame.render_widget(Paragraph::new(hints), chunks[7]);
}

fn render_text_field(frame: &mut Frame, area: Rect, label: &str, value: &str, focused: bool) {
    let border_style = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let mut lines: Vec<Line> = value.lines().map(|l| Line::raw(l.to_string())).collect();
    if value.is_empty() || value.ends_with('\n') {
        lines.push(Line::raw(""));
    }
    if focused {
        if let Some(last) = lines.last_mut() {
            let mut spans = last.spans.clone();
            spans.push(Span::raw("▏"));
            *last = Line::from(spans);
        }
    }

    let inner_height = area.height.saturating_sub(2);
    let line_count = lines.len() as u16;
    let scroll = line_count.saturating_sub(inner_height);

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(label)
                .border_style(border_style),
        )
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(paragraph, area);
}

/// Render a generic picker field with type-ahead support.
#[allow(clippy::too_many_arguments)]
fn render_picker(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    filtered: &[(usize, &(String, String))],
    selected_idx: usize,
    type_ahead: &str,
    selected_name: &str,
    focused: bool,
    visible_rows: usize,
) {
    let border_style = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let total = filtered.len();
    let selected_pos = filtered
        .iter()
        .position(|(idx, _)| *idx == selected_idx)
        .unwrap_or(0);

    let start = if total <= visible_rows {
        0
    } else if selected_pos < visible_rows / 2 {
        0
    } else if selected_pos >= total - visible_rows / 2 {
        total - visible_rows
    } else {
        selected_pos - visible_rows / 2
    };
    let end = (start + visible_rows).min(total);

    let lines: Vec<Line> = filtered[start..end]
        .iter()
        .map(|(idx, (_, name))| {
            let style = if *idx == selected_idx {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            Line::from(Span::styled(format!(" {} ", name), style))
        })
        .collect();

    let title = if type_ahead.is_empty() {
        format!("{} ({})", label, selected_name)
    } else {
        format!("{} [{}] ({})", label, type_ahead, selected_name)
    };
    let paragraph = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(border_style),
    );
    frame.render_widget(paragraph, area);
}

fn render_priority_picker(frame: &mut Frame, area: Rect, form: &CreateIssueForm) {
    let focused = form.focus == CreateIssueField::Priority;
    let border_style = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let spans: Vec<Span> = IssuePriority::ALL
        .iter()
        .enumerate()
        .flat_map(|(i, &p)| {
            let style = if p == form.priority {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let mut v = vec![Span::styled(p.label(), style)];
            if i < IssuePriority::ALL.len() - 1 {
                v.push(Span::raw("  "));
            }
            v
        })
        .collect();

    let paragraph = Paragraph::new(Line::from(spans)).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Priority")
            .border_style(border_style),
    );
    frame.render_widget(paragraph, area);
}
