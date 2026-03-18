use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::api::client::LinearApi;
use crate::app::App;

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

    let chunks = Layout::vertical([
        Constraint::Length(meta_height),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(area);

    // Metadata box
    let title = Line::from(vec![
        Span::styled(
            format!("{}", issue.identifier),
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
    if let Some(comments) = &issue.comments {
        if !comments.nodes.is_empty() {
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
    }

    let content_lines = body_lines.len() as u16;
    let inner_height = chunks[1].height.saturating_sub(2); // borders
    let scroll = app.detail_scroll;

    let detail = Paragraph::new(body_lines)
        .block(Block::default().borders(Borders::ALL).title("Description"))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));

    frame.render_widget(detail, chunks[1]);

    app.detail_scroll_max = content_lines.saturating_sub(inner_height);

    // Status bar
    let key_style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let bar = if app.awaiting_quit {
        Line::from(vec![
            Span::raw("Press "),
            Span::styled("q", key_style),
            Span::raw(" again to quit"),
        ])
    } else {
        Line::from(vec![
            Span::styled("Esc", key_style),
            Span::raw(" back  "),
            Span::styled("j", key_style),
            Span::raw("/"),
            Span::styled("k", key_style),
            Span::raw(" scroll"),
        ])
    };
    frame.render_widget(Paragraph::new(bar), chunks[2]);
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
