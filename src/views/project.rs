use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
};

use crate::api::client::LinearApi;
use crate::app::{App, SortDirection};

pub fn render<A: LinearApi>(frame: &mut Frame, area: Rect, app: &App<A>) {
    let project_name = app
        .selected_project()
        .map(|p| p.name.as_str())
        .unwrap_or("Project");

    if app.loading {
        let loading = Paragraph::new(Line::from(Span::styled(
            format!("Loading {} issues…", project_name),
            Style::default().fg(Color::Yellow),
        )))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(project_name.to_string()),
        );
        frame.render_widget(loading, area);
        return;
    }

    let chunks = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(area);

    let header = Row::new(vec![
        Cell::from("ID").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Title").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Status").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Priority").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Assignee").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Amp").style(Style::default().add_modifier(Modifier::BOLD)),
    ]);

    let issues = app.filtered_project_issues();

    let rows: Vec<Row> = issues
        .iter()
        .enumerate()
        .map(|(idx, issue)| {
            let is_multi_selected = app.selected_indices.contains(&idx);
            let base_style = if is_multi_selected {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default()
            };
            let thread_display = app.thread_count_display(&issue.identifier);
            let thread_style = if thread_display.contains('/') {
                Style::default().fg(Color::Green)
            } else if thread_display == "-" {
                Style::default().fg(Color::DarkGray)
            } else {
                base_style
            };
            let select_marker = if is_multi_selected { "▌ " } else { "  " };
            Row::new(vec![
                Cell::from(Line::from(vec![
                    Span::styled(select_marker, base_style),
                    Span::styled(issue.identifier.as_str(), base_style),
                ])),
                Cell::from(Span::styled(issue.title.as_str(), base_style)),
                Cell::from(Span::styled(
                    issue.status_str(),
                    if is_multi_selected {
                        base_style
                    } else {
                        status_style(issue.status_str())
                    },
                )),
                Cell::from(Span::styled(
                    issue.priority_str(),
                    if is_multi_selected {
                        base_style
                    } else {
                        priority_style(issue.priority_str())
                    },
                )),
                Cell::from(Span::styled(
                    issue.assignee.as_ref().map_or("—", |a| a.name.as_str()),
                    base_style,
                )),
                Cell::from(Span::styled(thread_display, thread_style)),
            ])
        })
        .collect();

    let sort_indicator = match &app.sort {
        Some((col, dir)) => {
            let arrow = match dir {
                SortDirection::Asc => "↑",
                SortDirection::Desc => "↓",
            };
            format!(" [sort: {} {}]", col.label(), arrow)
        }
        None => String::new(),
    };

    let title = format!(
        "{} — Issues ({}){}{}",
        project_name,
        issues.len(),
        sort_indicator,
        if app.selected_indices.len() > 1 {
            format!(" [{} selected]", app.selected_indices.len())
        } else {
            String::new()
        }
    );

    let table = Table::new(
        rows,
        [
            Constraint::Length(18),
            Constraint::Min(30),
            Constraint::Length(15),
            Constraint::Length(10),
            Constraint::Length(20),
            Constraint::Length(5),
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::ALL).title(title))
    .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    let mut table_state = TableState::default();
    if !issues.is_empty() {
        table_state.select(Some(app.project_issue_selected));
    }

    frame.render_stateful_widget(table, chunks[0], &mut table_state);

    let key_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);

    if let Some(ref err) = app.error {
        let line = Line::from(vec![
            Span::styled(
                "Error: ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::styled(&err.message, Style::default().fg(Color::Red)),
            Span::styled(
                " (press Esc to dismiss)",
                Style::default().fg(Color::DarkGray),
            ),
        ]);
        frame.render_widget(Paragraph::new(line), chunks[1]);
    } else if app.refreshing {
        let line = Line::from(Span::styled(
            "Refreshing...",
            Style::default().fg(Color::Yellow),
        ));
        frame.render_widget(Paragraph::new(line), chunks[1]);
    } else if let Some((ref msg, _)) = app.flash {
        let line = Line::from(Span::styled(
            msg.as_str(),
            Style::default().fg(Color::Green),
        ));
        frame.render_widget(Paragraph::new(line), chunks[1]);
    } else if app.awaiting_open {
        let line = Line::from(vec![
            Span::raw("open in: "),
            Span::styled("l", key_style),
            Span::raw("inear  "),
            Span::styled("g", key_style),
            Span::raw("ithub PR"),
        ]);
        frame.render_widget(Paragraph::new(line), chunks[1]);
    } else if app.awaiting_state_change {
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
        frame.render_widget(Paragraph::new(Line::from(spans)), chunks[1]);
    } else if app.awaiting_sort {
        let column_hints = vec![
            Span::styled("i", key_style),
            Span::raw("d  "),
            Span::styled("t", key_style),
            Span::raw("itle  "),
            Span::styled("s", key_style),
            Span::raw("tatus  p"),
            Span::styled("r", key_style),
            Span::raw("iority"),
        ];
        let mut spans = vec![Span::raw("sort by: ")];
        spans.extend(column_hints);
        frame.render_widget(Paragraph::new(Line::from(spans)), chunks[1]);
    } else if app.awaiting_quit {
        let line = Line::from(vec![
            Span::raw("Press "),
            Span::styled("q", key_style),
            Span::raw(" again to quit"),
        ]);
        frame.render_widget(Paragraph::new(line), chunks[1]);
    } else {
        let sep = Span::styled(" │ ", Style::default().fg(Color::DarkGray));
        let line = Line::from(vec![
            Span::styled("s", key_style),
            Span::raw("ort  "),
            Span::styled("i", key_style),
            Span::raw(" assign  "),
            Span::styled("o", key_style),
            Span::raw("pen  "),
            Span::styled("m", key_style),
            Span::raw("ove  "),
            Span::styled("r", key_style),
            Span::raw("efresh"),
            sep.clone(),
            Span::styled("Esc", key_style),
            Span::raw(" back  "),
            Span::styled("?", key_style),
            Span::raw("help"),
            sep,
            Span::styled("Enter", key_style),
            Span::raw(" open"),
        ]);
        frame.render_widget(Paragraph::new(line), chunks[1]);
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
