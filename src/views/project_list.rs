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
    if app.loading {
        let loading = Paragraph::new(Line::from(Span::styled(
            "Loading projects…",
            Style::default().fg(Color::Yellow),
        )))
        .block(Block::default().borders(Borders::ALL).title("Projects"));
        frame.render_widget(loading, area);
        return;
    }

    let chunks = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(area);

    let header = Row::new(vec![
        Cell::from("Name").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Status").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Lead").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from("Progress").style(Style::default().add_modifier(Modifier::BOLD)),
    ]);

    let projects = app.sorted_projects();

    let rows: Vec<Row> = projects
        .iter()
        .map(|project| {
            Row::new(vec![
                Cell::from(project.name.as_str()),
                Cell::from(Span::styled(
                    project.status_str(),
                    project_status_style(project.status_str()),
                )),
                Cell::from(project.lead_str()),
                Cell::from(Span::styled(
                    project.progress_percent(),
                    progress_style(project.progress),
                )),
            ])
        })
        .collect();

    let title = format!("Projects ({})", projects.len());

    let table = Table::new(
        rows,
        [
            Constraint::Min(30),
            Constraint::Length(15),
            Constraint::Length(20),
            Constraint::Length(10),
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::ALL).title(title))
    .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    let mut table_state = TableState::default();
    if !app.projects.is_empty() {
        table_state.select(Some(app.project_selected));
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
    } else if app.awaiting_sort {
        let sort_hints = vec![
            Span::raw("sort by: "),
            Span::styled("n", key_style),
            Span::raw("ame  "),
            Span::styled("s", key_style),
            Span::raw("tatus  "),
            Span::styled("l", key_style),
            Span::raw("ead  "),
            Span::styled("p", key_style),
            Span::raw("rogress"),
        ];
        frame.render_widget(Paragraph::new(Line::from(sort_hints)), chunks[1]);
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
            Span::styled("o", key_style),
            Span::raw("pen  "),
            Span::styled("r", key_style),
            Span::raw("efresh"),
            sep.clone(),
            Span::styled("Esc", key_style),
            Span::raw(" issues"),
            sep,
            Span::styled("?", key_style),
            Span::raw("help"),
        ]);
        frame.render_widget(Paragraph::new(line), chunks[1]);
    }
}

fn project_status_style(status: &str) -> Style {
    match status {
        "planned" => Style::default().fg(Color::Cyan),
        "started" | "backlog" => Style::default().fg(Color::Yellow),
        "paused" => Style::default().fg(Color::DarkGray),
        "completed" => Style::default().fg(Color::Green),
        "canceled" | "cancelled" => Style::default().fg(Color::DarkGray),
        _ => Style::default(),
    }
}

fn progress_style(progress: Option<f64>) -> Style {
    match progress {
        Some(p) if p >= 0.75 => Style::default().fg(Color::Green),
        Some(p) if p >= 0.25 => Style::default().fg(Color::Yellow),
        Some(_) => Style::default().fg(Color::Red),
        None => Style::default().fg(Color::DarkGray),
    }
}
