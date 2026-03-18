use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
};

use crate::api::client::LinearApi;
use crate::app::App;

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

    let show_bar = app.refreshing || app.awaiting_quit || app.awaiting_state_change || app.error.is_some();
    let chunks = if show_bar {
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(area)
    } else {
        Layout::vertical([Constraint::Min(0)]).split(area)
    };

    let header = Row::new(vec![
        Cell::from("ID").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Title").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Status").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Priority").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Assignee").style(Style::default().add_modifier(Modifier::BOLD)),
    ]);

    let rows: Vec<Row> = app
        .project_issues
        .iter()
        .map(|issue| {
            Row::new(vec![
                Cell::from(issue.identifier.as_str()),
                Cell::from(issue.title.as_str()),
                Cell::from(Span::styled(
                    issue.status_str(),
                    status_style(issue.status_str()),
                )),
                Cell::from(Span::styled(
                    issue.priority_str(),
                    priority_style(issue.priority_str()),
                )),
                Cell::from(issue.assignee.as_ref().map_or("—", |a| a.name.as_str())),
            ])
        })
        .collect();

    let title = format!("{} — Issues ({})", project_name, app.project_issues.len());

    let table = Table::new(
        rows,
        [
            Constraint::Length(10),
            Constraint::Min(30),
            Constraint::Length(15),
            Constraint::Length(10),
            Constraint::Length(20),
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::ALL).title(title))
    .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    let mut table_state = TableState::default();
    if !app.project_issues.is_empty() {
        table_state.select(Some(app.project_issue_selected));
    }

    frame.render_stateful_widget(table, chunks[0], &mut table_state);

    let key_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);

    if show_bar {
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
        } else if app.awaiting_state_change {
            let mut spans = vec![Span::raw("move to: ")];
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
        } else if app.awaiting_quit {
            let line = Line::from(vec![
                Span::raw("Press "),
                Span::styled("q", key_style),
                Span::raw(" again to quit"),
            ]);
            frame.render_widget(Paragraph::new(line), chunks[1]);
        }
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
