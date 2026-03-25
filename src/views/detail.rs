use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::amp::output::OutputKind;
use crate::amp::state::SessionRunStatus;
use crate::api::client::LinearApi;
use crate::app::{App, DetailSection};

fn clamp_scroll(scroll: u16, max_scroll: u16) -> u16 {
    if scroll == u16::MAX {
        max_scroll
    } else {
        scroll.min(max_scroll)
    }
}

pub fn render<A: LinearApi>(frame: &mut Frame, area: Rect, app: &mut App<A>) {
    if app.detail_section == DetailSection::RunLog {
        render_run_log(frame, area, app);
        return;
    }

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

    let assignee = issue.assignee.as_ref().map_or("—", |a| a.name.as_str());
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
        // Empty state: border (2) + hint line (1)
        3
    };

    // Output section: show when viewing output, or when there's output for the selected thread
    let has_output = app.detail_section == DetailSection::Output;
    let input_bar_height: u16 = if app.message_input_active { 1 } else { 0 };
    let output_height: u16 = if has_output {
        // Take roughly half the remaining space
        let available = area
            .height
            .saturating_sub(meta_height)
            .saturating_sub(threads_height)
            .saturating_sub(input_bar_height)
            .saturating_sub(1);
        available / 2
    } else {
        0
    };

    let chunks = Layout::vertical([
        Constraint::Length(meta_height),
        Constraint::Min(0),
        Constraint::Length(output_height),
        Constraint::Length(input_bar_height),
        Constraint::Length(threads_height),
        Constraint::Length(1),
    ])
    .split(area);

    // Metadata box
    let title = Line::from(vec![
        Span::styled(
            issue.identifier.to_string(),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(" — {}", issue.title)),
    ]);
    let meta =
        Paragraph::new(meta_lines).block(Block::default().borders(Borders::ALL).title(title));
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
            let author = comment.user.as_ref().map_or("Unknown", |u| u.name.as_str());
            body_lines.push(Line::from(vec![
                Span::styled(
                    author,
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
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
    let detail_scroll_max = content_lines.saturating_sub(inner_height);
    let scroll = clamp_scroll(app.detail_scroll, detail_scroll_max);

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
    app.detail_scroll_max = detail_scroll_max;
    app.detail_scroll = scroll;

    // Output section
    if has_output {
        let output_display: Vec<Line> = app
            .selected_thread_output()
            .iter()
            .map(|ol| {
                let style = match ol.kind {
                    OutputKind::User => Style::default().fg(Color::Cyan),
                };
                Line::from(Span::styled(ol.text.clone(), style))
            })
            .collect();

        let output_line_count = output_display.len() as u16;
        let output_inner = chunks[2].height.saturating_sub(2);
        let output_scroll_max = output_line_count.saturating_sub(output_inner);
        let output_scroll = clamp_scroll(app.detail_output_scroll, output_scroll_max);

        let session_indicator = app
            .selected_thread()
            .and_then(|t| app.run_status_for_thread(&t.id))
            .map(|s| format!(" [{}]", s.label()))
            .unwrap_or_default();
        let output_title = format!("Output{session_indicator}");

        let output_block = Paragraph::new(output_display)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(output_title)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .wrap(Wrap { trim: false })
            .scroll((output_scroll, 0));
        frame.render_widget(output_block, chunks[2]);
        app.detail_output_scroll_max = output_scroll_max;
        app.detail_output_scroll = output_scroll;
    }

    // Message input bar
    if app.message_input_active {
        let input_line = Line::from(vec![
            Span::styled("▶ ", Style::default().fg(Color::Cyan)),
            Span::raw(&app.message_input),
            Span::raw("▏"),
        ]);
        frame.render_widget(Paragraph::new(input_line), chunks[3]);
    }

    // Threads section (always shown)
    {
        let threads_focused = app.detail_section == DetailSection::Threads;
        let threads_border_style = if threads_focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let thread_lines: Vec<Line> = if thread_count > 0 {
            app.detail_threads
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
                    let mut spans = vec![
                        Span::styled(format!("  {} ", t.title), style),
                        Span::styled(
                            format!("({} msgs) ", t.message_count),
                            style.fg(Color::DarkGray),
                        ),
                        Span::styled(time_str, style.fg(Color::DarkGray)),
                    ];
                    if let Some(status) = app.run_status_for_thread(&t.id) {
                        spans.push(Span::raw(" "));
                        spans.push(Span::styled(
                            format!("[{}]", status.label()),
                            run_status_style(status),
                        ));
                    }
                    Line::from(spans)
                })
                .collect()
        } else {
            vec![Line::from(Span::styled(
                "  No threads yet — press a to start one",
                Style::default().fg(Color::DarkGray),
            ))]
        };

        let (running, pending) = app.active_run_counts();
        let threads_title = if thread_count > 0 && (running > 0 || pending > 0) {
            let mut parts = vec![format!("Threads ({})", thread_count)];
            if running > 0 {
                parts.push(format!("{} running", running));
            }
            if pending > 0 {
                parts.push(format!("{} pending", pending));
            }
            parts.join(" · ")
        } else {
            format!("Threads ({})", thread_count)
        };

        let threads_block = Paragraph::new(thread_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(threads_title)
                .border_style(threads_border_style),
        );
        frame.render_widget(threads_block, chunks[4]);
    }

    // Status bar
    let key_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let bar = if app.awaiting_state_change {
        let mut spans = vec![Span::raw("move to: ")];
        if !app.state_type_ahead.is_empty() {
            spans.push(Span::styled(
                format!("[{}] ", app.state_type_ahead),
                Style::default().fg(Color::Cyan),
            ));
        }
        for (i, state) in app.state_options.iter().enumerate() {
            if i == app.state_selected {
                spans.push(Span::styled(
                    state.as_str(),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                spans.push(Span::raw(state.as_str()));
            }
            if i < app.state_options.len() - 1 {
                spans.push(Span::raw("  "));
            }
        }
        Line::from(spans)
    } else if let Some((ref msg, _)) = app.flash {
        Line::from(Span::styled(
            msg.as_str(),
            Style::default().fg(Color::Green),
        ))
    } else if app.awaiting_quit {
        Line::from(vec![
            Span::raw("Press "),
            Span::styled("q", key_style),
            Span::raw(" again to quit"),
        ])
    } else if app.awaiting_open {
        Line::from(vec![
            Span::raw("open in: "),
            Span::styled("l", key_style),
            Span::raw("inear  "),
            Span::styled("g", key_style),
            Span::raw("ithub PR"),
        ])
    } else if app.message_input_active {
        Line::from(vec![
            Span::styled("Enter", key_style),
            Span::raw(" send  "),
            Span::styled("Esc", key_style),
            Span::raw(" cancel"),
        ])
    } else if app.detail_section == DetailSection::Output {
        let mut spans = vec![Span::styled("Esc", key_style), Span::raw(" back")];
        spans.push(Span::raw("  "));
        spans.push(Span::styled("j", key_style));
        spans.push(Span::raw("/"));
        spans.push(Span::styled("k", key_style));
        spans.push(Span::raw(" scroll"));
        spans.push(Span::raw("  "));
        spans.push(Span::styled("G", key_style));
        spans.push(Span::raw(" bottom"));
        spans.push(Span::raw("  "));
        spans.push(Span::styled("i", key_style));
        spans.push(Span::raw(" instruct"));
        Line::from(spans)
    } else if app.detail_section == DetailSection::Threads {
        let mut spans = vec![Span::styled("Esc", key_style), Span::raw(" back")];
        if app.detail_threads.len() > 1 {
            spans.push(Span::raw("  "));
            spans.push(Span::styled("j", key_style));
            spans.push(Span::raw("/"));
            spans.push(Span::styled("k", key_style));
            spans.push(Span::raw(" navigate"));
        }
        if app.selected_thread_run().is_some() {
            spans.push(Span::raw("  "));
            spans.push(Span::styled("l", key_style));
            spans.push(Span::raw(" log  "));
            spans.push(Span::styled("x", key_style));
            spans.push(Span::raw(" stale  "));
        }
        if app.selected_thread().is_some_and(|t| app.output_buffer.line_count(&t.id) > 0) {
            spans.push(Span::styled("o", key_style));
            spans.push(Span::raw(" output  "));
        }
        spans.push(Span::styled("a", key_style));
        spans.push(Span::raw(" new thread"));
        Line::from(spans)
    } else {
        let can_scroll = content_lines > inner_height;
        let mut spans = vec![Span::styled("Esc", key_style), Span::raw(" back")];
        if can_scroll {
            spans.push(Span::raw("  "));
            spans.push(Span::styled("j", key_style));
            spans.push(Span::raw("/"));
            spans.push(Span::styled("k", key_style));
            spans.push(Span::raw(" scroll"));
        }
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
    frame.render_widget(Paragraph::new(bar), chunks[5]);

    // Workspace picker modal
    if let Some(ref picker) = app.workspace_picker {
        // Extra line for the input row when typing, or the hint row
        let extra_lines: u16 = 1;
        let content_lines = picker.options.len() as u16 + extra_lines;
        let picker_height = (content_lines + 2).min(area.height.saturating_sub(4));
        let input_width = if picker.typing {
            // " " + input + "▏" + "  tab to complete"
            picker.input.len() as u16 + 21
        } else {
            0
        };
        let picker_width = picker
            .options
            .iter()
            .map(|s| s.len() as u16)
            .max()
            .unwrap_or(20)
            .max(input_width)
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
                let style = if i == picker.selected {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };
                Line::from(Span::styled(format!(" {} ", ws), style))
            })
            .collect();

        if picker.typing {
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled(&picker.input, Style::default().add_modifier(Modifier::BOLD)),
                Span::raw("▏"),
                Span::styled("  tab to complete", Style::default().fg(Color::DarkGray)),
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

fn render_run_log<A: LinearApi>(frame: &mut Frame, area: Rect, app: &mut App<A>) {
    let chunks = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(area);

    let log_lines: Vec<Line> = app
        .run_log_lines
        .iter()
        .map(|l| Line::raw(l.as_str()))
        .collect();

    let line_count = log_lines.len() as u16;
    let inner_height = chunks[0].height.saturating_sub(2);
    let run_log_scroll_max = line_count.saturating_sub(inner_height);
    app.run_log_scroll_max = run_log_scroll_max;
    let run_log_scroll = clamp_scroll(app.run_log_scroll, run_log_scroll_max);
    app.run_log_scroll = run_log_scroll;

    let log_block = Paragraph::new(log_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Run Log")
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .wrap(Wrap { trim: false })
        .scroll((run_log_scroll, 0));
    frame.render_widget(log_block, chunks[0]);

    let key_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let bar = Line::from(vec![
        Span::styled("Esc", key_style),
        Span::raw(" back  "),
        Span::styled("j", key_style),
        Span::raw("/"),
        Span::styled("k", key_style),
        Span::raw(" scroll  "),
        Span::styled("G", key_style),
        Span::raw(" bottom  "),
        Span::styled("g", key_style),
        Span::raw(" top"),
    ]);
    frame.render_widget(Paragraph::new(bar), chunks[1]);
}

fn run_status_style(status: SessionRunStatus) -> Style {
    match status {
        SessionRunStatus::Running => Style::default().fg(Color::Green),
        SessionRunStatus::Pending => Style::default().fg(Color::Yellow),
        SessionRunStatus::Failed => Style::default().fg(Color::Red),
        SessionRunStatus::Stale => Style::default().fg(Color::DarkGray),
        SessionRunStatus::Completed => Style::default().fg(Color::Cyan),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_status_style_projects_each_lifecycle_state() {
        assert_eq!(
            run_status_style(SessionRunStatus::Running).fg,
            Some(Color::Green)
        );
        assert_eq!(
            run_status_style(SessionRunStatus::Pending).fg,
            Some(Color::Yellow)
        );
        assert_eq!(
            run_status_style(SessionRunStatus::Failed).fg,
            Some(Color::Red)
        );
        assert_eq!(
            run_status_style(SessionRunStatus::Stale).fg,
            Some(Color::DarkGray)
        );
        assert_eq!(
            run_status_style(SessionRunStatus::Completed).fg,
            Some(Color::Cyan)
        );
    }

    #[test]
    fn clamp_scroll_handles_follow_bottom_sentinel() {
        assert_eq!(clamp_scroll(u16::MAX, 42), 42);
    }

    #[test]
    fn clamp_scroll_caps_regular_scroll_to_max() {
        assert_eq!(clamp_scroll(99, 5), 5);
        assert_eq!(clamp_scroll(3, 5), 3);
    }
}
