use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::api::client::LinearApi;
use crate::app::{App, DetailSection};

pub fn render<A: LinearApi>(frame: &mut Frame, area: Rect, app: &mut App<A>) {
    let issue = match app.selected_issue() {
        Some(i) => i,
        None => {
            frame.render_widget(Paragraph::new("No issue selected"), area);
            return;
        }
    };

    // Metadata fields
    let label_style = Style::default().fg(Color::DarkGray);

    let mut meta_lines: Vec<Line> = Vec::new();

    meta_lines.push(Line::from(vec![
        Span::styled("Status:   ", label_style),
        Span::styled(issue.status_str(), status_style(issue.status_str())),
    ]));

    meta_lines.push(Line::from(vec![
        Span::styled("Priority: ", label_style),
        Span::styled(issue.priority_str(), priority_style(issue.priority_str())),
    ]));

    meta_lines.push(Line::from(vec![
        Span::styled("Project:  ", label_style),
        Span::raw(issue.project_str()),
    ]));

    let assignee = issue
        .assignee
        .as_ref()
        .map_or("—", |a| a.name.as_str());
    meta_lines.push(Line::from(vec![
        Span::styled("Assignee: ", label_style),
        Span::raw(assignee),
    ]));

    let labels_str = issue
        .labels
        .as_ref()
        .map(|l| {
            l.nodes
                .iter()
                .map(|l| l.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    if !labels_str.is_empty() {
        meta_lines.push(Line::from(vec![
            Span::styled("Labels:   ", label_style),
            Span::raw(labels_str),
        ]));
    }

    // +2 for the border top/bottom
    let meta_height = (meta_lines.len() as u16) + 2;

    // Calculate thread section height
    let thread_count = app.detail_threads.len();
    let threads_height = if thread_count > 0 {
        // header border (1) + each thread line (1 each) + bottom border (1)
        (thread_count as u16) + 2
    } else {
        0
    };

    let chunks = Layout::vertical([
        Constraint::Length(meta_height),
        Constraint::Min(0),
        Constraint::Length(threads_height),
        Constraint::Length(1),
    ])
    .split(area);

    // Metadata box
    let title = Line::from(vec![
        Span::styled(
            issue.identifier.to_string(),
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(" — {}", issue.title)),
    ]);
    let meta = Paragraph::new(meta_lines)
        .block(Block::default().borders(Borders::ALL).title(title));
    frame.render_widget(meta, chunks[0]);

    // Description / Comments
    let mut body_lines: Vec<Line> = Vec::new();

    if let Some(desc) = &issue.description {
        for line in desc.lines() {
            body_lines.push(Line::raw(line.to_string()));
        }
    }

    // Comments
    if let Some(comments) = &issue.comments
        && !comments.nodes.is_empty()
    {
        if !body_lines.is_empty() {
            body_lines.push(Line::raw(""));
        }
        body_lines.push(Line::from(Span::styled(
            format!("Comments ({})", comments.nodes.len()),
            Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )));
        body_lines.push(Line::raw(""));

        for comment in &comments.nodes {
            let author = comment
                .user
                .as_ref()
                .map_or("Unknown", |u| u.name.as_str());
            body_lines.push(Line::from(vec![
                Span::styled(
                    author,
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("  {}", &comment.created_at[..10]),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
            for line in comment.body.lines() {
                body_lines.push(Line::raw(format!("  {}", line)));
            }
            body_lines.push(Line::raw(""));
        }
    }

    let content_lines = body_lines.len() as u16;
    let inner_height = chunks[1].height.saturating_sub(2); // borders
    let scroll = app.detail_scroll;

    let body_focused = app.detail_section == DetailSection::Body;
    let body_border_style = if body_focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let detail = Paragraph::new(body_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Description")
                .border_style(body_border_style),
        )
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));

    frame.render_widget(detail, chunks[1]);

    app.detail_scroll_max = content_lines.saturating_sub(inner_height);

    // Threads section
    if thread_count > 0 {
        let threads_focused = app.detail_section == DetailSection::Threads;
        let threads_border_style = if threads_focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let thread_lines: Vec<Line> = app
            .detail_threads
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let is_selected = threads_focused && i == app.detail_thread_selected;
                let style = if is_selected {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };
                let time_str = t.relative_time();
                Line::from(vec![
                    Span::styled(
                        format!("  {} ", t.title),
                        style,
                    ),
                    Span::styled(
                        format!("({} msgs) ", t.message_count),
                        style.fg(Color::DarkGray),
                    ),
                    Span::styled(
                        time_str,
                        style.fg(Color::DarkGray),
                    ),
                ])
            })
            .collect();

        let threads_block = Paragraph::new(thread_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Threads ({})", thread_count))
                .border_style(threads_border_style),
        );
        frame.render_widget(threads_block, chunks[2]);
    }

    // Status bar
    let key_style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let bar = if app.awaiting_quit {
        Line::from(vec![
            Span::raw("Press "),
            Span::styled("q", key_style),
            Span::raw(" again to quit"),
        ])
    } else if app.detail_section == DetailSection::Threads {
        Line::from(vec![
            Span::styled("Esc", key_style),
            Span::raw(" back  "),
            Span::styled("j", key_style),
            Span::raw("/"),
            Span::styled("k", key_style),
            Span::raw(" navigate  "),
            Span::styled("Enter", key_style),
            Span::raw(" continue  "),
            Span::styled("a", key_style),
            Span::raw(" new thread"),
        ])
    } else {
        let mut spans = vec![
            Span::styled("Esc", key_style),
            Span::raw(" back  "),
            Span::styled("j", key_style),
            Span::raw("/"),
            Span::styled("k", key_style),
            Span::raw(" scroll"),
        ];
        if !app.detail_threads.is_empty() {
            spans.push(Span::raw("  "));
            spans.push(Span::styled("Tab", key_style));
            spans.push(Span::raw(" threads"));
        }
        spans.push(Span::raw("  "));
        spans.push(Span::styled("a", key_style));
        spans.push(Span::raw(" new thread"));
        Line::from(spans)
    };
    frame.render_widget(Paragraph::new(bar), chunks[3]);

    // Workspace picker modal
    if let Some(ref picker) = app.workspace_picker {
        // Extra line for the input row when typing, or the hint row
        let extra_lines: u16 = 1;
        let content_lines = picker.options.len() as u16 + extra_lines;
        let picker_height = (content_lines + 2).min(area.height.saturating_sub(4));
        let picker_width = picker
            .options
            .iter()
            .map(|s| s.len() as u16)
            .max()
            .unwrap_or(20)
            .max(20)
            + 4;
        let picker_width = picker_width.min(area.width.saturating_sub(4));

        let x = area.x + (area.width.saturating_sub(picker_width)) / 2;
        let y = area.y + (area.height.saturating_sub(picker_height)) / 2;
        let popup_area = Rect::new(x, y, picker_width, picker_height);

        frame.render_widget(Clear, popup_area);

        let mut lines: Vec<Line> = picker
            .options
            .iter()
            .enumerate()
            .map(|(i, ws)| {
                let style = if !picker.typing && i == picker.selected {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };
                Line::from(Span::styled(format!(" {} ", ws), style))
            })
            .collect();

        if picker.typing {
            lines.push(Line::from(vec![
                Span::raw(" /"),
                Span::styled(&picker.input, Style::default().add_modifier(Modifier::BOLD)),
                Span::raw("▏"),
            ]));
        } else {
            let key_style = Style::default().fg(Color::DarkGray);
            lines.push(Line::from(Span::styled(" / to type a path", key_style)));
        }

        let popup = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Select workspace")
                .border_style(Style::default().fg(Color::Cyan)),
        );
        frame.render_widget(popup, popup_area);
    }
}

fn status_style(status: &str) -> Style {
    match status {
        "In Progress" => Style::default().fg(Color::Yellow),
        "Todo" => Style::default().fg(Color::Cyan),
        "Done" => Style::default().fg(Color::Green),
        "Canceled" | "Cancelled" => Style::default().fg(Color::DarkGray),
        "In Review" => Style::default().fg(Color::Magenta),
        "Backlog" | "Triage" => Style::default().fg(Color::DarkGray),
        _ => Style::default(),
    }
}

fn priority_style(priority: &str) -> Style {
    match priority {
        "Urgent" => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        "High" => Style::default().fg(Color::Red),
        "Medium" => Style::default().fg(Color::Yellow),
        "Low" => Style::default().fg(Color::DarkGray),
        _ => Style::default(),
    }
}
